/*
 * The web management server.
 *
 * # What it is for
 *
 * A browser on the same network is a better keyboard than a five-centimetre panel: a Wi-Fi password
 * typed on a phone is typed once, and a file dragged onto a page is a file the box has. So the box
 * carries a small HTTP server — `esp_http_server`, started on demand by the app of the same name and
 * stopped by it — and a page that drives it.
 *
 * # The page lives in the image, not on the filesystem
 *
 * `web_page.html` is embedded into this component at build time (see `CMakeLists.txt`). Serving it
 * from `/internal/www` would be the other way round and the wrong one: the filesystem is the thing a
 * person edits through this very page, so a page kept there is a page that can be deleted,
 * half-written, or simply absent on a board whose `internal` partition was never flashed — and the
 * answer is a 404 on the one URL that has no other way to be reached.
 *
 * # Why the bodies are raw and the answers are JSON
 *
 * There is no multipart parser here and no form encoding. An upload is the file's bytes as the whole
 * request body, with the destination in the query string; everything else answers `{"ok":true}` or
 * `{"error":"<code>"}`. The page is the only client and it is written in JavaScript on the other side
 * of this interface, so `fetch` sends a `File` as a body without being asked twice; a multipart
 * parser would be two hundred lines of C to un-say that. The error *codes* are ASCII and the page owns
 * their wording, which keeps every human-readable string in the one file that is HTML. The one upload
 * that is more than a body is the one that would replace a file: a name that is taken is refused with
 * `409 already_exists` unless the caller says `overwrite=1`, so that the answer is a question the
 * person at the browser gets asked, and `handle_upload` is where the rest of that is written down.
 *
 * # No authentication, on purpose
 *
 * Anyone on the same network can read and write these files and change this network. That is what the
 * box is for — it is the same trust the panel has, one room over — and it is worth saying out loud
 * rather than discovering: the server is bound to `0.0.0.0`, it has no password, and it is off until
 * the app on the panel starts it. A board on a network it does not control should leave it off.
 */

#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

#include "esp_app_desc.h"
#include "esp_chip_info.h"
#include "esp_heap_caps.h"
#include "esp_http_server.h"
#include "esp_log.h"
#include "esp_netif.h"
#include "esp_system.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "lwip/inet.h"
#include "lwip/netdb.h"
#include "lwip/sockets.h"

#include "board_hal.h"

static const char *TAG = "board_web";

/* ---------------------------------------------------------------------------
 * Bounds
 *
 * Each of these is a limit on what one request may cost. They bound the *page's* behaviour as much as
 * a hostile caller's: a directory of ten thousand files is asked for one page at a time, and a body
 * longer than an audio file is not what this is for.
 * ------------------------------------------------------------------------- */

#define WEB_PATH_MAX      256
#define WEB_QUERY_MAX     512
#define WEB_NAME_MAX      128
#define WEB_MAX_ENTRIES   200
#define WEB_MAX_APS       20
#define WEB_JSON_MAX      (48 * 1024)
#define WEB_CHUNK         1024
#define WEB_PORT_DEFAULT  80

/* The page's bytes, put in the image by `EMBED_FILES` in `CMakeLists.txt`. `EMBED_FILES` and not
 * `EMBED_TXTFILES`: the exact length is `end - start`, with no terminator appended to subtract. */
extern const uint8_t web_page_html_start[] asm("_binary_web_page_html_start");
extern const uint8_t web_page_html_end[] asm("_binary_web_page_html_end");

/* ---------------------------------------------------------------------------
 * Server lifetime
 * ------------------------------------------------------------------------- */

static httpd_handle_t s_server = NULL;
static uint16_t       s_port   = WEB_PORT_DEFAULT;

/* The handlers run on the server's own tasks and the app starts and stops it from another, so the
 * handle is read and written under a lock rather than assumed to be one or the other. */
static portMUX_TYPE s_lock = portMUX_INITIALIZER_UNLOCKED;

/* What the panel is showing, named by the Rust launcher through hal_web_set_app_name(). The server
 * cannot look this up: the foreground is iced's decision, one of the two things C does not own. */
static char s_app_name[64] = "pomelo";

void hal_web_set_app_name(const char *name)
{
    if (name == NULL || name[0] == '\0') {
        return;
    }

    /* Copied under the same lock the handler-side readers use: a name arriving mid-answer would
     * otherwise be half of one name and half of another. */
    portENTER_CRITICAL(&s_lock);
    snprintf(s_app_name, sizeof s_app_name, "%s", name);
    portEXIT_CRITICAL(&s_lock);
}

static void app_name_copy(char *out, size_t cap)
{
    portENTER_CRITICAL(&s_lock);
    snprintf(out, cap, "%s", s_app_name);
    portEXIT_CRITICAL(&s_lock);
}

/* When this server last answered anything, for the idle sleep below. Written from every handler
 * through the two functions that end every answer — send_json and handle_page — which is every
 * request this server has. */
static volatile int64_t s_last_request_us = 0;

bool hal_web_is_running(void)
{
    portENTER_CRITICAL(&s_lock);
    bool running = s_server != NULL;
    portEXIT_CRITICAL(&s_lock);

    return running;
}

uint16_t hal_web_get_port(void)
{
    portENTER_CRITICAL(&s_lock);
    uint16_t port = s_server ? s_port : 0;
    portEXIT_CRITICAL(&s_lock);

    return port;
}

/* ---------------------------------------------------------------------------
 * Small helpers: buffers, escaping, query strings, paths
 * ------------------------------------------------------------------------- */

/* A grown-on-demand string. Every JSON answer is built in one of these: the size of a directory is
 * not known until it has been read, and a fixed buffer would either be a limit a person runs into or
 * a third of the heap spent on the answer nobody asked for. */
typedef struct {
    char  *data;
    size_t len;
    size_t cap;
    bool   full;
} web_buf_t;

