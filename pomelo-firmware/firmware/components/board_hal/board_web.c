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
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <unistd.h>

#include "esp_app_desc.h"
#include "esp_chip_info.h"
#include "esp_heap_caps.h"
#include "esp_http_server.h"
#include "esp_log.h"
#include "esp_system.h"
#include "esp_timer.h"

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

    buf_printf(&buf, ",\"uptime\":%lld", esp_timer_get_time() / 1000000);
    buf_printf(&buf, ",\"heap_free\":%u", (unsigned)esp_get_free_heap_size());
    buf_printf(&buf, ",\"heap_min\":%u", (unsigned)esp_get_minimum_free_heap_size());
    buf_printf(&buf, ",\"psram_free\":%u",
               (unsigned)heap_caps_get_free_size(MALLOC_CAP_SPIRAM));
    buf_printf(&buf, ",\"reset\":\"%s\"", reset_reason_name(esp_reset_reason()));

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

    char  chunk[WEB_CHUNK];
    size_t read;

    while ((read = fread(chunk, 1, sizeof chunk, file)) > 0) {
        if (httpd_resp_send_chunk(req, chunk, (ssize_t)read) != ESP_OK) {
            /* The client went away — a tap on another tab, a phone that locked. Nothing is wrong, and
             * the file is closed either way. */
            ESP_LOGD(TAG, "download interrupted: %s", clean);
            fclose(file);
            return ESP_OK;
        }
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

    FILE *file = fopen(part, "wb");
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
        /* A short write is a file with a hole in it, and a file with a hole in it is worse than no
         * file: it is taken for one. It goes, and the answer says why. What it is not is the file that
         * was already at this name — that one is still whole and is still there. */
        remove(part);

        return send_error(req, "408 Request Timeout", "upload_incomplete", "the body was cut short");
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
    buf_printf(&buf, ",\"bytes\":%u}", (unsigned)received);

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