static void buf_init(web_buf_t *buf, size_t cap)
{
    buf->data = heap_caps_malloc(cap, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (buf->data == NULL) {
        buf->data = malloc(cap);
    }

    buf->cap  = buf->data ? cap : 0;
    buf->len  = 0;
    buf->full = buf->data == NULL;

    if (buf->data) {
        buf->data[0] = '\0';
    }
}

static void buf_free(web_buf_t *buf)
{
    free(buf->data);
    buf->data = NULL;
    buf->len = buf->cap = 0;
}

/* Back to empty, keeping the allocation. For the one caller that builds many small answers in a row
 * — the event stream, which frames a line, sends it, and frames the next — where re-allocating each
 * time would be a heap churn with nothing to show for it.
 *
 * `full` is cleared with the length: it means "this answer hit the ceiling", and an answer that has
 * been emptied has not. */
static void buf_reset(web_buf_t *buf)
{
    buf->len  = 0;
    buf->full = false;

    if (buf->data != NULL) {
        buf->data[0] = '\0';
    }
}

static bool buf_reserve(web_buf_t *buf, size_t extra)
{
    if (buf->full) {
        return false;
    }

    if (buf->len + extra + 1 <= buf->cap) {
        return true;
    }

    size_t cap = buf->cap ? buf->cap : 512;
    while (cap < buf->len + extra + 1) {
        cap *= 2;
    }

    if (cap > WEB_JSON_MAX) {
        buf->full = true;
        return false;
    }

    void *grown = heap_caps_realloc(buf->data, cap, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (grown == NULL) {
        grown = realloc(buf->data, cap);
    }

    if (grown == NULL) {
        buf->full = true;
        return false;
    }

    buf->data = grown;
    buf->cap  = cap;

    return true;
}

static void buf_puts(web_buf_t *buf, const char *text)
{
    size_t len = strlen(text);
    if (!buf_reserve(buf, len)) {
        return;
    }

    memcpy(buf->data + buf->len, text, len + 1);
    buf->len += len;
}

static void buf_printf(web_buf_t *buf, const char *format, ...)
{
    char    stack[256];
    va_list args;

    va_start(args, format);
    int written = vsnprintf(stack, sizeof stack, format, args);
    va_end(args);

    if (written < 0) {
        return;
    }

    if ((size_t)written < sizeof stack) {
        buf_puts(buf, stack);
        return;
    }

    /* Longer than the scratch buffer, which only an escaped path can be: build it properly. */
    if (!buf_reserve(buf, (size_t)written)) {
        return;
    }

    va_start(args, format);
    vsnprintf(buf->data + buf->len, buf->cap - buf->len, format, args);
    va_end(args);

    buf->len += (size_t)written;
    buf->data[buf->len] = '\0';
}

/* One JSON string, quoted, with the characters JSON has that a filename may. Control characters are
 * dropped rather than encoded: a filename is drawn in a browser, and a name that is not there reads
 * better than one with a `\u000a` in the middle of it. */
static void buf_json(web_buf_t *buf, const char *text)
{
    buf_puts(buf, "\"");

    for (const unsigned char *c = (const unsigned char *)text; *c; c++) {
        switch (*c) {
        case '"':  buf_puts(buf, "\\\""); break;
        case '\\': buf_puts(buf, "\\\\"); break;
        case '\n': buf_puts(buf, "\\n");  break;
        case '\r': buf_puts(buf, "\\r");  break;
        case '\t': buf_puts(buf, "\\t");  break;
        default:
            if (*c < 0x20) {
                break;
            }

            /* The bytes of a UTF-8 name are passed through as they are: it is one stream of bytes in
             * and out, and re-encoding it here would be this layer deciding what a name is. */
            if (!buf_reserve(buf, 1)) {
                return;
            }

            buf->data[buf->len++] = (char)*c;
            buf->data[buf->len]   = '\0';
        }
    }

    buf_puts(buf, "\"");
}

/* Percent-decoding, as `fetch` encodes it — plus `+` for a space, which is what a browser's address
 * bar has always done. */
static size_t url_decode(const char *in, char *out, size_t cap)
{
    size_t used = 0;

    for (size_t i = 0; in[i] != '\0' && used + 1 < cap; i++) {
        char c = in[i];

        if (c == '+') {
            c = ' ';
        } else if (c == '%' && isxdigit((unsigned char)in[i + 1]) && isxdigit((unsigned char)in[i + 2])) {
            char hex[3] = { in[i + 1], in[i + 2], '\0' };
            c = (char)strtol(hex, NULL, 16);
            i += 2;
        }

        out[used++] = c;
    }

    out[used] = '\0';

    return used;
}

/* One query parameter, decoded. `false` means it was not in the query at all. */
static bool query_string(httpd_req_t *req, const char *key, char *out, size_t cap)
{
    char query[WEB_QUERY_MAX];
    char raw[WEB_QUERY_MAX];

    if (httpd_req_get_url_query_str(req, query, sizeof query) != ESP_OK) {
        return false;
    }

    if (httpd_query_key_value(query, key, raw, sizeof raw) != ESP_OK) {
        return false;
    }

    url_decode(raw, out, cap);

    return true;
}

/* The mount points this box can be asked about, in the order a page shows them. */
static size_t web_roots(hal_storage_volume_t *out, size_t max)
{
    size_t count = 0;

    if (count < max && hal_storage_get_internal(&out[count]) == ESP_OK) {
        count++;
    }

    if (count < max && hal_storage_get_card(&out[count]) == ESP_OK) {
        count++;
    }

    return count;
}

/* The volume a path is on, for the numbers in a listing's header. */
static bool volume_for(const char *path, hal_storage_volume_t *out)
{
    hal_storage_volume_t roots[2];
    size_t               count = web_roots(roots, 2);

    for (size_t i = 0; i < count; i++) {
        size_t len = strlen(roots[i].mount_point);
        if (strncmp(path, roots[i].mount_point, len) == 0 && (path[len] == '\0' || path[len] == '/')) {
            *out = roots[i];
            return true;
        }
    }

    return false;
}

/* What a path is allowed to be: one of the mounted volumes, and never above one.
 *
 * The check is on the *shape* rather than on a whitelist of names, because the whole point of a file
 * manager is that it walks into directories this file has never heard of. What it must not do is walk
 * out: `..` is refused anywhere in the path, and a path that does not start at a mount point is not a
 * path this box has. A trailing slash is dropped, so that "/internal/" and "/internal" are one answer
 * to everything below. */
static bool path_clean(const char *path, char *out, size_t cap)
{
    if (path == NULL || path[0] != '/' || strstr(path, "..") != NULL) {
        return false;
    }

    size_t used = 0;
    for (size_t i = 0; path[i] != '\0'; i++) {
        if (path[i] == '/' && used > 0 && out[used - 1] == '/') {
            continue;
        }

        if (used + 1 >= cap) {
            return false;
        }

        out[used++] = path[i];
    }

    while (used > 1 && out[used - 1] == '/') {
        used--;
    }

    out[used] = '\0';

    hal_storage_volume_t volume;
    return volume_for(out, &volume);
}

/* A name a caller may give for a file: one component, and not a way out. */
static bool name_clean(const char *name, char *out, size_t cap)
{
    if (name == NULL || name[0] == '\0') {
        return false;
    }

    if (strcmp(name, ".") == 0 || strcmp(name, "..") == 0) {
        return false;
    }

    size_t len = strlen(name);
    if (len >= cap || len >= WEB_NAME_MAX) {
        return false;
    }

    if (strchr(name, '/') != NULL || strchr(name, '\\') != NULL) {
        return false;
    }

    memcpy(out, name, len + 1);

    return true;
}

/* `mkdir -p`, for the directories above a file about to be written. LittleFS has no path creation of
 * its own, so a page that creates "/internal/a/b/c" would otherwise have to ask three times. */
static void make_dirs(const char *path)
{
    char work[WEB_PATH_MAX];
    if (strlen(path) >= sizeof work) {
        return;
    }

    strcpy(work, path);

    for (char *at = work + 1; *at; at++) {
        if (*at != '/') {
            continue;
        }

        *at = '\0';
        if (mkdir(work, 0777) != 0 && errno != EEXIST) {
            /* Not fatal here: the write that follows says whether the path works, and it has a
             * better answer than this one does. */
            ESP_LOGD(TAG, "mkdir %s: %s", work, strerror(errno));
        }

        *at = '/';
    }
}

/* ---------------------------------------------------------------------------
 * Answers
 * ------------------------------------------------------------------------- */

static esp_err_t send_json(httpd_req_t *req, const char *json)
{
    s_last_request_us = esp_timer_get_time();
    httpd_resp_set_type(req, "application/json; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");

    return httpd_resp_send(req, json, HTTPD_RESP_USE_STRLEN);
}

/* The failure form: an HTTP status for anything that is not this page, and a code for the page's own
 * table of sentences. `message` is optional and is what the filesystem said, for the log. */
static esp_err_t send_error(httpd_req_t *req, const char *status, const char *code, const char *message)
{
    if (message != NULL) {
        ESP_LOGW(TAG, "%s: %s", code, message);
    }

    char body[192];
    snprintf(body, sizeof body, "{\"error\":\"%s\"}", code);

    httpd_resp_set_status(req, status);

    return send_json(req, body);
}

static esp_err_t send_busy(httpd_req_t *req)
{
    return send_error(req, "503 Service Unavailable", "too_big",
                      "the answer would not fit in the buffer");
}

static esp_err_t send_blocked(httpd_req_t *req, const char *message)
{
    return send_error(req, "400 Bad Request", "bad_request", message);
}

/* ---------------------------------------------------------------------------
 * The page
 * ------------------------------------------------------------------------- */

static esp_err_t handle_page(httpd_req_t *req)
{
    size_t len = (size_t)(web_page_html_end - web_page_html_start);

    s_last_request_us = esp_timer_get_time();
    httpd_resp_set_type(req, "text/html; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");

    return httpd_resp_send(req, (const char *)web_page_html_start, (ssize_t)len);
}

/* Asked for by every browser and drawn by none of them here. Answered rather than 404'd so that the
 * log holds requests a person made. */
static esp_err_t handle_favicon(httpd_req_t *req)
{
    httpd_resp_set_status(req, "204 No Content");

    return httpd_resp_send(req, NULL, 0);
}

/* ---------------------------------------------------------------------------
 * The box: what it is, and how much of it is left
 * ------------------------------------------------------------------------- */

static const char *reset_reason_name(esp_reset_reason_t reason)
{
    switch (reason) {
    case ESP_RST_POWERON:  return "power_on";
    case ESP_RST_EXT:      return "external";
    case ESP_RST_SW:       return "software";
    case ESP_RST_PANIC:    return "panic";
    case ESP_RST_INT_WDT:  return "interrupt_watchdog";
    case ESP_RST_TASK_WDT: return "task_watchdog";
    case ESP_RST_WDT:      return "watchdog";
    case ESP_RST_DEEPSLEEP:return "deep_sleep";
    case ESP_RST_BROWNOUT: return "brownout";
    case ESP_RST_SDIO:     return "sdio";
    default:               return "unknown";
    }
}

static esp_err_t handle_system(httpd_req_t *req)
{
    web_buf_t buf;
    buf_init(&buf, 1024);

    const esp_app_desc_t *app = esp_app_get_description();
    esp_chip_info_t       chip;
    esp_chip_info(&chip);

    buf_puts(&buf, "{");

    buf_puts(&buf, "\"app\":");
    buf_json(&buf, app != NULL ? app->project_name : "pomelo");
    buf_puts(&buf, ",\"version\":");
    buf_json(&buf, app != NULL ? app->version : "");
    buf_puts(&buf, ",\"idf\":");
    buf_json(&buf, app != NULL ? app->idf_ver : "");
    buf_puts(&buf, ",\"build\":");
    buf_json(&buf, app != NULL ? app->date : "");

    buf_printf(&buf, ",\"cores\":%d,\"chip\":\"esp32s3\"", chip.cores);

    buf_puts(&buf, ",\"running\":");
    {
        char running[64];
        app_name_copy(running, sizeof running);
        buf_json(&buf, running);
    }

    buf_printf(&buf, ",\"uptime\":%lld", esp_timer_get_time() / 1000000);
    buf_printf(&buf, ",\"heap_free\":%u", (unsigned)esp_get_free_heap_size());
    buf_printf(&buf, ",\"heap_min\":%u", (unsigned)esp_get_minimum_free_heap_size());
    buf_printf(&buf, ",\"psram_free\":%u",
               (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    buf_printf(&buf, ",\"reset\":\"%s\"", reset_reason_name(esp_reset_reason()));

    /* The die's own thermometer, in tenths of a degree from the PMIC (see the HAL note). Reported as
     * a decimal here so that a page does not have to know the unit. */
    int32_t temperature_dc = 0;
    if (hal_power_get_chip_temperature_dc(&temperature_dc) == ESP_OK) {
        buf_printf(&buf, ",\"chip_temp_c\":%.1f", (double)temperature_dc / 10.0);
    } else {
        buf_puts(&buf, ",\"chip_temp_c\":null");
    }

    buf_printf(&buf, ",\"display\":{\"on\":%s,\"brightness\":%u,\"capture\":%s}",
               hal_display_is_powered() ? "true" : "false",
               (unsigned)hal_display_get_brightness(),
               hal_display_has_capture() ? "true" : "false");

    buf_printf(&buf, ",\"battery\":{\"percent\":%d,\"charging\":%s,\"voltage_mv\":%d}",
               (int)hal_power_get_battery_percent(),
               hal_power_is_charging() ? "true" : "false",
               (int)hal_power_get_battery_voltage_mv());

    /* The volumes, so that a page draws the numbers the box has rather than asking for each of them
     * separately: they change under a finger, and three calls are three answers at three times. */
    buf_puts(&buf, ",\"volumes\":[");
    hal_storage_volume_t roots[2];
    size_t               count = web_roots(roots, 2);
    for (size_t i = 0; i < count; i++) {
        if (i > 0) {
            buf_puts(&buf, ",");
        }

        buf_puts(&buf, "{\"mount\":");
        buf_json(&buf, roots[i].mount_point);
        buf_puts(&buf, ",\"filesystem\":");
        buf_json(&buf, roots[i].filesystem);
        buf_printf(&buf, ",\"total\":%llu,\"free\":%llu}",
                   (unsigned long long)roots[i].total_bytes,
                   (unsigned long long)roots[i].free_bytes);
    }
    buf_puts(&buf, "]}");

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* ---------------------------------------------------------------------------
 * Files
 * ------------------------------------------------------------------------- */

typedef struct {
    char          name[WEB_NAME_MAX];
    bool          dir;
    unsigned long size;
} web_entry_t;

static int entry_compare(const void *left, const void *right)
{
    const web_entry_t *a = left;
    const web_entry_t *b = right;

    /* Directories first — a listing is walked, and the walk goes down — then by name, which is the
     * order a person reads a list of files in. */
    if (a->dir != b->dir) {
        return a->dir ? -1 : 1;
    }

    return strcasecmp(a->name, b->name);
}

static esp_err_t handle_files(httpd_req_t *req)
{
    char path[WEB_PATH_MAX];
    if (!query_string(req, "path", path, sizeof path)) {
        hal_storage_volume_t internal;
        if (hal_storage_get_internal(&internal) != ESP_OK) {
            return send_error(req, "404 Not Found", "no_storage", "internal is not mounted");
        }

        snprintf(path, sizeof path, "%s", internal.mount_point);
    }

    char clean[WEB_PATH_MAX];
    if (!path_clean(path, clean, sizeof clean)) {
        return send_blocked(req, "path is not under a mounted volume");
    }

    DIR *dir = opendir(clean);
    if (dir == NULL) {
        return send_error(req, "404 Not Found", "no_such_directory", strerror(errno));
    }

    web_entry_t *entries = heap_caps_malloc(sizeof(web_entry_t) * WEB_MAX_ENTRIES,
                                            MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (entries == NULL) {
        entries = malloc(sizeof(web_entry_t) * WEB_MAX_ENTRIES);
    }

    if (entries == NULL) {
        closedir(dir);
        return send_busy(req);
    }

    size_t   count     = 0;
    size_t   clean_len = strlen(clean);
    bool     more      = false;
    struct dirent *item;

    while ((item = readdir(dir)) != NULL) {
        if (strcmp(item->d_name, ".") == 0 || strcmp(item->d_name, "..") == 0) {
            continue;
        }

        if (count == WEB_MAX_ENTRIES) {
            /* Said in the answer rather than passed over: a listing that stopped at a page boundary
             * without saying so is a directory that looks like it lost its files. */
            more = true;
            break;
        }

        /* A listing carries names this API can hand back: one that does not fit in a path or in a name
         * is left out and reported, because an entry the page would have to quote back before it could
         * act on it is an entry it could not act on. */
        size_t name_len = strlen(item->d_name);
        if (name_len == 0 || name_len >= WEB_NAME_MAX || clean_len + 1 + name_len >= WEB_PATH_MAX) {
            more = true;
            continue;
        }

        web_entry_t *entry = &entries[count];
        memcpy(entry->name, item->d_name, name_len + 1);

        char full[WEB_PATH_MAX];
        memcpy(full, clean, clean_len);
        full[clean_len] = '/';
        memcpy(full + clean_len + 1, entry->name, name_len + 1);

        struct stat info;
        if (stat(full, &info) == 0) {
            entry->dir  = S_ISDIR(info.st_mode);
            entry->size = (unsigned long)info.st_size;
        } else {
            entry->dir  = item->d_type == DT_DIR;
            entry->size = 0;
        }

        count++;
    }

    closedir(dir);

    qsort(entries, count, sizeof(web_entry_t), entry_compare);

    web_buf_t buf;
    buf_init(&buf, 4096);

    buf_puts(&buf, "{\"path\":");
    buf_json(&buf, clean);
    buf_printf(&buf, ",\"more\":%s", more ? "true" : "false");

    hal_storage_volume_t volume;
    if (volume_for(clean, &volume)) {
        buf_puts(&buf, ",\"mount\":");
        buf_json(&buf, volume.mount_point);
        buf_puts(&buf, ",\"filesystem\":");
        buf_json(&buf, volume.filesystem);
        buf_printf(&buf, ",\"total\":%llu,\"free\":%llu",
                   (unsigned long long)volume.total_bytes,
                   (unsigned long long)volume.free_bytes);
    }

    buf_puts(&buf, ",\"roots\":[");
    hal_storage_volume_t roots[2];
    size_t               roots_count = web_roots(roots, 2);
    for (size_t i = 0; i < roots_count; i++) {
        if (i > 0) {
            buf_puts(&buf, ",");
        }

        buf_json(&buf, roots[i].mount_point);
    }
    buf_puts(&buf, "]");

    buf_puts(&buf, ",\"entries\":[");
    for (size_t i = 0; i < count; i++) {
        if (i > 0) {
            buf_puts(&buf, ",");
        }

        buf_puts(&buf, "{\"name\":");
        buf_json(&buf, entries[i].name);
        buf_printf(&buf, ",\"dir\":%s,\"size\":%lu}",
                   entries[i].dir ? "true" : "false", entries[i].size);
    }
    buf_puts(&buf, "]}");

    free(entries);

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* What a browser is told a file is. The list is short because it is about what a browser *draws*: a
 * text file or a picture, in the tab, without being downloaded first. Anything else is a download,
 * which is the honest answer for a file this box does not know. */
static const char *mime_for(const char *path)
{
    static const struct { const char *extension; const char *type; } table[] = {
        { "txt",  "text/plain; charset=utf-8" },
        { "log",  "text/plain; charset=utf-8" },
        { "md",   "text/plain; charset=utf-8" },
        { "csv",  "text/csv; charset=utf-8" },
        { "json", "application/json; charset=utf-8" },
        { "html", "text/html; charset=utf-8" },
        { "htm",  "text/html; charset=utf-8" },
        { "css",  "text/css; charset=utf-8" },
        { "js",   "text/javascript; charset=utf-8" },
        { "xml",  "text/xml; charset=utf-8" },
        { "png",  "image/png" },
        { "jpg",  "image/jpeg" },
        { "jpeg", "image/jpeg" },
        { "gif",  "image/gif" },
        { "webp", "image/webp" },
        { "bmp",  "image/bmp" },
        { "svg",  "image/svg+xml" },
        { "mp3",  "audio/mpeg" },
        { "wav",  "audio/wav" },
        { "flac", "audio/flac" },
        { "m4a",  "audio/mp4" },
        { "aac",  "audio/aac" },
        { "ogg",  "audio/ogg" },
        { "mp4",  "video/mp4" },
        { "webm", "video/webm" },
    };

    const char *dot = strrchr(path, '.');
    if (dot == NULL || dot[1] == '\0') {
        return "application/octet-stream";
    }

    for (size_t i = 0; i < sizeof table / sizeof table[0]; i++) {
        if (strcasecmp(dot + 1, table[i].extension) == 0) {
            return table[i].type;
        }
    }

    return "application/octet-stream";
}

/* A byte range out of a `Range:` header, and whether this server should act on one.
 *
 * The three answers are the three things a caller can do: no range (or one this server will not
 * act on, in which case the whole file is the answer, because a header it cannot read is no reason
 * to refuse a request it can serve), a range to honour, or a range that starts past the end of the
 * file — which is the one case that is its own HTTP status.
 *
 * `bytes=first-last`, `bytes=first-` and `bytes=-suffix` are the three forms a browser sends; a
 * multi-range request (`a-b,c-d`) is deliberately not read, because the answer to it is a multipart
 * body and no media element on the page asks for one.
 */
static int range_parse(const char *header, long long total, long long *start, long long *end)
{
    if (header == NULL || strncasecmp(header, "bytes=", 6) != 0) {
        return 0;
    }

    const char *spec = header + 6;
    long long   first;
    long long   last;
    char       *after;

    if (spec[0] == '-') {
        /* `bytes=-N`: the last N bytes, which is how a player asks for a file's tail. */
        long long suffix = strtoll(spec + 1, &after, 10);
        if (after == spec + 1 || suffix <= 0) {
            return 0;
        }

        first = total - suffix;
        if (first < 0) {
            first = 0;
        }
        last = total - 1;
    } else {
        first = strtoll(spec, &after, 10);
        if (after == spec || *after != '-') {
            return 0;
        }

        const char *tail = after + 1;
        if (*tail == '\0' || *tail == ',') {
            last = total - 1; /* `bytes=N-`: from N to the end. */
        } else if (*tail >= '0' && *tail <= '9') {
            last = strtoll(tail, NULL, 10);
        } else {
            return 0;
        }
    }

    if (total <= 0 || first >= total) {
        return -1;
    }

    if (last >= total) {
        last = total - 1;
    }

    if (last < first) {
        return 0;
    }

    *start = first;
    *end   = last;
    return 1;
}

/* A file out of the card, whole or in the part the caller asked for.
 *
 * The part matters because this is also how the page plays music: an `<audio>` element asks with
 * `Range: bytes=...` when it wants to seek, and a server that ignores the header answers with the
 * file from the beginning every time, which is a track that cannot be scrubbed. So the header is
 * read, the status becomes `206 Partial Content`, and the body is the slice it named.
 *
 * The body is still sent chunked — a card-sized file does not fit in the RAM this box has, and
 * esp_httpd both adds `Transfer-Encoding: chunked` and refuses to let a caller set `Content-Length`
 * beside it. `Content-Range` is what carries the range in that arrangement, and it is the header a
 * media element reads.
 */
static esp_err_t handle_download(httpd_req_t *req)
{
    char path[WEB_PATH_MAX];
    if (!query_string(req, "path", path, sizeof path)) {
        return send_blocked(req, "path is required");
    }

    char clean[WEB_PATH_MAX];
    if (!path_clean(path, clean, sizeof clean)) {
        return send_blocked(req, "path is not under a mounted volume");
    }

    struct stat info;
    if (stat(clean, &info) != 0 || S_ISDIR(info.st_mode)) {
        return send_error(req, "404 Not Found", "no_such_file", strerror(errno));
    }

    FILE *file = fopen(clean, "rb");
    if (file == NULL) {
        return send_error(req, "404 Not Found", "no_such_file", strerror(errno));
    }

    const char *type = mime_for(clean);
    httpd_resp_set_type(req, type);

    /* Sent on the whole file too, and it has to be: a player asks for the file first and only then
     * learns there is anything to seek with. */
    httpd_resp_set_hdr(req, "Accept-Ranges", "bytes");

    /* A picture or a text file opens in the tab; anything else is saved. `inline=1` is the page
     * saying "show me this one" — the preview a file manager needs and the browser cannot guess. */
    char inline_flag[8];
    if (query_string(req, "inline", inline_flag, sizeof inline_flag) && strcmp(inline_flag, "1") == 0) {
        httpd_resp_set_hdr(req, "Content-Disposition", "inline");
    } else {
        const char *name = strrchr(clean, '/');
        name = name != NULL ? name + 1 : clean;

        /* Built by hand: the name is a file's, so it may hold the quote and the backslash that would
         * end this header early. They become underscores, which is what a browser shows for a
         * download whose name it cannot use. */
        char   disposition[WEB_NAME_MAX + 32];
        size_t used = (size_t)snprintf(disposition, sizeof disposition, "attachment; filename=\"");

        for (const char *c = name; *c != '\0' && used + 2 < sizeof disposition; c++) {
            bool plain = (unsigned char)*c >= 0x20 && *c != '"' && *c != '\\' && *c != 0x7f;
            disposition[used++] = plain ? *c : '_';
        }

        disposition[used++] = '"';
        disposition[used]   = '\0';

        httpd_resp_set_hdr(req, "Content-Disposition", disposition);
    }

    /* Read before the body starts: esp_httpd purges the request headers on the first send, so this
     * is the only moment the `Range` header can be looked at. */
    long long total       = (long long)info.st_size;
    long long start       = 0;
    long long wanted      = total;

    char range_header[64];
    if (total > 0 && httpd_req_get_hdr_value_str(req, "Range", range_header, sizeof range_header) == ESP_OK) {
        long long first = 0;
        long long last  = 0;
        int       parsed = range_parse(range_header, total, &first, &last);

        if (parsed < 0) {
            fclose(file);

            char unsatisfiable[48];
            snprintf(unsatisfiable, sizeof unsatisfiable, "bytes */%lld", total);
            httpd_resp_set_hdr(req, "Content-Range", unsatisfiable);

            return send_error(req, "416 Range Not Satisfiable", "bad_range", range_header);
        }

        if (parsed > 0) {
            start  = first;
            wanted = last - first + 1;

            char content_range[64];
            snprintf(content_range, sizeof content_range, "bytes %lld-%lld/%lld", first, last, total);
            httpd_resp_set_hdr(req, "Content-Range", content_range);
            httpd_resp_set_status(req, "206 Partial Content");

            if (fseek(file, (long)start, SEEK_SET) != 0) {
                fclose(file);
                return send_error(req, "500 Internal Server Error", "seek_failed", strerror(errno));
            }
        }
    }

    char   chunk[WEB_CHUNK];
    size_t remaining = (size_t)wanted;

    while (remaining > 0) {
        size_t want = remaining < sizeof chunk ? remaining : sizeof chunk;
        size_t read = fread(chunk, 1, want, file);
        if (read == 0) {
            /* The file is shorter than its own `stat` said — a card pulled mid-read, a write that
             * never finished. Stop rather than pad the answer with nothing. */
            break;
        }

        if (httpd_resp_send_chunk(req, chunk, (ssize_t)read) != ESP_OK) {
            /* The client went away — a tap on another tab, a phone that locked. Nothing is wrong, and
             * the file is closed either way. */
            ESP_LOGD(TAG, "download interrupted: %s", clean);
            fclose(file);
            return ESP_OK;
        }

        remaining -= read;
    }

    fclose(file);

    /* The empty chunk that ends the body. Not sent when the loop failed, which is why the failure
     * above returns straight out of this function. */
    return httpd_resp_send_chunk(req, NULL, 0);
}

/* An upload is the file's bytes as the whole body, with the destination in the query string (see the
 * note at the top of this file). What is left to decide is what a name that is already taken means,
 * and that is a question for whoever is holding the file: the person at the browser is the one who can
 * say whether what they just dropped in is a new version of what is there or a mistake. So overwriting
 * is asked for with `overwrite=1`, and a name that is taken without it is refused — before a byte is
 * written, which is what makes the refusal cost nothing.
 *
 * The bytes land next to the target and only the last step makes them the file: `<name>.part` while
 * the body is arriving, and a rename once all of it has. Two things follow, and they are the point:
 *
 *   - an upload that is cut short — a phone that walked out of range, a card that filled up — leaves
 *     the file that was there exactly as it was, where writing straight over it would have destroyed
 *     a file that worked to produce one that does not;
 *   - a `.part` file is left behind when that happens. It shows in the listing and a person can delete
 *     it, which is the leftover that says what happened, against the half-written file that does not.
 *
 * It costs one thing worth naming: overwriting needs room for both copies at once, so a card with just
 * enough room for the new file refuses where writing in place would have managed.
 *
 * The `.part` is also why the answer names the file rather than the directory: what a caller wants to
 * hear back is where the bytes ended up. */
static esp_err_t handle_upload(httpd_req_t *req)
{
    char directory[WEB_PATH_MAX];
    char name[WEB_NAME_MAX];
    char raw_name[WEB_QUERY_MAX];
    char flag[8];

    if (!query_string(req, "path", directory, sizeof directory)) {
        return send_blocked(req, "path is required");
    }

    if (!query_string(req, "name", raw_name, sizeof raw_name) || !name_clean(raw_name, name, sizeof name)) {
        return send_blocked(req, "name is required and must be one file name");
    }

    bool overwrite = query_string(req, "overwrite", flag, sizeof flag) && strcmp(flag, "1") == 0;

    /* How much of this file the box already has. A page that lost the network halfway through a large
     * upload comes back with `offset=<bytes already stored>` and sends only the rest, which on a card
     * and a phone is the difference between a retry and a restart. No `offset`, or a zero, is a fresh
     * upload and the partial file — if one is there from last time — is started over. */
    long long offset = 0;
    char      offset_text[20];
    if (query_string(req, "offset", offset_text, sizeof offset_text)) {
        offset = strtoll(offset_text, NULL, 10);
    }
    if (offset < 0) {
        offset = 0;
    }

    char clean[WEB_PATH_MAX];
    if (!path_clean(directory, clean, sizeof clean)) {
        return send_blocked(req, "path is not under a mounted volume");
    }

    make_dirs(clean);

    char full[WEB_PATH_MAX];
    if (snprintf(full, sizeof full, "%s/%s", clean, name) >= (int)sizeof full) {
        return send_blocked(req, "the path is too long");
    }

    /* The target is bounded by `WEB_PATH_MAX`, and this is that plus what `.part` adds to it. */
    char part[WEB_PATH_MAX + 8];
    if (snprintf(part, sizeof part, "%s.part", full) >= (int)sizeof part) {
        return send_blocked(req, "the path is too long");
    }

    struct stat existing;
    if (stat(full, &existing) == 0) {
        if (S_ISDIR(existing.st_mode)) {
            return send_error(req, "409 Conflict", "already_exists", "a directory has that name");
        }

        if (!overwrite) {
            return send_error(req, "409 Conflict", "already_exists",
                              "the name is taken and overwrite was not asked for");
        }
    }

    FILE *file = NULL;

    if (offset > 0) {
        /* A resume is only a resume if the partial file is still there and is exactly as long as the
         * caller believes. Appending to a partial of a different length is how a file comes out with
         * a seam in it that nothing downstream can see. */
        struct stat partial;
        if (stat(part, &partial) != 0) {
            return send_error(req, "409 Conflict", "cannot_resume", "the partial upload is gone");
        }
        if ((long long)partial.st_size != offset) {
            return send_error(req, "409 Conflict", "cannot_resume",
                              "the partial upload is not the length the caller has");
        }

        file = fopen(part, "ab");
    } else {
        file = fopen(part, "wb");
    }

    if (file == NULL) {
        return send_error(req, "500 Internal Server Error", "write_failed", strerror(errno));
    }

    char   chunk[WEB_CHUNK];
    size_t received = 0;
    int    timeouts = 0;

    while (req->content_len > (int)received) {
        int read = httpd_req_recv(req, chunk, sizeof chunk);
        if (read == HTTPD_SOCK_ERR_TIMEOUT) {
            /* A slow upload is not a failed one: a phone that is thinking about a large file is
             * still sending it. Three timeouts in a row is a client that left. */
            if (++timeouts > 3) {
                break;
            }

            continue;
        }

        if (read <= 0) {
            break;
        }

        timeouts = 0;

        if (fwrite(chunk, 1, (size_t)read, file) != (size_t)read) {
            fclose(file);
            remove(part);
            return send_error(req, "500 Internal Server Error", "write_failed", strerror(errno));
        }

        received += (size_t)read;
    }

    fclose(file);

    if ((int)received != req->content_len) {
        /* The bytes that did arrive stay in `<name>.part` rather than being thrown away: they are the
         * resume point, and the answer says how far they got. A `.part` is never mistaken for the
         * file — the name it would have had is a different name — so the only thing keeping it costs
         * is the room it takes, which is exactly the room an upload already needed. */
        char detail[96];
        snprintf(detail, sizeof detail, "cut short at %lld bytes; resume with offset",
                 offset + (long long)received);

        return send_error(req, "408 Request Timeout", "upload_incomplete", detail);
    }

    /* All of it is on the flash, so what is left is to put it where it was asked for. The old copy
     * goes first: neither FATFS nor LittleFS renames onto a name that is taken. This is the one moment
     * the name is neither file, which is what the `.part` bought. */
    if (remove(full) != 0 && errno != ENOENT) {
        ESP_LOGD(TAG, "remove %s: %s", full, strerror(errno));
    }

    if (rename(part, full) != 0) {
        remove(part);
        return send_error(req, "500 Internal Server Error", "write_failed", strerror(errno));
    }

    ESP_LOGI(TAG, "stored %u bytes in %s", (unsigned)received, full);

    web_buf_t buf;
    buf_init(&buf, 256);

    buf_puts(&buf, "{\"ok\":true,\"path\":");
    buf_json(&buf, full);
    buf_printf(&buf, ",\"bytes\":%lld,\"resumed_from\":%lld}",
               offset + (long long)received, offset);

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

static esp_err_t handle_delete(httpd_req_t *req)
{
    char path[WEB_PATH_MAX];
    if (!query_string(req, "path", path, sizeof path)) {
        return send_blocked(req, "path is required");
    }

    char clean[WEB_PATH_MAX];
    if (!path_clean(path, clean, sizeof clean)) {
        return send_blocked(req, "path is not under a mounted volume");
    }

    /* The volume root itself is not a file to delete: emptying the box is not a thing this page does,
     * and it would leave a board with nothing to serve the deletion from. */
    hal_storage_volume_t volume;
    if (volume_for(clean, &volume) && strcmp(clean, volume.mount_point) == 0) {
        return send_blocked(req, "a mount point cannot be removed");
    }

    if (remove(clean) != 0) {
        if (errno == ENOTEMPTY || errno == EEXIST) {
            return send_error(req, "409 Conflict", "directory_not_empty", strerror(errno));
        }

        return send_error(req, "500 Internal Server Error", "remove_failed", strerror(errno));
    }

    return send_json(req, "{\"ok\":true}");
}

static esp_err_t handle_mkdir(httpd_req_t *req)
{
    char path[WEB_PATH_MAX];
    if (!query_string(req, "path", path, sizeof path)) {
        return send_blocked(req, "path is required");
    }

    char clean[WEB_PATH_MAX];
    if (!path_clean(path, clean, sizeof clean)) {
        return send_blocked(req, "path is not under a mounted volume");
    }

    if (mkdir(clean, 0777) != 0) {
        if (errno == EEXIST) {
            return send_error(req, "409 Conflict", "already_exists", strerror(errno));
        }

        return send_error(req, "500 Internal Server Error", "mkdir_failed", strerror(errno));
    }

    return send_json(req, "{\"ok\":true}");
}

static esp_err_t handle_rename(httpd_req_t *req)
{
    char path[WEB_PATH_MAX];
    char raw_name[WEB_QUERY_MAX];
    char name[WEB_NAME_MAX];

    if (!query_string(req, "path", path, sizeof path)) {
        return send_blocked(req, "path is required");
    }

    if (!query_string(req, "name", raw_name, sizeof raw_name) || !name_clean(raw_name, name, sizeof name)) {
        return send_blocked(req, "name is required and must be one file name");
    }

    char clean[WEB_PATH_MAX];
    if (!path_clean(path, clean, sizeof clean)) {
        return send_blocked(req, "path is not under a mounted volume");
    }

    /* A rename stays in its own directory: a page renames a file, and moving one between volumes is a
     * copy and a delete with a progress bar, which is a different feature. */
    char directory[WEB_PATH_MAX];
    snprintf(directory, sizeof directory, "%s", clean);

    char *slash = strrchr(directory, '/');
    if (slash == NULL) {
        return send_blocked(req, "the path has no directory");
    }

    *slash = '\0';

    char target[WEB_PATH_MAX];
    if (snprintf(target, sizeof target, "%s/%s", directory[0] ? directory : "/", name) >= (int)sizeof target) {
        return send_blocked(req, "the path is too long");
    }

    if (rename(clean, target) != 0) {
        return send_error(req, "500 Internal Server Error", "rename_failed", strerror(errno));
    }

    return send_json(req, "{\"ok\":true}");
}

/* ---------------------------------------------------------------------------
 * Wi-Fi
 *
 * The radio is driven through the same calls the panel's pages use, so the two cannot disagree about
 * it: the state, the scan cache and the connection are `board_wifi.c`'s and this file only reads and
 * asks.
 *
 * The credentials *file* is a different thing. The panel writes it from the settings page, once the
 * connection it asked for has come up, and for a good reason: a write at the attempt would let one
 * mistyped password destroy the network that worked. The page here does the same — `connect` only
 * connects, and the browser calls `remember` after the status says it is up. Two front ends, one
 * order, and the file's owner is still the app.
 * ------------------------------------------------------------------------- */

#define WEB_WIFI_CONF "/internal/AppData/WIFI/wifi.conf"

static const char *wifi_state_name(int32_t state)
{
    switch (state) {
    case HAL_WIFI_STATE_SCANNING:   return "scanning";
    case HAL_WIFI_STATE_CONNECTING: return "connecting";
    case HAL_WIFI_STATE_CONNECTED:  return "connected";
    default:                        return "disconnected";
    }
}

static const char *scan_state_name(int32_t state)
{
    switch (state) {
    case HAL_WIFI_SCAN_SCANNING: return "scanning";
    case HAL_WIFI_SCAN_DONE:     return "done";
    case HAL_WIFI_SCAN_ERROR:    return "error";
    default:                     return "idle";
    }
}

/* One TOML basic string, appended to `out`: the `[wifi]` file is read by `serde`, and a password with
 * a quote or a backslash in it has to come back as itself — the panel's own writer escapes exactly
 * these, and a second writer that escaped less would be a second format. */
static bool toml_quote(char *out, size_t cap, size_t *used, const char *value)
{
    if (*used + 2 > cap) {
        return false;
    }

    out[(*used)++] = '"';

    for (const unsigned char *c = (const unsigned char *)value; *c; c++) {
        char        scratch[8];
        const char *escape = NULL;

        switch (*c) {
        case '"':  escape = "\\\""; break;
        case '\\': escape = "\\\\"; break;
        case '\n': escape = "\\n";  break;
        case '\r': escape = "\\r";  break;
        case '\t': escape = "\\t";  break;
        default:
            if (*c < 0x20) {
                snprintf(scratch, sizeof scratch, "\\u%04X", *c);
                escape = scratch;
            }
        }

        size_t length = escape != NULL ? strlen(escape) : 1;

        if (*used + length + 2 > cap) {
            return false;
        }

        if (escape != NULL) {
            memcpy(out + *used, escape, length);
        } else {
            out[(*used)] = (char)*c;
        }

        *used += length;
    }

    out[(*used)++] = '"';
    out[*used] = '\0';

    return true;
}

/* The network the file names, for the page to show against the one that is connected. Read here and
 * not cached: this file is written by a browser a minute ago and by a panel a week ago, and the only
 * copy that is right is the one on the flash. */
static bool wifi_conf_ssid(char *out, size_t cap)
{
    char   text[4096];
    size_t len = 0;

    if (hal_storage_read_file(WEB_WIFI_CONF, text, sizeof text, &len) != ESP_OK) {
        return false;
    }

    for (const char *at = text; (at = strstr(at, "ssid")) != NULL; at += 4) {
        const char *cursor = at + 4;

        while (*cursor == ' ' || *cursor == '\t') {
            cursor++;
        }

        if (*cursor != '=') {
            continue;
        }

        cursor++;
        while (*cursor == ' ' || *cursor == '\t') {
            cursor++;
        }

        if (*cursor != '"') {
            continue;
        }

        cursor++;

        size_t used = 0;
        while (*cursor != '\0' && *cursor != '"' && used + 1 < cap) {
            if (*cursor == '\\' && (cursor[1] == '"' || cursor[1] == '\\')) {
                cursor++;
            }

            out[used++] = *cursor++;
        }

        out[used] = '\0';

        return true;
    }

    return false;
}

static esp_err_t handle_wifi(httpd_req_t *req)
{
    hal_wifi_status_t status;
    if (hal_wifi_get_status(&status) != ESP_OK) {
        return send_error(req, "500 Internal Server Error", "wifi_unavailable", "no status");
    }

    char saved[128];
    bool has_saved = wifi_conf_ssid(saved, sizeof saved);

    web_buf_t buf;
    buf_init(&buf, 512);

    buf_printf(&buf, "{\"enabled\":%s,\"state\":\"%s\",\"ssid\":",
               hal_wifi_is_enabled() ? "true" : "false", wifi_state_name(status.state));
    buf_json(&buf, status.ssid);
    buf_puts(&buf, ",\"ip\":");
    buf_json(&buf, status.ip);
    buf_puts(&buf, ",\"netmask\":");
    buf_json(&buf, status.netmask);
    buf_puts(&buf, ",\"gateway\":");
    buf_json(&buf, status.gateway);
    buf_printf(&buf, ",\"rssi\":%d,\"scan\":\"%s\",\"saved\":",
               (int)status.rssi, scan_state_name(hal_wifi_scan_get_state()));
    buf_json(&buf, has_saved ? saved : "");
    buf_puts(&buf, "}");

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* A scan needs the radio, and the radio may be off: asking the page to find the switch first would be
 * a page that does nothing with no way to say why. Bringing it up is what the panel's Wi-Fi page does
 * with the same call. */
static esp_err_t handle_wifi_scan_start(httpd_req_t *req)
{
    if (!hal_wifi_is_enabled()) {
        esp_err_t ret = hal_wifi_set_enabled(true);
        if (ret != ESP_OK) {
            return send_error(req, "500 Internal Server Error", "radio_failed", esp_err_to_name(ret));
        }
    }

    esp_err_t ret = hal_wifi_scan_start();
    if (ret != ESP_OK) {
        return send_error(req, "503 Service Unavailable", "scan_failed", esp_err_to_name(ret));
    }

    return send_json(req, "{\"ok\":true}");
}

static esp_err_t handle_wifi_scan_results(httpd_req_t *req)
{
    int32_t   state    = hal_wifi_scan_get_state();
    web_buf_t buf;

    buf_init(&buf, 2048);
    buf_printf(&buf, "{\"state\":\"%s\"", scan_state_name(state));

    if (state == HAL_WIFI_SCAN_DONE) {
        hal_wifi_ap_t records[WEB_MAX_APS];
        uint16_t      count = 0;

        esp_err_t ret = hal_wifi_scan_get_results(records, WEB_MAX_APS, &count);
        if (ret != ESP_OK) {
            buf_free(&buf);
            return send_error(req, "500 Internal Server Error", "scan_failed", esp_err_to_name(ret));
        }

        buf_puts(&buf, ",\"networks\":[");
        for (uint16_t i = 0; i < count; i++) {
            if (i > 0) {
                buf_puts(&buf, ",");
            }

            buf_puts(&buf, "{\"ssid\":");
            buf_json(&buf, records[i].ssid);
            buf_printf(&buf, ",\"rssi\":%d,\"channel\":%u,\"secure\":%s}",
                       (int)records[i].rssi, (unsigned)records[i].channel,
                       records[i].secure ? "true" : "false");
        }
        buf_puts(&buf, "]");
    }

    buf_puts(&buf, "}");

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

static esp_err_t handle_wifi_connect(httpd_req_t *req)
{
    char ssid[WEB_QUERY_MAX];
    char password[WEB_QUERY_MAX];

    if (!query_string(req, "ssid", ssid, sizeof ssid) || ssid[0] == '\0') {
        return send_blocked(req, "ssid is required");
    }

    if (!query_string(req, "password", password, sizeof password)) {
        password[0] = '\0';
    }

    /* The password is on its way to the radio either way. Keeping it out of this log line is the one
     * thing this file can do about a network that is not encrypted to begin with. */
    ESP_LOGI(TAG, "connecting to %s", ssid);

    if (!hal_wifi_is_enabled()) {
        esp_err_t ret = hal_wifi_set_enabled(true);
        if (ret != ESP_OK) {
            return send_error(req, "500 Internal Server Error", "radio_failed", esp_err_to_name(ret));
        }
    }

    esp_err_t ret = hal_wifi_connect(ssid, password);
    if (ret != ESP_OK) {
        return send_error(req, "500 Internal Server Error", "connect_failed", esp_err_to_name(ret));
    }

    return send_json(req, "{\"ok\":true}");
}

/* Written only after the status says the connection is up — the browser calls this, see the note above
 * the section. `autoconnect` is what this page is promising: a network chosen here is a network the
 * box joins by itself next time. */
static esp_err_t handle_wifi_remember(httpd_req_t *req)
{
    char ssid[WEB_QUERY_MAX];
    char password[WEB_QUERY_MAX];

    if (!query_string(req, "ssid", ssid, sizeof ssid) || ssid[0] == '\0') {
        return send_blocked(req, "ssid is required");
    }

    if (!query_string(req, "password", password, sizeof password)) {
        password[0] = '\0';
    }

    char text[1024];
    size_t used = 0;

    used += (size_t)snprintf(text + used, sizeof text - used, "[wifi]\nenabled = true\nssid = ");
    if (!toml_quote(text, sizeof text, &used, ssid)) {
        return send_blocked(req, "the network name is too long");
    }

    used += (size_t)snprintf(text + used, sizeof text - used, "\npassword = ");
    if (!toml_quote(text, sizeof text, &used, password)) {
        return send_blocked(req, "the password is too long");
    }

    used += (size_t)snprintf(text + used, sizeof text - used, "\nautoconnect = true\n");

    esp_err_t ret = hal_storage_write_file(WEB_WIFI_CONF, text, used);
    if (ret != ESP_OK) {
        return send_error(req, "500 Internal Server Error", "save_failed", esp_err_to_name(ret));
    }

    ESP_LOGI(TAG, "remembered %s for the next boot", ssid);

    return send_json(req, "{\"ok\":true}");
}

static esp_err_t handle_wifi_disconnect(httpd_req_t *req)
{
    esp_err_t ret = hal_wifi_disconnect();
    if (ret != ESP_OK) {
        return send_error(req, "500 Internal Server Error", "disconnect_failed", esp_err_to_name(ret));
    }

    return send_json(req, "{\"ok\":true}");
}

static esp_err_t handle_wifi_forget(httpd_req_t *req)
{
    /* The file and nothing else: the connection this box has right now is a thing a person is using,
     * and dropping it because they changed their mind about the next boot would be this page taking a
     * decision that is not its. */
    esp_err_t ret = hal_storage_remove_file(WEB_WIFI_CONF);
    if (ret != ESP_OK) {
        return send_error(req, "500 Internal Server Error", "remove_failed", esp_err_to_name(ret));
    }

    return send_json(req, "{\"ok\":true}");
}

/* ---------------------------------------------------------------------------
 * Restart
 * ------------------------------------------------------------------------- */

static void restart_timer(void *argument)
{
    (void)argument;

    hal_power_restart();
}

/* A reset has to happen *after* the answer has left the socket, or the browser sees a connection
 * dropped mid-body and reports a failure for something that worked. So it is a timer, and the answer
 * is sent while the box is still up. */
static esp_err_t handle_restart(httpd_req_t *req)
{
    esp_timer_handle_t           timer = NULL;
    const esp_timer_create_args_t args  = {
        .callback = restart_timer,
        .name     = "web_restart",
    };

    if (esp_timer_create(&args, &timer) == ESP_OK) {
        esp_timer_start_once(timer, 800 * 1000);
    } else {
        ESP_LOGW(TAG, "the restart timer could not be created");
    }

    return send_json(req, "{\"ok\":true}");
}

/* ---------------------------------------------------------------------------
 * The server itself
 * ------------------------------------------------------------------------- */

/* Defined below the table they are named in, and earlier than it would read well for them to be:
 * the section after this one is the routes that need the most explaining, and that explanation is
 * worth more next to the code than next to the list of names. */
static esp_err_t handle_logs(httpd_req_t *req);
static esp_err_t handle_events(httpd_req_t *req);
static esp_err_t handle_diag(httpd_req_t *req);
static esp_err_t handle_write(httpd_req_t *req);
static esp_err_t handle_move(httpd_req_t *req);
static esp_err_t handle_archive(httpd_req_t *req);
static esp_err_t handle_analysis(httpd_req_t *req);
static esp_err_t handle_display(httpd_req_t *req);
static esp_err_t handle_screenshot(httpd_req_t *req);
static esp_err_t handle_remote(httpd_req_t *req);
static esp_err_t handle_audio(httpd_req_t *req);
static esp_err_t handle_rtc(httpd_req_t *req);
static esp_err_t handle_rtc_set(httpd_req_t *req);
static esp_err_t handle_manifest(httpd_req_t *req);
static esp_err_t handle_service_worker(httpd_req_t *req);
static esp_err_t handle_icon(httpd_req_t *req);

typedef struct {
    const char   *uri;
    httpd_method_t method;
    esp_err_t (*handler)(httpd_req_t *);
} web_route_t;

/* Every route this server answers, and the whole of its surface. Two entries sharing a URI with
 * different methods are the two halves of a conversation — `POST /api/wifi/scan` starts one and
 * `GET /api/wifi/scan` reports on it — which `httpd_register_uri_handler` is built to express. */
static const web_route_t s_routes[] = {
    { "/",                    HTTP_GET,  handle_page },
    { "/favicon.ico",         HTTP_GET,  handle_favicon },
    { "/api/system",          HTTP_GET,  handle_system },
    { "/api/files",           HTTP_GET,  handle_files },
    { "/api/download",        HTTP_GET,  handle_download },
    { "/api/upload",          HTTP_POST, handle_upload },
    { "/api/delete",          HTTP_POST, handle_delete },
    { "/api/mkdir",           HTTP_POST, handle_mkdir },
    { "/api/rename",          HTTP_POST, handle_rename },
    { "/api/wifi",            HTTP_GET,  handle_wifi },
    { "/api/wifi/scan",       HTTP_POST, handle_wifi_scan_start },
    { "/api/wifi/scan",       HTTP_GET,  handle_wifi_scan_results },
    { "/api/wifi/connect",    HTTP_POST, handle_wifi_connect },
    { "/api/wifi/remember",   HTTP_POST, handle_wifi_remember },
    { "/api/wifi/disconnect", HTTP_POST, handle_wifi_disconnect },
    { "/api/wifi/forget",     HTTP_POST, handle_wifi_forget },
    { "/api/restart",         HTTP_POST, handle_restart },
    { "/api/logs",            HTTP_GET,  handle_logs },
    { "/api/events",          HTTP_GET,  handle_events },
    { "/api/diag",            HTTP_GET,  handle_diag },
    { "/api/write",           HTTP_POST, handle_write },
    { "/api/move",            HTTP_POST, handle_move },
    { "/api/archive",         HTTP_POST, handle_archive },
    { "/api/analysis",        HTTP_GET,  handle_analysis },
    { "/api/display",         HTTP_GET | HTTP_POST, handle_display },
    { "/api/screenshot",      HTTP_GET,  handle_screenshot },
    { "/api/remote",          HTTP_POST, handle_remote },
    { "/api/audio",           HTTP_POST, handle_audio },
    { "/api/rtc",             HTTP_GET,  handle_rtc },
    { "/api/rtc",             HTTP_POST, handle_rtc_set },
    { "/manifest.webmanifest", HTTP_GET, handle_manifest },
    { "/sw.js",               HTTP_GET,  handle_service_worker },
    { "/icon.svg",            HTTP_GET,  handle_icon },
};

esp_err_t hal_web_start(uint16_t port)
{
    if (port == 0) {
        return ESP_ERR_INVALID_ARG;
    }

    if (hal_web_is_running()) {
        if (hal_web_get_port() == port) {
            return ESP_OK;
        }

        hal_web_stop();
    }

    httpd_config_t config = HTTPD_DEFAULT_CONFIG();

    config.server_port        = port;
    config.max_uri_handlers   = sizeof s_routes / sizeof s_routes[0] + 3;
    /* Four sockets is a phone that opened a tab and forgot about it, plus the one that is being used.
     * More would be more of the heap for clients that are not there. */
    config.max_open_sockets   = 4;
    config.lru_purge_enable   = true;
    config.stack_size         = 8192;
    config.recv_wait_timeout  = 10;
    config.send_wait_timeout  = 10;

    httpd_handle_t server = NULL;

    esp_err_t ret = httpd_start(&server, &config);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "httpd_start: %s", esp_err_to_name(ret));
        return ret;
    }

    for (size_t i = 0; i < sizeof s_routes / sizeof s_routes[0]; i++) {
        httpd_uri_t route = {
            .uri      = s_routes[i].uri,
            .method   = s_routes[i].method,
            .handler  = s_routes[i].handler,
            .user_ctx = NULL,
        };

        ret = httpd_register_uri_handler(server, &route);
        if (ret != ESP_OK) {
            /* A half-registered server is worse than none: it is up, and the page it serves is
             * missing whichever route failed. */
            ESP_LOGE(TAG, "route %s: %s", s_routes[i].uri, esp_err_to_name(ret));
            httpd_stop(server);
            return ret;
        }
    }

    portENTER_CRITICAL(&s_lock);
    s_server = server;
    s_port   = port;
    portEXIT_CRITICAL(&s_lock);

    ESP_LOGI(TAG, "the management page is up on port %u", (unsigned)port);

    return ESP_OK;
}

esp_err_t hal_web_stop(void)
{
    portENTER_CRITICAL(&s_lock);
    httpd_handle_t server = s_server;
    s_server             = NULL;
    portEXIT_CRITICAL(&s_lock);

    if (server == NULL) {
        return ESP_OK;
    }

    /* Stopping the handle closes every socket it owns, so a browser holding the page open loses it —
     * which is what a page that was just switched off should do. */
    esp_err_t ret = httpd_stop(server);
    if (ret != ESP_OK) {
        ESP_LOGW(TAG, "httpd_stop: %s", esp_err_to_name(ret));
        return ret;
    }

    ESP_LOGI(TAG, "the management page is down");

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * The log ring, live
 *
 * The console has the lines and nobody is sitting next to it. These two routes are the sofa's end of
 * it: one hands back what has been written since a number, and one keeps a connection open and
 * pushes the same lines as they arrive.
 * ------------------------------------------------------------------------- */

/* How many lines one answer carries. The ring holds HAL_LOG_LINES; a page that is up to date asks
 * for a handful and gets a handful. */
#define WEB_LOG_LINES 100

static esp_err_t handle_logs(httpd_req_t *req)
{
    char     raw[16];
    uint32_t since = 0;
    size_t   limit = 40;

    if (query_string(req, "since", raw, sizeof raw)) {
        since = (uint32_t)strtoul(raw, NULL, 10);
    }
    if (query_string(req, "limit", raw, sizeof raw)) {
        long asked = strtol(raw, NULL, 10);
        if (asked > 0) {
            limit = (size_t)asked;
        }
    }
    if (limit > WEB_LOG_LINES) {
        limit = WEB_LOG_LINES;
    }

    hal_log_entry_t *lines = heap_caps_malloc(sizeof(hal_log_entry_t) * HAL_LOG_LINES,
                                              MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (lines == NULL) {
        lines = malloc(sizeof(hal_log_entry_t) * HAL_LOG_LINES);
    }
    if (lines == NULL) {
        return send_busy(req);
    }

    size_t count = hal_log_read(lines, HAL_LOG_LINES, &since);

    /* The newest `limit`, because a page that opens a log view wants the end of it. `hal_log_read`
     * hands lines over oldest-first, which is the order they read in. */
    size_t first = count > limit ? count - limit : 0;

    web_buf_t buf;
    buf_init(&buf, 4096);

    buf_printf(&buf, "{\"since\":%u,\"count\":%u,\"lines\":[", (unsigned)since,
               (unsigned)(count - first));

    for (size_t i = first; i < count; i++) {
        int seconds = (int)(lines[i].uptime_us / 1000000);

        if (i > first) {
            buf_puts(&buf, ",");
        }

        buf_printf(&buf, "{\"seq\":%u,\"at\":\"%02d:%02d:%02d\",\"text\":", (unsigned)lines[i].seq,
                   seconds / 3600, (seconds / 60) % 60, seconds % 60);
        buf_json(&buf, lines[i].text);
        buf_puts(&buf, "}");
    }

    buf_puts(&buf, "]}");

    free(lines);

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* One `event:` frame, framed the way the SSE spec wants it and flushed by httpd's chunked send. The
 * buffer is sized for what is ever in it — a status object and one log line, escaped — and not for
 * the largest answer this component can build, because it lives on the handler's stack. */
#define WEB_EVENT_FRAME_MAX 1024

static bool event_send(httpd_req_t *req, const char *name, const char *json)
{
    char frame[WEB_EVENT_FRAME_MAX];

    int written = snprintf(frame, sizeof frame, "event: %s\ndata: %s\n\n", name, json);
    if (written <= 0 || (size_t)written >= sizeof frame) {
        return false;
    }

    return httpd_resp_send_chunk(req, frame, written) == ESP_OK;
}

/* The live status, small: what changes while a page is open and nothing else. The whole picture is
 * `/api/system`, which is asked for when the page is drawn. */
static void live_status_json(web_buf_t *buf)
{
    hal_wifi_status_t wifi;
    bool              have_wifi = hal_wifi_get_status(&wifi) == ESP_OK;
    int32_t           temperature_dc = 0;

    buf_puts(buf, "{");

    buf_printf(buf, "\"uptime\":%lld", esp_timer_get_time() / 1000000);
    buf_printf(buf, ",\"heap_free\":%u", (unsigned)esp_get_free_heap_size());
    buf_printf(buf, ",\"battery\":%d", (int)hal_power_get_battery_percent());

    if (hal_power_get_chip_temperature_dc(&temperature_dc) == ESP_OK) {
        buf_printf(buf, ",\"chip_temp_c\":%.1f", (double)temperature_dc / 10.0);
    } else {
        buf_puts(buf, ",\"chip_temp_c\":null");
    }

    buf_printf(buf, ",\"wifi\":{\"state\":\"%s\",\"rssi\":%d}",
               have_wifi ? wifi_state_name(wifi.state) : "disconnected",
               have_wifi ? (int)wifi.rssi : 0);

    buf_puts(buf, "}");
}

/* A connection held open and fed. Bounded on purpose: a stream that runs as long as the tab is open
 * is a socket the box never gets back, and there are four of them. Thirty seconds is long enough
 * that a page looks live and short enough that a tab left open overnight costs one reconnect a
 * minute. The page remembers the last `seq` it saw and asks for the rest, so the gap is not a hole.
 */
#define WEB_EVENT_SECONDS 30

static esp_err_t handle_events(httpd_req_t *req)
{
    char     raw[16];
    uint32_t since = 0;

    if (query_string(req, "since", raw, sizeof raw)) {
        since = (uint32_t)strtoul(raw, NULL, 10);
    }

    httpd_resp_set_type(req, "text/event-stream; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    /* Asked of any proxy in between: this is one answer that arrives in pieces. */
    httpd_resp_set_hdr(req, "X-Accel-Buffering", "no");

    /* A comment first, so the browser fires `onopen` and a buffering proxy lets go of what it is
     * holding before the first real event. */
    if (httpd_resp_send_chunk(req, ": pomelo\n\n", 10) != ESP_OK) {
        return ESP_OK;
    }

    hal_log_entry_t *lines = heap_caps_malloc(sizeof(hal_log_entry_t) * HAL_LOG_LINES,
                                              MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (lines == NULL) {
        lines = malloc(sizeof(hal_log_entry_t) * HAL_LOG_LINES);
    }

    const int64_t deadline = esp_timer_get_time() + (int64_t)WEB_EVENT_SECONDS * 1000000;
    bool          ok = true;

    while (ok && esp_timer_get_time() < deadline) {
        web_buf_t buf;
        buf_init(&buf, 1024);

        if (lines != NULL) {
            size_t count = hal_log_read(lines, HAL_LOG_LINES, &since);

            for (size_t i = 0; ok && i < count; i++) {
                buf_reset(&buf);
                buf_printf(&buf, "{\"seq\":%u,\"at\":\"%lld\",\"text\":", (unsigned)lines[i].seq,
                           lines[i].uptime_us / 1000000);
                buf_json(&buf, lines[i].text);
                buf_puts(&buf, "}");

                ok = !buf.full && event_send(req, "log", buf.data);
            }
        }

        buf_reset(&buf);
        live_status_json(&buf);
        buf_printf(&buf, ",\"since\":%u}", (unsigned)since);

        ok = ok && !buf.full && event_send(req, "status", buf.data);
        buf_free(&buf);

        /* The only thing keeping this task from starving the idle task, and the reason the push
         * rate is a choice rather than a spin. */
        vTaskDelay(pdMS_TO_TICKS(1000));
    }

    free(lines);
    httpd_resp_send_chunk(req, NULL, 0);

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * Diagnostics: the chip's own numbers, and what the network does
 * ------------------------------------------------------------------------- */

/* A TCP connect and the time it took. Not an ICMP echo: raw sockets are a capability this box has no
 * other use for, and "can I open a connection to the thing I actually talk to" is the question a
 * person is asking when they ask whether the network works. */
static int net_probe(const char *host, int port, int timeout_ms, char *ip_out, size_t ip_cap, int *rtt_ms)
{
    char port_text[8];
    snprintf(port_text, sizeof port_text, "%d", port);

    struct addrinfo  hints = {0};
    struct addrinfo *found = NULL;

    hints.ai_family   = AF_INET;
    hints.ai_socktype = SOCK_STREAM;

    if (getaddrinfo(host, port_text, &hints, &found) != 0 || found == NULL) {
        return -1; /* the name did not resolve */
    }

    struct sockaddr_in target;
    memcpy(&target, found->ai_addr, sizeof target);

    if (ip_out != NULL && ip_cap > 0) {
        inet_ntoa_r(target.sin_addr, ip_out, (int)ip_cap);
    }

    freeaddrinfo(found);

    int fd = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (fd < 0) {
        return -2;
    }

    int flags = fcntl(fd, F_GETFL, 0);
    if (flags >= 0) {
        fcntl(fd, F_SETFL, flags | O_NONBLOCK);
    }

    int64_t started = esp_timer_get_time();

    if (connect(fd, (struct sockaddr *)&target, sizeof target) != 0 && errno != EINPROGRESS) {
        close(fd);
        return -2;
    }

    struct timeval wait = {
        .tv_sec  = timeout_ms / 1000,
        .tv_usec = (timeout_ms % 1000) * 1000,
    };

    fd_set writable;
    fd_set failed;
    FD_ZERO(&writable);
    FD_ZERO(&failed);
    FD_SET(fd, &writable);
    FD_SET(fd, &failed);

    int ready = select(fd + 1, NULL, &writable, &failed, &wait);
    if (ready <= 0) {
        close(fd);
        return -3; /* nothing answered in time */
    }

    int       error  = 0;
    socklen_t length = sizeof error;
    if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &length) != 0 || error != 0) {
        close(fd);
        return -4; /* reached, and refused */
    }

    if (rtt_ms != NULL) {
        *rtt_ms = (int)((esp_timer_get_time() - started) / 1000);
    }

    close(fd);

    return 0;
}

static esp_err_t handle_diag(httpd_req_t *req)
{
    web_buf_t buf;
    buf_init(&buf, 2048);

    int32_t temperature_dc = 0;
    bool    have_temp = hal_power_get_chip_temperature_dc(&temperature_dc) == ESP_OK;

    buf_puts(&buf, "{");

    if (have_temp) {
        buf_printf(&buf, "\"chip_temp_c\":%.1f", (double)temperature_dc / 10.0);
    } else {
        buf_puts(&buf, "\"chip_temp_c\":null");
    }

    buf_printf(&buf, ",\"uptime\":%lld", esp_timer_get_time() / 1000000);
    buf_printf(&buf, ",\"heap_free\":%u,\"heap_min\":%u,\"psram_free\":%u",
               (unsigned)esp_get_free_heap_size(),
               (unsigned)esp_get_minimum_free_heap_size(),
               (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    buf_printf(&buf, ",\"reset\":\"%s\"", reset_reason_name(esp_reset_reason()));

    /* The resolver's own view, which is the one the box uses and the one that is wrong when a name
     * stops working and an address does not. */
    esp_netif_t *station = esp_netif_get_handle_from_ifkey("WIFI_STA_DEF");
    if (station != NULL) {
        esp_netif_dns_info_t dns;
        if (esp_netif_get_dns_info(station, ESP_NETIF_DNS_MAIN, &dns) == ESP_OK) {
            char server[16];
            if (inet_ntoa_r(dns.ip.u_addr.ip4, server, (int)sizeof server) != NULL) {
                buf_puts(&buf, ",\"dns\":");
                buf_json(&buf, server);
            }
        }
    }

    hal_wifi_status_t wifi;
    if (hal_wifi_get_status(&wifi) == ESP_OK) {
        buf_printf(&buf, ",\"wifi\":{\"state\":\"%s\",\"rssi\":%d,\"ip\":",
                   wifi_state_name(wifi.state), (int)wifi.rssi);
        buf_json(&buf, wifi.ip);
        buf_puts(&buf, ",\"gateway\":");
        buf_json(&buf, wifi.gateway);
        buf_puts(&buf, ",\"netmask\":");
        buf_json(&buf, wifi.netmask);
        buf_puts(&buf, "}");
    }

    /* Reached only when asked: the probe blocks this task for up to its timeout, and a page being
     * drawn does not need to wait for somebody else's server. */
    char host[128];
    if (query_string(req, "host", host, sizeof host) && host[0] != '\0') {
        char port_text[8];
        int  port = 80;

        if (query_string(req, "port", port_text, sizeof port_text)) {
            int asked = atoi(port_text);
            if (asked > 0 && asked <= 65535) {
                port = asked;
            }
        }

        char ip[16] = "";
        int  rtt_ms = 0;
        int  result = net_probe(host, port, 2500, ip, sizeof ip, &rtt_ms);

        buf_puts(&buf, ",\"probe\":{\"host\":");
        buf_json(&buf, host);
        buf_printf(&buf, ",\"port\":%d,\"result\":", port);

        switch (result) {
        case 0:
            buf_printf(&buf, "\"ok\",\"ms\":%d,\"ip\":", rtt_ms);
            buf_json(&buf, ip);
            break;
        case -1: buf_puts(&buf, "\"dns_failed\""); break;
        case -3: buf_puts(&buf, "\"timeout\"");    break;
        case -4: buf_puts(&buf, "\"refused\"");    break;
        default: buf_puts(&buf, "\"error\"");      break;
        }

        buf_puts(&buf, "}");
    }

    buf_puts(&buf, "}");

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* ---------------------------------------------------------------------------
 * Writing a file in place: the editor's save
 * ------------------------------------------------------------------------- */

/* The body of a text file, as `POST /api/write?path=...`. The whole body is the file, exactly as an
 * upload's is and for the same reason: the page is JavaScript, and a `Blob` is a body.
 *
 * What differs from an upload is the intent. An upload puts a *new* file beside the old one and
 * renames it over, because the bytes came from outside and a half-written upload must not touch what
 * was there. An editor's save is the person who was reading the file writing it back, so it goes
 * straight over: no `.part`, no rename, and no temporary file left in a listing to be puzzled over. */
#define WEB_WRITE_MAX (256 * 1024)

static esp_err_t handle_write(httpd_req_t *req)
{
    char path[WEB_PATH_MAX];
    if (!query_string(req, "path", path, sizeof path)) {
        return send_blocked(req, "path is required");
    }

    char clean[WEB_PATH_MAX];
    if (!path_clean(path, clean, sizeof clean)) {
        return send_blocked(req, "path is not under a mounted volume");
    }

    hal_storage_volume_t volume;
    if (volume_for(clean, &volume) && strcmp(clean, volume.mount_point) == 0) {
        return send_blocked(req, "a mount point is not a file");
    }

    if (req->content_len > WEB_WRITE_MAX) {
        return send_error(req, "413 Payload Too Large", "too_big", "larger than a text file");
    }

    /* The directories above it, so that a save to a path the page is about to create works. */
    char parent[WEB_PATH_MAX];
    snprintf(parent, sizeof parent, "%s", clean);

    char *slash = strrchr(parent, '/');
    if (slash != NULL && slash != parent) {
        *slash = '\0';
        make_dirs(parent);
    }

    FILE *file = fopen(clean, "wb");
    if (file == NULL) {
        return send_error(req, "500 Internal Server Error", "write_failed", strerror(errno));
    }

    char   chunk[WEB_CHUNK];
    size_t received = 0;
    int    timeouts = 0;

    while (req->content_len > (int)received) {
        int read = httpd_req_recv(req, chunk, sizeof chunk);

        if (read == HTTPD_SOCK_ERR_TIMEOUT) {
            if (++timeouts > 3) {
                break;
            }
            continue;
        }

        if (read <= 0) {
            break;
        }

        timeouts = 0;

        if (fwrite(chunk, 1, (size_t)read, file) != (size_t)read) {
            fclose(file);
            return send_error(req, "500 Internal Server Error", "write_failed", strerror(errno));
        }

        received += (size_t)read;
    }

    if (fclose(file) != 0) {
        return send_error(req, "500 Internal Server Error", "write_failed", strerror(errno));
    }

    if ((int)received != req->content_len) {
        return send_error(req, "408 Request Timeout", "upload_incomplete", "the body was cut short");
    }

    ESP_LOGI(TAG, "wrote %u bytes to %s", (unsigned)received, clean);

    web_buf_t buf;
    buf_init(&buf, 512);
    buf_puts(&buf, "{\"ok\":true,\"path\":");
    buf_json(&buf, clean);
    buf_printf(&buf, ",\"bytes\":%u}", (unsigned)received);

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* ---------------------------------------------------------------------------
 * Moving a file, including between the two volumes
 * ------------------------------------------------------------------------- */

/* `rename` covers the same filesystem and nothing else. Between LittleFS on the flash and FATFS on
 * the card it fails — they are different drivers, and there is no inode to move.
 *
 * The move a person means there is a copy and a delete: the copy goes under the destination's name
 * and the original goes only once the copy is closed and whole. A card that fills up leaves the
 * original where it was, and the half-written copy is removed. */
static esp_err_t move_file(const char *from, const char *to)
{
    if (rename(from, to) == 0) {
        return ESP_OK;
    }

    FILE *source = fopen(from, "rb");
    if (source == NULL) {
        return ESP_FAIL;
    }

    FILE *destination = fopen(to, "wb");
    if (destination == NULL) {
        fclose(source);
        return ESP_FAIL;
    }

    char     *buffer = malloc(WEB_CHUNK);
    size_t    read;
    esp_err_t ret = ESP_OK;

    if (buffer == NULL) {
        ret = ESP_ERR_NO_MEM;
    } else {
        while ((read = fread(buffer, 1, WEB_CHUNK, source)) > 0) {
            if (fwrite(buffer, 1, read, destination) != read) {
                ret = ESP_FAIL;
                break;
            }
        }

        free(buffer);
    }

    fclose(source);

    if (fclose(destination) != 0) {
        ret = ESP_FAIL;
    }

    if (ret != ESP_OK) {
        remove(to);
        return ret;
    }

    if (remove(from) != 0) {
        /* Both copies are whole, which is a state a person can see and fix. Deleting the original
         * without a copy that took is the one that cannot. */
        ESP_LOGW(TAG, "copied %s to %s but could not remove the original", from, to);
        return ESP_FAIL;
    }

    return ESP_OK;
}

static esp_err_t handle_move(httpd_req_t *req)
{
    char raw_from[WEB_QUERY_MAX];
    char raw_to[WEB_QUERY_MAX];
    char flag[8];

    if (!query_string(req, "from", raw_from, sizeof raw_from) ||
        !query_string(req, "to", raw_to, sizeof raw_to)) {
        return send_blocked(req, "from and to are required");
    }

    bool overwrite = query_string(req, "overwrite", flag, sizeof flag) && strcmp(flag, "1") == 0;

    char from[WEB_PATH_MAX];
    char to[WEB_PATH_MAX];
    if (!path_clean(raw_from, from, sizeof from) || !path_clean(raw_to, to, sizeof to)) {
        return send_blocked(req, "a path is not under a mounted volume");
    }

    hal_storage_volume_t volume;
    if (volume_for(from, &volume) && strcmp(from, volume.mount_point) == 0) {
        return send_blocked(req, "a mount point cannot be moved");
    }

    struct stat info;
    if (stat(from, &info) != 0) {
        return send_error(req, "404 Not Found", "no_such_file", strerror(errno));
    }

    /* A destination that is a directory means "into it", which is what dragging a file onto a folder
     * means. Otherwise the destination is the name the file is to take. */
    char        target[WEB_PATH_MAX];
    struct stat into;

    if (stat(to, &into) == 0 && S_ISDIR(into.st_mode)) {
        const char *base = strrchr(from, '/');
        base = base != NULL ? base + 1 : from;

        if (snprintf(target, sizeof target, "%s/%s", to, base) >= (int)sizeof target) {
            return send_blocked(req, "the path is too long");
        }
    } else if (snprintf(target, sizeof target, "%s", to) >= (int)sizeof target) {
        return send_blocked(req, "the path is too long");
    }

    char clean_target[WEB_PATH_MAX];
    if (!path_clean(target, clean_target, sizeof clean_target)) {
        return send_blocked(req, "the destination is not under a mounted volume");
    }

    if (strcmp(from, clean_target) == 0) {
        return send_blocked(req, "the source and the destination are the same");
    }

    struct stat existing;
    if (stat(clean_target, &existing) == 0) {
        if (!overwrite) {
            return send_error(req, "409 Conflict", "already_exists", "the destination is taken");
        }

        if (remove(clean_target) != 0) {
            return send_error(req, "409 Conflict", "directory_not_empty", strerror(errno));
        }
    }

    if (S_ISDIR(info.st_mode)) {
        /* A directory move is a rename or nothing: carrying one across the two filesystems would be a
         * progress bar and a partial state, and this page has neither. */
        if (rename(from, clean_target) != 0) {
            return send_error(req, "400 Bad Request", "cross_volume",
                              "a directory cannot be moved between volumes");
        }
    } else if (move_file(from, clean_target) != ESP_OK) {
        return send_error(req, "500 Internal Server Error", "move_failed", strerror(errno));
    }

    ESP_LOGI(TAG, "moved %s to %s", from, clean_target);

    web_buf_t buf;
    buf_init(&buf, 512);
    buf_puts(&buf, "{\"ok\":true,\"path\":");
    buf_json(&buf, clean_target);
    buf_puts(&buf, "}");

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* ---------------------------------------------------------------------------
 * An archive of several files, streamed as it is built
 * ------------------------------------------------------------------------- */

/* A STORE-only zip — no compression — because the box is a microcontroller and what is being packed
 * is mostly already-compressed: audio, pictures, and text too small to be worth a deflate window.
 * What compression would buy is worth less than the library it would cost, and the format is the
 * same either way to everything that opens the result.
 *
 * Entries are written with a data descriptor (bit 3 of the local header's flags): the CRC and the
 * sizes come *after* the bytes instead of before them, which is the difference between one pass over
 * each file and two. Every unzip in a browser and on a desktop reads them — the archive is correct,
 * not merely tolerated. */
#define ZIP_NAME_MAX 200
#define ZIP_MAX_ENTRIES WEB_MAX_ENTRIES

typedef struct {
    char     name[ZIP_NAME_MAX];
    uint32_t crc;
    uint32_t size;
    uint32_t offset;
} zip_entry_t;

typedef struct {
    httpd_req_t *req;
    uint32_t     offset;
    zip_entry_t *entries;
    size_t       count;
    bool         overflow;
    bool         aborted;
} zip_ctx_t;

/* The CRC-32 the format specifies, table built on first use. It is the one piece of arithmetic a
 * stored zip still needs. */
static uint32_t s_crc_table[256];
static bool     s_crc_ready = false;

static void crc_build(void)
{
    for (uint32_t i = 0; i < 256; i++) {
        uint32_t value = i;

        for (int bit = 0; bit < 8; bit++) {
            value = (value & 1) ? (0xEDB88320u ^ (value >> 1)) : (value >> 1);
        }

        s_crc_table[i] = value;
    }

    s_crc_ready = true;
}

static uint32_t crc_update(uint32_t crc, const uint8_t *data, size_t length)
{
    if (!s_crc_ready) {
        crc_build();
    }

    crc = crc ^ 0xFFFFFFFFu;

    for (size_t i = 0; i < length; i++) {
        crc = s_crc_table[(crc ^ data[i]) & 0xFF] ^ (crc >> 8);
    }

    return crc ^ 0xFFFFFFFFu;
}

static void put16(uint8_t *at, uint16_t value)
{
    at[0] = (uint8_t)(value & 0xFF);
    at[1] = (uint8_t)((value >> 8) & 0xFF);
}

static void put32(uint8_t *at, uint32_t value)
{
    at[0] = (uint8_t)(value & 0xFF);
    at[1] = (uint8_t)((value >> 8) & 0xFF);
    at[2] = (uint8_t)((value >> 16) & 0xFF);
    at[3] = (uint8_t)((value >> 24) & 0xFF);
}

static bool zip_send(zip_ctx_t *ctx, const void *data, size_t length)
{
    if (ctx->aborted || length == 0) {
        return !ctx->aborted;
    }

    if (httpd_resp_send_chunk(ctx->req, (const char *)data, (ssize_t)length) != ESP_OK) {
        ctx->aborted = true;
        return false;
    }

    ctx->offset += (uint32_t)length;

    return true;
}

/* The format's date and time, which are a compression of the calendar invented in 1980 and not in
 * any way the same thing as a Unix timestamp. */
static void zip_dos_time(time_t when, uint16_t *out_date, uint16_t *out_time)
{
    struct tm parts;

    if (localtime_r(&when, &parts) == NULL) {
        *out_date = (uint16_t)(((2024 - 1980) << 9) | (1 << 5) | 1);
        *out_time = 0;
        return;
    }

    int year = parts.tm_year + 1900;
    if (year < 1980) {
        year = 1980;
    }

    *out_date = (uint16_t)(((year - 1980) << 9) | ((parts.tm_mon + 1) << 5) | parts.tm_mday);
    *out_time = (uint16_t)((parts.tm_hour << 11) | (parts.tm_min << 5) | (parts.tm_sec / 2));
}

static void zip_add_file(zip_ctx_t *ctx, const char *fs_path, const char *arc_name)
{
    if (ctx->aborted || ctx->overflow) {
        return;
    }

    size_t name_len = strlen(arc_name);
    if (name_len == 0 || name_len >= ZIP_NAME_MAX) {
        return;
    }

    struct stat info;
    if (stat(fs_path, &info) != 0 || S_ISDIR(info.st_mode)) {
        return;
    }

    FILE *file = fopen(fs_path, "rb");
    if (file == NULL) {
        return;
    }

    uint16_t dos_date;
    uint16_t dos_time;
    zip_dos_time(info.st_mtime, &dos_date, &dos_time);

    uint8_t header[30];
    put32(header + 0, 0x04034b50);
    put16(header + 4, 20);      /* version needed */
    put16(header + 6, 0x0008);  /* bit 3: the CRC and the sizes follow the data */
    put16(header + 8, 0);       /* stored */
    put16(header + 10, dos_time);
    put16(header + 12, dos_date);
    put32(header + 14, 0);      /* crc, in the descriptor */
    put32(header + 18, 0);
    put32(header + 22, 0);
    put16(header + 26, (uint16_t)name_len);
    put16(header + 28, 0);      /* no extra field */

    uint32_t start = ctx->offset;

    if (!zip_send(ctx, header, sizeof header) || !zip_send(ctx, arc_name, name_len)) {
        fclose(file);
        return;
    }

    char     buffer[WEB_CHUNK];
    size_t   read;
    uint32_t crc  = 0;
    uint32_t size = 0;

    while ((read = fread(buffer, 1, sizeof buffer, file)) > 0) {
        crc = crc_update(crc, (const uint8_t *)buffer, read);
        size += (uint32_t)read;

        if (!zip_send(ctx, buffer, read)) {
            fclose(file);
            return;
        }
    }

    fclose(file);

    uint8_t descriptor[16];
    put32(descriptor + 0, 0x08074b50);
    put32(descriptor + 4, crc);
    put32(descriptor + 8, size);
    put32(descriptor + 12, size);

    if (!zip_send(ctx, descriptor, sizeof descriptor)) {
        return;
    }

    if (ctx->count == ZIP_MAX_ENTRIES) {
        ctx->overflow = true;
        return;
    }

    zip_entry_t *entry = &ctx->entries[ctx->count++];
    memcpy(entry->name, arc_name, name_len + 1);
    entry->crc    = crc;
    entry->size   = size;
    entry->offset = start;
}

/* A directory is walked into rather than stored as an entry: an archive of a folder that opens to
 * the folder's contents is what a person unzipping it expects, and directory entries are optional in
 * the format. Depth is capped, because a card can hold a symlink-less tree deep enough to be a
 * fallback for a runaway recursion. */
static void zip_add_tree(zip_ctx_t *ctx, const char *fs_dir, const char *arc_prefix, int depth)
{
    if (ctx->aborted || ctx->overflow || depth > 8) {
        return;
    }

    DIR *dir = opendir(fs_dir);
    if (dir == NULL) {
        return;
    }

    struct dirent *item;
    while (!ctx->aborted && !ctx->overflow && (item = readdir(dir)) != NULL) {
        if (strcmp(item->d_name, ".") == 0 || strcmp(item->d_name, "..") == 0) {
            continue;
        }

        char full[WEB_PATH_MAX];
        char name[ZIP_NAME_MAX];

        if (snprintf(full, sizeof full, "%s/%s", fs_dir, item->d_name) >= (int)sizeof full ||
            snprintf(name, sizeof name, "%s/%s", arc_prefix, item->d_name) >= (int)sizeof name) {
            continue;
        }

        struct stat info;
        if (stat(full, &info) != 0) {
            continue;
        }

        if (S_ISDIR(info.st_mode)) {
            zip_add_tree(ctx, full, name, depth + 1);
        } else {
            zip_add_file(ctx, full, name);
        }
    }

    closedir(dir);
}

/* The list of what to pack arrives as the body, one path per line: a URL long enough to hold a
 * browser's idea of "these twenty files" is a URL a proxy truncates, and the paths contain the
 * characters a separator would have been. The answer is the archive, as it is built. */
static esp_err_t handle_archive(httpd_req_t *req)
{
    if (req->content_len <= 0 || req->content_len > WEB_JSON_MAX) {
        return send_blocked(req, "a list of paths is required");
    }

    char *body = heap_caps_malloc((size_t)req->content_len + 1, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (body == NULL) {
        body = malloc((size_t)req->content_len + 1);
    }
    if (body == NULL) {
        return send_busy(req);
    }

    int received = 0;
    int timeouts = 0;

    while (received < req->content_len) {
        int read = httpd_req_recv(req, body + received, req->content_len - received);

        if (read == HTTPD_SOCK_ERR_TIMEOUT) {
            if (++timeouts > 3) {
                break;
            }
            continue;
        }

        if (read <= 0) {
            break;
        }

        timeouts = 0;
        received += read;
    }

    body[received] = '\0';

    if (received != req->content_len) {
        free(body);
        return send_error(req, "408 Request Timeout", "upload_incomplete", "the body was cut short");
    }

    zip_ctx_t ctx = {0};
    ctx.req = req;
    ctx.entries = heap_caps_malloc(sizeof(zip_entry_t) * ZIP_MAX_ENTRIES,
                                   MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (ctx.entries == NULL) {
        ctx.entries = malloc(sizeof(zip_entry_t) * ZIP_MAX_ENTRIES);
    }

    if (ctx.entries == NULL) {
        free(body);
        return send_busy(req);
    }

    char name[WEB_NAME_MAX];
    if (!query_string(req, "name", name, sizeof name) || name[0] == '\0') {
        snprintf(name, sizeof name, "pomelo-archive.zip");
    }

    httpd_resp_set_type(req, "application/zip");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");

    char disposition[WEB_NAME_MAX + 32];
    snprintf(disposition, sizeof disposition, "attachment; filename=\"%s\"", name);
    httpd_resp_set_hdr(req, "Content-Disposition", disposition);

    for (char *line = body; line != NULL && !ctx.aborted && !ctx.overflow; ) {
        char *next = strchr(line, '\n');
        if (next != NULL) {
            *next++ = '\0';
        }

        if (line[0] != '\0') {
            char        clean[WEB_PATH_MAX];
            struct stat info;

            if (path_clean(line, clean, sizeof clean) && stat(clean, &info) == 0) {
                const char *base = strrchr(clean, '/');
                base = base != NULL ? base + 1 : clean;

                /* A mount point would recurse into the whole volume, which is a download nobody
                 * meant to start. */
                hal_storage_volume_t volume;
                bool is_mount = volume_for(clean, &volume) && strcmp(clean, volume.mount_point) == 0;

                if (!is_mount) {
                    if (S_ISDIR(info.st_mode)) {
                        zip_add_tree(&ctx, clean, base, 0);
                    } else {
                        zip_add_file(&ctx, clean, base);
                    }
                }
            }
        }

        line = next;
    }

    /* The central directory, then where it is. Both are built after the fact from the entries
     * recorded on the way through, which is the whole reason the entries are recorded. */
    uint32_t directory_offset = ctx.offset;

    for (size_t i = 0; i < ctx.count && !ctx.aborted; i++) {
        zip_entry_t *entry   = &ctx.entries[i];
        size_t       name_len = strlen(entry->name);

        uint8_t record[46];
        put32(record + 0, 0x02014b50);
        put16(record + 4, 20);       /* version made by */
        put16(record + 6, 20);       /* version needed */
        put16(record + 8, 0x0008);   /* bit 3, matching the local headers */
        put16(record + 10, 0);       /* stored */
        put16(record + 12, 0);       /* time, already written locally */
        put16(record + 14, 0);       /* date, likewise */
        put32(record + 16, entry->crc);
        put32(record + 20, entry->size);
        put32(record + 24, entry->size);
        put16(record + 28, (uint16_t)name_len);
        put16(record + 30, 0);       /* extra */
        put16(record + 32, 0);       /* comment */
        put16(record + 34, 0);       /* disk */
        put16(record + 36, 0);       /* internal attributes */
        put32(record + 38, 0);       /* external attributes */
        put32(record + 42, entry->offset);

        if (!zip_send(&ctx, record, sizeof record) || !zip_send(&ctx, entry->name, name_len)) {
            break;
        }
    }

    uint32_t directory_size = ctx.offset - directory_offset;

    uint8_t end[22];
    put32(end + 0, 0x06054b50);
    put16(end + 4, 0);
    put16(end + 6, 0);
    put16(end + 8, (uint16_t)ctx.count);
    put16(end + 10, (uint16_t)ctx.count);
    put32(end + 12, directory_size);
    put32(end + 16, directory_offset);
    put16(end + 20, 0);

    zip_send(&ctx, end, sizeof end);

    httpd_resp_send_chunk(req, NULL, 0);

    ESP_LOGI(TAG, "packed %u files into %s", (unsigned)ctx.count, name);

    free(ctx.entries);
    free(body);

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * What is taking up the room
 * ------------------------------------------------------------------------- */

/* The walk is bounded twice over: a count of entries and a wall-clock deadline. Both matter. A card
 * can hold more files than a request should hold the CPU for, and FATFS answers `stat` with a
 * directory walk of its own, so a hundred thousand of them is minutes. A truncated answer that says
 * so is worth more than an answer that arrives after the socket has given up. */
#define WEB_ANALYSIS_BUDGET 3000
#define WEB_ANALYSIS_MS     2500
#define WEB_ANALYSIS_TOP    10

typedef struct {
    char          path[WEB_PATH_MAX];
    unsigned long size;
} analysis_top_t;

typedef struct {
    uint64_t       bytes;
    size_t         files;
    size_t         directories;
    size_t         visited;
    int64_t        deadline;
    bool           truncated;
    analysis_top_t top[WEB_ANALYSIS_TOP];
} analysis_t;

static void analysis_note(analysis_t *scan, const char *path, unsigned long size)
{
    /* An insertion sort into a fixed list of ten, over a list that is mostly not full: the honest
     * structure for "the ten biggest" in a walk that cannot keep everything. */
    size_t at = WEB_ANALYSIS_TOP;

    for (size_t i = 0; i < WEB_ANALYSIS_TOP; i++) {
        if (scan->top[i].path[0] == '\0' || size > scan->top[i].size) {
            at = i;
            break;
        }
    }

    if (at == WEB_ANALYSIS_TOP) {
        return;
    }

    for (size_t i = WEB_ANALYSIS_TOP - 1; i > at; i--) {
        scan->top[i] = scan->top[i - 1];
    }

    snprintf(scan->top[at].path, sizeof scan->top[at].path, "%s", path);
    scan->top[at].size = size;
}

static void analysis_walk(const char *dir, int depth, analysis_t *scan)
{
    /* Recursion, so the depth is a stack bound as much as a sanity bound: each frame holds a
     * `WEB_PATH_MAX` path, and the handler is already on the server's 8 KB task stack. */
    if (scan->truncated || depth > 8) {
        scan->truncated = true;
        return;
    }

    if (scan->visited >= WEB_ANALYSIS_BUDGET || esp_timer_get_time() > scan->deadline) {
        scan->truncated = true;
        return;
    }

    DIR *handle = opendir(dir);
    if (handle == NULL) {
        return;
    }

    struct dirent *item;

    while ((item = readdir(handle)) != NULL) {
        if (strcmp(item->d_name, ".") == 0 || strcmp(item->d_name, "..") == 0) {
            continue;
        }

        if (++scan->visited >= WEB_ANALYSIS_BUDGET || esp_timer_get_time() > scan->deadline) {
            scan->truncated = true;
            break;
        }

        char full[WEB_PATH_MAX];
        if (snprintf(full, sizeof full, "%s/%s", dir, item->d_name) >= (int)sizeof full) {
            scan->truncated = true;
            continue;
        }

        struct stat info;
        if (stat(full, &info) != 0) {
            continue;
        }

        if (S_ISDIR(info.st_mode)) {
            scan->directories++;
            analysis_walk(full, depth + 1, scan);
        } else {
            scan->files++;
            scan->bytes += (uint64_t)info.st_size;
            analysis_note(scan, full, (unsigned long)info.st_size);
        }
    }

    closedir(handle);
}

static esp_err_t handle_analysis(httpd_req_t *req)
{
    char raw[WEB_QUERY_MAX];
    if (!query_string(req, "path", raw, sizeof raw)) {
        return send_blocked(req, "path is required");
    }

    char clean[WEB_PATH_MAX];
    if (!path_clean(raw, clean, sizeof clean)) {
        return send_blocked(req, "path is not under a mounted volume");
    }

    struct stat info;
    if (stat(clean, &info) != 0 || !S_ISDIR(info.st_mode)) {
        return send_error(req, "404 Not Found", "no_such_directory", strerror(errno));
    }

    analysis_t *scan = heap_caps_malloc(sizeof(analysis_t), MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (scan == NULL) {
        scan = malloc(sizeof(analysis_t));
    }
    if (scan == NULL) {
        return send_busy(req);
    }

    memset(scan, 0, sizeof *scan);
    scan->deadline = esp_timer_get_time() + (int64_t)WEB_ANALYSIS_MS * 1000;

    analysis_walk(clean, 0, scan);

    web_buf_t buf;
    buf_init(&buf, 2048);

    buf_puts(&buf, "{\"path\":");
    buf_json(&buf, clean);
    buf_printf(&buf, ",\"bytes\":%llu,\"files\":%u,\"directories\":%u,\"scanned\":%u,\"truncated\":%s",
               (unsigned long long)scan->bytes, (unsigned)scan->files, (unsigned)scan->directories,
               (unsigned)scan->visited, scan->truncated ? "true" : "false");

    buf_puts(&buf, ",\"largest\":[");
    for (size_t i = 0; i < WEB_ANALYSIS_TOP && scan->top[i].path[0] != '\0'; i++) {
        if (i > 0) {
            buf_puts(&buf, ",");
        }

        buf_puts(&buf, "{\"path\":");
        buf_json(&buf, scan->top[i].path);
        buf_printf(&buf, ",\"size\":%lu}", scan->top[i].size);
    }
    buf_puts(&buf, "]}");

    free(scan);

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* ---------------------------------------------------------------------------
 * The screen itself: its brightness, its sleep, and its picture
 * ------------------------------------------------------------------------- */

/* The idle sleep, which is the one thing on this page that acts on its own. It is off by default and
 * only ever runs when a page has asked it to: a box whose screen goes dark because a browser once
 * mentioned it would be a bug, not a feature.
 *
 * "Idle" means nothing has touched the glass and nothing has asked the server anything — the two
 * kinds of attention a box on a shelf receives. Touching wakes it back up, and, since a wake without
 * the touch reaching the UI would be a tap that did nothing, the touch is never consumed here; only
 * its timestamp is read. */
static esp_timer_handle_t s_idle_timer   = NULL;
static volatile int32_t   s_idle_seconds = 0;
static volatile bool      s_idle_asleep  = false;

static void idle_sleep_wake(void)
{
    if (s_idle_asleep) {
        s_idle_asleep = false;
        hal_display_set_power(true);
    }
}

static void idle_tick(void *argument)
{
    (void)argument;

    if (s_idle_seconds <= 0) {
        idle_sleep_wake();
        return;
    }

    int64_t now      = esp_timer_get_time();
    int64_t touched  = hal_touch_last_activity_us();
    int64_t asked    = s_last_request_us;
    int64_t last     = touched > asked ? touched : asked;
    int64_t limit_us = (int64_t)s_idle_seconds * 1000000;

    if (now - last >= limit_us) {
        if (!s_idle_asleep) {
            s_idle_asleep = true;
            hal_display_set_power(false);
            ESP_LOGI(TAG, "the screen is asleep after %d s", (int)s_idle_seconds);
        }
    } else {
        idle_sleep_wake();
    }
}

static esp_err_t handle_display(httpd_req_t *req)
{
    char on_text[8];
    char brightness_text[8];
    char idle_text[8];

    bool want_on         = query_string(req, "on", on_text, sizeof on_text);
    bool want_brightness = query_string(req, "brightness", brightness_text, sizeof brightness_text);
    bool want_idle       = query_string(req, "idle", idle_text, sizeof idle_text);

    /* Asking for nothing is asking what it is. The page opens the screen card before it has anything
     * to show, and the shape of the answer is the same either way: what the panel is doing now. */
    if (want_on) {
        bool on = atoi(on_text) != 0;
        hal_display_set_power(on);
        if (on) {
            s_idle_asleep = false;
        }
    }

    if (want_brightness) {
        int level = atoi(brightness_text);
        if (level < 0) {
            level = 0;
        }
        if (level > 255) {
            level = 255;
        }
        hal_display_set_brightness((uint8_t)level);
    }

    if (want_idle) {
        int minutes = atoi(idle_text);
        if (minutes < 0) {
            minutes = 0;
        }
        if (minutes > 120) {
            minutes = 120;
        }

        s_idle_seconds = minutes * 60;

        if (s_idle_seconds > 0) {
            if (s_idle_timer == NULL) {
                esp_timer_create_args_t args = {
                    .callback = idle_tick,
                    .name     = "web_idle",
                };
                esp_timer_create(&args, &s_idle_timer);
            }

            if (s_idle_timer != NULL) {
                esp_timer_start_periodic(s_idle_timer, 1000000);
            }
        } else if (s_idle_timer != NULL) {
            esp_timer_stop(s_idle_timer);
            idle_sleep_wake();
        }
    }

    web_buf_t buf;
    buf_init(&buf, 256);
    buf_printf(&buf, "{\"ok\":true,\"on\":%s,\"brightness\":%u,\"idle_seconds\":%d,\"capture\":%s}",
               hal_display_is_powered() ? "true" : "false",
               (unsigned)hal_display_get_brightness(),
               (int)s_idle_seconds,
               hal_display_has_capture() ? "true" : "false");

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

/* The screen as a picture.
 *
 * The panel cannot be read back — the CO5300 has no path that returns its GRAM over QSPI — so the
 * picture is the shadow copy the display driver keeps on the way past (see `s_shadow` in
 * board_display.c). What that copy holds is exactly what the panel was last given, which is exactly
 * what is on the screen.
 *
 * BMP and not PNG: twenty-four bits per pixel, bottom-up rows, and no library. It is the one image
 * format whose header is thirty bytes of arithmetic and whose body is the pixels, and a browser shows
 * one without being asked twice. */
static esp_err_t handle_screenshot(httpd_req_t *req)
{
    if (!hal_display_has_capture()) {
        return send_error(req, "501 Not Implemented", "no_capture",
                          "no PSRAM for the shadow frame");
    }

    const int width  = BOARD_DISPLAY_WIDTH;
    const int height = BOARD_DISPLAY_HEIGHT;

    uint16_t *frame = heap_caps_malloc((size_t)width * height * sizeof(uint16_t),
                                       MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (frame == NULL) {
        return send_busy(req);
    }

    if (hal_display_capture(frame, (size_t)width * height) != ESP_OK) {
        free(frame);
        return send_error(req, "500 Internal Server Error", "capture_failed", NULL);
    }

    /* Sent in bands rather than as one buffer: the whole image is 691 KB of a page's answer, and
     * allocating it twice — once as the shadow, once as the conversion — is PSRAM spent on nothing.
     * Sixty-four rows is 92 KB, which is a chunk httpd is happy to hand to the socket. */
    const size_t rows_per_band = 64;
    const size_t row_bytes     = (size_t)width * 3;
    const size_t band_bytes    = row_bytes * rows_per_band;
    const size_t image_bytes   = row_bytes * (size_t)height;

    uint8_t *band = heap_caps_malloc(band_bytes, MALLOC_CAP_SPIRAM | MALLOC_CAP_8BIT);
    if (band == NULL) {
        band = malloc(band_bytes);
    }
    if (band == NULL) {
        free(frame);
        return send_busy(req);
    }

    uint8_t header[54];
    memset(header, 0, sizeof header);

    header[0] = 'B';
    header[1] = 'M';
    put32(header + 2, (uint32_t)(sizeof header + image_bytes));
    put32(header + 10, (uint32_t)sizeof header);
    put32(header + 14, 40);                          /* the DIB header */
    put32(header + 18, (uint32_t)width);
    /* Negative height: rows are stored top-down, which is the order the shadow already has them in. */
    put32(header + 22, (uint32_t)(-height));
    put16(header + 26, 1);
    put16(header + 28, 24);
    put32(header + 34, (uint32_t)image_bytes);
    put32(header + 38, 2835);                        /* 72 dpi */
    put32(header + 42, 2835);

    httpd_resp_set_type(req, "image/bmp");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    httpd_resp_set_hdr(req, "Content-Disposition", "inline; filename=\"screenshot.bmp\"");

    esp_err_t ret = httpd_resp_send_chunk(req, (const char *)header, sizeof header);

    for (int y = 0; y < height && ret == ESP_OK; y += (int)rows_per_band) {
        int rows = height - y;
        if (rows > (int)rows_per_band) {
            rows = (int)rows_per_band;
        }

        for (int r = 0; r < rows; r++) {
            const uint16_t *source = frame + (size_t)(y + r) * width;
            uint8_t        *target = band + (size_t)r * row_bytes;

            for (int x = 0; x < width; x++) {
                uint16_t pixel = source[x];

                /* 5-6-5 widened to 8-8-8 by repeating the high bits into the low ones, which is what
                 * keeps white white and black black. BMP wants blue first. */
                uint8_t red   = (uint8_t)((pixel >> 11) & 0x1F);
                uint8_t green = (uint8_t)((pixel >> 5) & 0x3F);
                uint8_t blue  = (uint8_t)(pixel & 0x1F);

                target[x * 3 + 0] = (uint8_t)((blue << 3) | (blue >> 2));
                target[x * 3 + 1] = (uint8_t)((green << 2) | (green >> 4));
                target[x * 3 + 2] = (uint8_t)((red << 3) | (red >> 2));
            }
        }

        ret = httpd_resp_send_chunk(req, (const char *)band, (ssize_t)(row_bytes * (size_t)rows));
    }

    httpd_resp_send_chunk(req, NULL, 0);

    free(band);
    free(frame);

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * Driving the box from the page
 * ------------------------------------------------------------------------- */

/* A finger, from a browser. The event goes into the same queue the panel's own touches arrive on, so
 * the UI above it cannot tell the difference — which is what makes this a remote control rather than
 * a second way to talk to the apps. */
static esp_err_t handle_remote(httpd_req_t *req)
{
    char type[8];
    char text[8];

    if (!query_string(req, "type", type, sizeof type)) {
        return send_blocked(req, "type is required");
    }

    int x = 0;
    int y = 0;

    if (query_string(req, "x", text, sizeof text)) {
        x = atoi(text);
    }
    if (query_string(req, "y", text, sizeof text)) {
        y = atoi(text);
    }

    esp_err_t ret;

    if (strcmp(type, "down") == 0) {
        ret = hal_touch_inject(HAL_TOUCH_EVENT_DOWN, x, y);
    } else if (strcmp(type, "move") == 0) {
        ret = hal_touch_inject(HAL_TOUCH_EVENT_MOVE, x, y);
    } else if (strcmp(type, "up") == 0) {
        ret = hal_touch_inject(HAL_TOUCH_EVENT_UP, x, y);
    } else if (strcmp(type, "tap") == 0) {
        ret = hal_touch_inject(HAL_TOUCH_EVENT_DOWN, x, y);
        if (ret == ESP_OK) {
            /* Long enough for the UI to have moved past the press before the release arrives: a
             * down and an up in the same millisecond is a click in the abstract and nothing at all
             * in a state machine that wants a frame between them. */
            vTaskDelay(pdMS_TO_TICKS(60));
            ret = hal_touch_inject(HAL_TOUCH_EVENT_UP, x, y);
        }
    } else {
        return send_blocked(req, "type is tap, down, move or up");
    }

    if (ret != ESP_OK && ret != ESP_ERR_TIMEOUT) {
        return send_error(req, "500 Internal Server Error", "touch_failed", esp_err_to_name(ret));
    }

    web_buf_t buf;
    buf_init(&buf, 128);
    buf_printf(&buf, "{\"ok\":true,\"type\":\"%s\",\"x\":%d,\"y\":%d}", type, x, y);

    esp_err_t answer = send_json(req, buf.data);
    buf_free(&buf);

    return answer;
}

/* A tone out of the speaker, which is the "where is it" button: a box on a shelf with no screen
 * pointed at it and a page that can make it beep is a box you can find. Blocking for as long as the
 * tone lasts, which is capped well inside the server's patience. */
static esp_err_t handle_audio(httpd_req_t *req)
{
    char text[16];

    int frequency = 880;
    int duration  = 300;
    int volume    = 101; /* above 100 means "leave the current volume alone" */

    if (query_string(req, "freq", text, sizeof text)) {
        frequency = atoi(text);
    }
    if (query_string(req, "ms", text, sizeof text)) {
        duration = atoi(text);
    }
    if (query_string(req, "volume", text, sizeof text)) {
        volume = atoi(text);
    }

    if (frequency < 1 || frequency > 7000) {
        return send_blocked(req, "freq must be between 1 and 7000 Hz");
    }

    if (duration < 1) {
        duration = 1;
    }
    if (duration > 2000) {
        duration = 2000;
    }

    if (volume > 100) {
        volume = 101;
    }
    if (volume < 0) {
        volume = 0;
    }

    esp_err_t played = hal_audio_tone((uint32_t)frequency, (uint32_t)duration, (uint8_t)volume);
    if (played != ESP_OK) {
        return send_error(req, "500 Internal Server Error", "audio_failed", esp_err_to_name(played));
    }

    web_buf_t buf;
    buf_init(&buf, 128);
    buf_printf(&buf, "{\"ok\":true,\"freq\":%d,\"ms\":%d}", frequency, duration);

    esp_err_t answer = send_json(req, buf.data);
    buf_free(&buf);

    return answer;
}

/* ---------------------------------------------------------------------------
 * The clock
 * ------------------------------------------------------------------------- */

static esp_err_t handle_rtc(httpd_req_t *req)
{
    time_t   system_now = time(NULL);
    time_t   chip_now   = 0;
    esp_err_t chip_read = hal_rtc_get_epoch(&chip_now);

    web_buf_t buf;
    buf_init(&buf, 512);

    buf_printf(&buf, "{\"epoch\":%lld,\"valid\":%s,\"uptime\":%lld", (long long)system_now,
               system_now > 1704067200 ? "true" : "false",
               (long long)(esp_timer_get_time() / 1000000));

    /* The chip is a thing that may not be there — this board has one, the next revision may not — so
     * the answer carries it only when it answered, and the object closes either way. */
    if (chip_read == ESP_OK) {
        struct tm parts;
        char      text[32];

        if (gmtime_r(&chip_now, &parts) != NULL &&
            strftime(text, sizeof text, "%Y-%m-%dT%H:%M:%SZ", &parts) > 0) {
            buf_puts(&buf, ",\"rtc\":{\"epoch\":");
            buf_printf(&buf, "%lld,\"iso\":", (long long)chip_now);
            buf_json(&buf, text);
            buf_puts(&buf, "}");
        }
    }

    buf_puts(&buf, "}");

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    esp_err_t ret = send_json(req, buf.data);
    buf_free(&buf);

    return ret;
}

static esp_err_t handle_rtc_set(httpd_req_t *req)
{
    char text[24];

    if (!query_string(req, "epoch", text, sizeof text)) {
        return send_blocked(req, "epoch is required");
    }

    /* Deliberately a string and not a float: a browser's `Date.now() / 1000` is a number with a
     * fraction on it if it is careless, and the fraction is the year 2100 somewhere if it is parsed
     * at the wrong width. Seconds, whole, as text. */
    long long epoch = strtoll(text, NULL, 10);

    esp_err_t set = hal_rtc_set_epoch((time_t)epoch);
    if (set == ESP_ERR_INVALID_ARG) {
        return send_blocked(req, "the time is before 2024");
    }
    if (set != ESP_OK) {
        return send_error(req, "500 Internal Server Error", "rtc_failed", esp_err_to_name(set));
    }

    /* Read back rather than echo: whether the chip took the write is the answer, and the only way to
     * know is to ask it. */
    time_t now = time(NULL);

    web_buf_t buf;
    buf_init(&buf, 256);
    buf_printf(&buf, "{\"ok\":true,\"epoch\":%lld}", (long long)now);

    esp_err_t answer = send_json(req, buf.data);
    buf_free(&buf);

    return answer;
}

/* ---------------------------------------------------------------------------
 * Being an app: the manifest, the icon, and the service worker
 *
 * All three are only consulted by a browser that considers this a secure origin — HTTPS, or the
 * `localhost` exception. The box's own page is served over plain HTTP on whatever address the router
 * handed it, so on a LAN these go unread and the page is a page. Behind a reverse proxy, or on a
 * network that has been given a certificate, they are what turns it into something installed: the
 * same routes, answering a question that is only asked in a secure context.
 * ------------------------------------------------------------------------- */

static esp_err_t handle_manifest(httpd_req_t *req)
{
    char running[64];
    app_name_copy(running, sizeof running);

    /* Built as one string first and escaped once: the app name is whatever the firmware set it to,
     * and a name with a quote in it would otherwise end the JSON string early. */
    char title[96];
    snprintf(title, sizeof title, running[0] ? "Pomelo - %s" : "Pomelo", running);

    web_buf_t buf;
    buf_init(&buf, 512);

    buf_puts(&buf, "{\"name\":");
    buf_json(&buf, title);
    buf_printf(&buf, ",\"short_name\":\"Pomelo\",\"start_url\":\"/\",\"scope\":\"/\","
                     "\"display\":\"standalone\",\"orientation\":\"any\","
                     "\"background_color\":\"#0b0b0f\",\"theme_color\":\"#0b0b0f\","
                     "\"icons\":[{\"src\":\"/icon.svg\",\"sizes\":\"any\","
                     "\"type\":\"image/svg+xml\",\"purpose\":\"any\"},"
                     "{\"src\":\"/icon.svg\",\"sizes\":\"any\","
                     "\"type\":\"image/svg+xml\",\"purpose\":\"maskable\"}]}");

    if (buf.full) {
        buf_free(&buf);
        return send_busy(req);
    }

    httpd_resp_set_type(req, "application/manifest+json; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");

    esp_err_t ret = httpd_resp_send(req, buf.data, HTTPD_RESP_USE_STRLEN);
    buf_free(&buf);

    return ret;
}

/* A pomelo: a circle, a leaf, and the two dots that make it a face. Drawn rather than stored as a
 * bitmap because the box has no image decoder and a browser has an SVG renderer. */
static const char WEB_ICON_SVG[] =
    "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 512 512\">"
    "<rect width=\"512\" height=\"512\" rx=\"96\" fill=\"#0b0b0f\"/>"
    "<circle cx=\"256\" cy=\"292\" r=\"150\" fill=\"#ffd25f\"/>"
    "<path d=\"M256 146c0-44 34-78 78-78-6 44-34 78-78 78z\" fill=\"#5ec268\"/>"
    "<circle cx=\"204\" cy=\"276\" r=\"17\" fill=\"#0b0b0f\"/>"
    "<circle cx=\"308\" cy=\"276\" r=\"17\" fill=\"#0b0b0f\"/>"
    "<path d=\"M196 336c34 34 86 34 120 0\" stroke=\"#0b0b0f\" stroke-width=\"20\" "
    "stroke-linecap=\"round\" fill=\"none\"/>"
    "</svg>";

static esp_err_t handle_icon(httpd_req_t *req)
{
    httpd_resp_set_type(req, "image/svg+xml");
    /* Cached: it is the same circle forever, and it is asked for on every install. */
    httpd_resp_set_hdr(req, "Cache-Control", "public, max-age=86400");

    return httpd_resp_send(req, WEB_ICON_SVG, HTTPD_RESP_USE_STRLEN);
}

/* Network first, and the cache is the fallback for the one case it is for: a page opened where the
 * box cannot be reached. Caching the shell first would serve yesterday's page after a firmware
 * update, which on a device whose whole point is that its page is generated from its state is a
 * worse failure than a blank tab. */
static const char WEB_SERVICE_WORKER_JS[] =
    "const CACHE = 'pomelo-shell-v1';\n"
    "self.addEventListener('install', (event) => { self.skipWaiting(); });\n"
    "self.addEventListener('activate', (event) => {\n"
    "  event.waitUntil(self.clients.claim());\n"
    "});\n"
    "self.addEventListener('fetch', (event) => {\n"
    "  const request = event.request;\n"
    "  if (request.method !== 'GET' || new URL(request.url).pathname.startsWith('/api/')) {\n"
    "    return;\n"
    "  }\n"
    "  event.respondWith((async () => {\n"
    "    try {\n"
    "      const fresh = await fetch(request);\n"
    "      if (fresh.ok && fresh.type === 'basic') {\n"
    "        const copy = fresh.clone();\n"
    "        caches.open(CACHE).then((cache) => cache.put(request, copy)).catch(() => {});\n"
    "      }\n"
    "      return fresh;\n"
    "    } catch (error) {\n"
    "      const cached = await caches.match(request);\n"
    "      if (cached) { return cached; }\n"
    "      throw error;\n"
    "    }\n"
    "  })());\n"
    "});\n";

static esp_err_t handle_service_worker(httpd_req_t *req)
{
    httpd_resp_set_type(req, "text/javascript; charset=utf-8");
    /* A worker served from a stale cache is a worker that never updates, and a worker that never
     * updates is a page that never updates. */
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    /* The worker's scope is the directory it is served from, and this one is served from the root —
     * which is the whole page. */
    httpd_resp_set_hdr(req, "Service-Worker-Allowed", "/");

    return httpd_resp_send(req, WEB_SERVICE_WORKER_JS, HTTPD_RESP_USE_STRLEN);
}
