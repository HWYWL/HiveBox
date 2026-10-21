// =============================================================================
// WorkBuddy account status
// -----------------------------------------------------------------------------
// "Is my WorkBuddy account signed in today, and how long does the token last?" The
// board holds one token and can ask the account two questions: what the daily
// check-in activity looks like, and to check in. `hal_workbuddy_start` names the
// question and returns; a worker does the handshake and the read, and the answer is
// *taken* rather than read twice -- the same shape as `board_cloud.c` and
// `board_http.c`, and for the same reason: this is called from a frame loop and
// from an HTTP handler, and neither can wait seconds for a TLS handshake.
//
// -----------------------------------------------------------------------------
// Why this is not `board_http.c`
// -----------------------------------------------------------------------------
// The other client is a GET: a URL goes in and bytes come out. This one is a POST
// with an `Authorization` header of its own, and the board's `HttpBackend` is
// deliberately GET-only (`pomelo-hal`'s `traits/http.rs` says so out loud). So this
// file owns its own `esp_http_client`, like `board_cloud.c`, and the Rust side
// reaches it through `hal_workbuddy_*`.
//
// -----------------------------------------------------------------------------
// The interface
// -----------------------------------------------------------------------------
// WorkBuddy's client talks to `https://www.workbuddy.cn` (the auth file's `domain`)
// with a bearer token:
//
//     POST /v2/billing/meter/checkin-activity-status
//     Authorization: Bearer <access token>
//     Content-Type: application/json
//     body: {}
//
// and gets, under `data`, `today_checked_in`, `streak_days`, `daily_credit`,
// `total_credits`, `theme_name`, `activity_name`, `start_time`, `end_time`.
// `/v2/billing/meter/daily-checkin` is the same call with a different path, and is
// idempotent: a second check-in of the day answers code 10001 rather than taking
// anything twice.
//
// The credits *balance* is not one of these two -- that endpoint is not known yet,
// and a page that draws a number nobody can fetch draws a lie. So the answer carries
// `credit_remaining` and `credit_total` as empty strings, which the app draws as a
// dash, and filling them in later is one function here rather than a new trait.
//
// -----------------------------------------------------------------------------
// The token's lifetime
// -----------------------------------------------------------------------------
// The access token is a JWT, and its `exp` claim is the only reliable "when does
// this run out" there is -- the file's `expiresAt` is written by somebody else and
// is not on the board. So the payload is base64url-decoded here and its `exp` read,
// and the answer says how many days are left. A token that cannot be decoded says
// -1, which is "unknown" and not "expired".
//
// -----------------------------------------------------------------------------
// The token
// -----------------------------------------------------------------------------
// One plaintext file, `/internal/AppData/WORKBUDDY/workbuddy.conf`:
//
//     host = "www.workbuddy.cn"
//     token = "eyJhbGciOi..."
//     refresh = "eyJhbGciOi..."
//
// Plaintext, like the Wi-Fi password and the NAS logins, and for the same reason:
// this board has no secure element and no key store, and a token that has to be
// retyped after every flash is a token nobody keeps. It is nearly two kilobytes and
// nobody is going to type it on a touch keyboard, so it arrives through the web
// page (`/api/workbuddy/token`, where a person pastes it) or through a file dropped
// on the card. Nothing here hands the token back out: `hal_workbuddy_credentials_get`
// answers with a mask and the host, which is what a page draws "one is saved" from.
// =============================================================================

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "esp_crt_bundle.h"
#include "esp_err.h"
#include "esp_heap_caps.h"
#include "esp_http_client.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"

#include "board_hal.h"

static const char *TAG = "board_workbuddy";

/** Where the token lives. Plaintext, and the whole of the reason is at the top. */
static const char *WB_CONF_PATH = "/internal/AppData/WORKBUDDY/workbuddy.conf";

/** The host the token was issued for. `www.workbuddy.cn` is the auth file's own `domain`. */
static const char *WB_DEFAULT_HOST = "www.workbuddy.cn";

/** The two calls this file knows. Both are POSTs with an empty object for a body. */
static const char *WB_STATUS_PATH  = "/v2/billing/meter/checkin-activity-status";
static const char *WB_CHECKIN_PATH = "/v2/billing/meter/daily-checkin";

/** How long the whole call may take: resolve, connect, handshake, POST, answer. */
#define WB_TIMEOUT_MS 15000

/** The worker's stack. The deep part is the handshake; `board_cloud.c` pays the same. */
#define WB_TASK_STACK 8192

/** The socket's read buffer, and the one headers and the request body are written through. */
#define WB_IO_CHUNK 2048

/** What one answer may be. The activity document is a few hundred bytes; the rest is headroom. */
#define WB_RESPONSE_MAX 8192

/** The rendered answer: ten short lines. */
#define WB_ANSWER_MAX 1024

/** How many times a call is tried when it was the wire that failed. */
#define WB_TRIES 2

/** `code` in a successful answer. Anything else is the API saying why not. */
#define WB_CODE_OK 0

/** The answer to a check-in that has already happened today. */
#define WB_CODE_ALREADY 10001

// =============================================================================
// State
// =============================================================================

typedef struct {
    int32_t state;
    int32_t fault;
    /** Length of the answer in `s_answer`, meaningful while the state is DONE or FAILED. */
    size_t length;
    /** A worker task is alive. */
    bool busy;
    /** Asked to stop since the worker started. The worker looks before it sends. */
    bool cancelled;
} wb_snapshot_t;

static wb_snapshot_t s_snap = { .state = HAL_WORKBUDDY_IDLE };

static SemaphoreHandle_t s_lock = NULL;

/** The rendered answer, out of PSRAM by preference: nothing needs it fast. */
static char *s_answer = NULL;

/** The raw body of the last answer. */
static char *s_response = NULL;

/** The account, as loaded from the file. */
typedef struct {
    bool used;
    char host[HAL_WORKBUDDY_HOST_MAX];
    char token[HAL_WORKBUDDY_TOKEN_MAX];
    char refresh[HAL_WORKBUDDY_REFRESH_MAX];
} wb_account_t;

static wb_account_t s_account;

/** Whether the account above has been read from the file yet. */
static bool s_loaded = false;

/** What the worker is doing. Written under the lock, read by the worker while `busy`. */
static int32_t s_action = HAL_WORKBUDDY_ACTION_STATUS;

static inline void lock(void)
{
    if (s_lock) {
        xSemaphoreTake(s_lock, portMAX_DELAY);
    }
}

static inline void unlock(void)
{
    if (s_lock) {
        xSemaphoreGive(s_lock);
    }
}

// =============================================================================
// The two refusals that need no network
// =============================================================================

static bool network_is_up(void)
{
    hal_wifi_status_t status;
    if (hal_wifi_get_status(&status) != ESP_OK) {
        return false;
    }
    return status.state == HAL_WIFI_STATE_CONNECTED;
}

// =============================================================================
// JSON, read only
// =============================================================================

/**
 * The value that follows `"key"`, or NULL.
 *
 * `strstr` and not a scan, because the key is looked for with its quotes *and* the
 * colon after it: a key name appearing inside another key's value fails the colon
 * test and the search carries on. Same function as `board_cloud.c`, and kept here
 * rather than shared because the two files have no other business with each other.
 */
static const char *json_value(const char *from, const char *key)
{
    if (from == NULL) {
        return NULL;
    }

    char pattern[32];
    const int made = snprintf(pattern, sizeof pattern, "\"%s\"", key);
    if (made < 0 || (size_t)made >= sizeof pattern) {
        return NULL;
    }

    const size_t width = (size_t)made;
    const char *at = from;

    while ((at = strstr(at, pattern)) != NULL) {
        const char *value = at + width;

        while (*value == ' ' || *value == '\t' || *value == '\n' || *value == '\r') {
            value++;
        }

        if (*value == ':') {
            value++;
            while (*value == ' ' || *value == '\t' || *value == '\n' || *value == '\r') {
                value++;
            }
            return value;
        }

        at += width;
    }

    return NULL;
}

/**
 * A string value copied out, with its escapes resolved.
 *
 * `\uXXXX` becomes UTF-8: the activity's name is Chinese and arrives as either form
 * depending on which server answered. A surrogate pair is beyond this -- it would
 * need the second half -- and an emoji in an activity name is not worth that.
 */
static bool json_string(const char *value, char *out, size_t capacity)
{
    if (value == NULL || *value != '"' || capacity == 0) {
        return false;
    }

    value++;

    size_t used = 0;
    while (*value != '\0' && *value != '"') {
        unsigned long code = 0;

        if (*value == '\\') {
            value++;
            if (*value == '\0') {
                break;
            }

            switch (*value) {
            case 'n': code = '\n'; value++; break;
            case 't': code = '\t'; value++; break;
            case 'r': code = '\r'; value++; break;
            case 'b': code = '\b'; value++; break;
            case 'f': code = '\f'; value++; break;
            case 'u':
                value++;
                for (int i = 0; i < 4 && *value != '\0'; i++, value++) {
                    const char digit = *value;
                    unsigned long nibble;
                    if (digit >= '0' && digit <= '9') {
                        nibble = (unsigned long)(digit - '0');
                    } else if (digit >= 'a' && digit <= 'f') {
                        nibble = (unsigned long)(digit - 'a' + 10);
                    } else if (digit >= 'A' && digit <= 'F') {
                        nibble = (unsigned long)(digit - 'A' + 10);
                    } else {
                        nibble = 0;
                    }
                    code = (code << 4) | nibble;
                }
                break;
            default:
                code = (unsigned char)*value;
                value++;
                break;
            }
        } else {
            code = (unsigned char)*value;
            value++;
        }

        char utf8[4];
        size_t width;

        if (code < 0x80) {
            utf8[0] = (char)code;
            width = 1;
        } else if (code < 0x800) {
            utf8[0] = (char)(0xC0 | (code >> 6));
            utf8[1] = (char)(0x80 | (code & 0x3F));
            width = 2;
        } else {
            utf8[0] = (char)(0xE0 | (code >> 12));
            utf8[1] = (char)(0x80 | ((code >> 6) & 0x3F));
            utf8[2] = (char)(0x80 | (code & 0x3F));
            width = 3;
        }

        if (used + width >= capacity) {
            break;
        }

        for (size_t i = 0; i < width; i++) {
            out[used++] = utf8[i];
        }
    }

    out[used] = '\0';

    return used > 0;
}

/** Whether `value` starts an object, i.e. the `data` of an answer. */
static const char *json_object(const char *value)
{
    return (value != NULL && *value == '{') ? value : NULL;
}

/**
 * A number that may be written as a number or as a string.
 *
 * `"code":"0"` and `"code":0` are both in the wild on this API, and a page that
 * said "refused" because the zero was quoted would be a page that says the wrong
 * thing about a working account.
 */
static bool json_number(const char *value, long long *out)
{
    if (value == NULL) {
        return false;
    }

    if (*value == '"') {
        value++;
    }

    if (*value == '\0') {
        return false;
    }

    char *end = NULL;
    const long long number = strtoll(value, &end, 10);
    if (end == value) {
        return false;
    }

    *out = number;
    return true;
}

/** A boolean that may be `true`, `"true"`, `1` or `"1"`. */
static bool json_boolean(const char *value, bool *out)
{
    if (value == NULL) {
        return false;
    }

    if (*value == '"') {
        value++;
    }

    if (strncmp(value, "true", 4) == 0) {
        *out = true;
        return true;
    }
    if (strncmp(value, "false", 5) == 0) {
        *out = false;
        return true;
    }

    char *end = NULL;
    const long long number = strtoll(value, &end, 10);
    if (end == value) {
        return false;
    }

    *out = number != 0;
    return true;
}

// =============================================================================
// The token's lifetime: a JWT payload, base64url-decoded
// =============================================================================

/**
 * The `exp` claim of a JWT, or 0 when there is none this file can read.
 *
 * A JWT is three base64url runs separated by dots and the middle one is a JSON
 * object. Only that object is decoded, into a small buffer: an `exp` is a ten-digit
 * number near a key of three characters, and the claims around it are of no interest.
 */
static long long jwt_expires_at(const char *token)
{
    if (token == NULL) {
        return 0;
    }

    const char *payload = strchr(token, '.');
    if (payload == NULL) {
        return 0;
    }
    payload++;

    const char *end = strchr(payload, '.');
    if (end == NULL) {
        return 0;
    }

    /* The decoded payload is at most three quarters of the encoded run. The claims of
     * a token this board holds are well under a kilobyte, so a small buffer is enough
     * and the tail -- other claims -- is simply not decoded. */
    char decoded[512];
    size_t used = 0;

    unsigned int accumulator = 0;
    int bits = 0;

    for (const char *at = payload; at < end && used < sizeof decoded - 1; at++) {
        unsigned int value;

        if (*at >= 'A' && *at <= 'Z') {
            value = (unsigned int)(*at - 'A');
        } else if (*at >= 'a' && *at <= 'z') {
            value = (unsigned int)(*at - 'a') + 26;
        } else if (*at >= '0' && *at <= '9') {
            value = (unsigned int)(*at - '0') + 52;
        } else if (*at == '-' || *at == '+') {
            value = 62;
        } else if (*at == '_' || *at == '/') {
            value = 63;
        } else if (*at == '=') {
            break;
        } else {
            /* Not base64url: a payload this file cannot read is "unknown", not "expired". */
            return 0;
        }

        accumulator = (accumulator << 6) | value;
        bits += 6;

        if (bits >= 8) {
            bits -= 8;
            decoded[used++] = (char)((accumulator >> bits) & 0xFF);
        }
    }

    decoded[used] = '\0';

    long long exp = 0;
    if (!json_number(json_value(decoded, "exp"), &exp)) {
        return 0;
    }

    return exp;
}

/** Days from now until `exp`, or -1 when the board has no clock or the claim is missing. */
static long long days_until(long long exp)
{
    if (exp <= 0) {
        return -1;
    }

    int64_t now = 0;
    if (hal_system_get_epoch(&now) != ESP_OK || now <= 1577836800) {
        return -1;
    }

    const long long left = exp - (long long)now;
    if (left <= 0) {
        return 0;
    }

    return left / 86400;
}

// =============================================================================
// The file
// =============================================================================

/** The value of `key = "..."` in the file, with escapes resolved. */
static bool conf_string(const char *from, const char *key, char *out, size_t capacity)
{
    return json_string(json_value(from, key), out, capacity);
}

static esp_err_t wb_load(void)
{
    char *text = (char *)malloc(HAL_WORKBUDDY_CONF_MAX);

    if (text == NULL) {
        return ESP_ERR_NO_MEM;
    }

    size_t length = 0;
    const esp_err_t read = hal_storage_read_file(WB_CONF_PATH, text, HAL_WORKBUDDY_CONF_MAX, &length);

    if (read != ESP_OK) {
        free(text);
        memset(&s_account, 0, sizeof s_account);
        return read;
    }

    memset(&s_account, 0, sizeof s_account);

    if (!conf_string(text, "token", s_account.token, sizeof s_account.token)) {
        /* A file without a token is a file that says nothing: no account. */
        free(text);
        return ESP_OK;
    }

    if (!conf_string(text, "host", s_account.host, sizeof s_account.host) || s_account.host[0] == '\0') {
        snprintf(s_account.host, sizeof s_account.host, "%s", WB_DEFAULT_HOST);
    }

    if (!conf_string(text, "refresh", s_account.refresh, sizeof s_account.refresh)) {
        s_account.refresh[0] = '\0';
    }

    s_account.used = true;

    free(text);

    return ESP_OK;
}

static esp_err_t wb_store(void)
{
    char text[HAL_WORKBUDDY_CONF_MAX];
    size_t used = 0;

    const int made = snprintf(text + used, sizeof text - used,
                              "host = \"%s\"\ntoken = \"%s\"\nrefresh = \"%s\"\n",
                              s_account.host, s_account.token, s_account.refresh);

    if (made < 0 || (size_t)made >= sizeof text - used) {
        ESP_LOGE(TAG, "the token file did not fit in %d bytes", HAL_WORKBUDDY_CONF_MAX);
        return ESP_ERR_INVALID_SIZE;
    }

    used += (size_t)made;

    const esp_err_t err = hal_storage_write_file(WB_CONF_PATH, text, used);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "cannot write %s: %s", WB_CONF_PATH, esp_err_to_name(err));
        return err;
    }

    ESP_LOGI(TAG, "the token is in %s", WB_CONF_PATH);

    return ESP_OK;
}

static void wb_ensure_loaded(void)
{
    if (s_loaded) {
        return;
    }

    s_loaded = true;
    wb_load();
}

// =============================================================================
// The wire
// =============================================================================

/**
 * Opens the connection holder. No socket is opened until the first call.
 *
 * The URL is the host alone: `esp_http_client` takes the path from the post field
 * of the same call, and a URL with the sign-in path in it would be a second copy of
 * the path to keep in step with the one used in the header list below.
 */
typedef struct {
    esp_http_client_handle_t client;
    /** `https://host`, which every call's URL starts with. */
    char base[96];
    char *out;
    size_t capacity;
    size_t used;
    bool overflowed;
} wb_session_t;

static esp_err_t wb_collect(esp_http_client_event_t *event)
{
    wb_session_t *session = (wb_session_t *)event->user_data;

    if (event->event_id != HTTP_EVENT_ON_DATA || event->data == NULL || event->data_len <= 0) {
        return ESP_OK;
    }

    const size_t room = session->capacity - 1 - session->used;
    const size_t take = (size_t)event->data_len < room ? (size_t)event->data_len : room;

    memcpy(session->out + session->used, event->data, take);
    session->used += take;

    if (take < (size_t)event->data_len) {
        session->overflowed = true;
    }

    return ESP_OK;
}

static bool wb_session_open(wb_session_t *session, const char *host)
{
    memset(session, 0, sizeof *session);

    session->out      = s_response;
    session->capacity = WB_RESPONSE_MAX;

    snprintf(session->base, sizeof session->base, "https://%s", host);

    const esp_http_client_config_t config = {
        .url               = session->base,
        .method            = HTTP_METHOD_POST,
        .timeout_ms        = WB_TIMEOUT_MS,
        .buffer_size       = WB_IO_CHUNK,
        .buffer_size_tx    = WB_IO_CHUNK,
        .crt_bundle_attach = esp_crt_bundle_attach,
        .keep_alive_enable = false,
        .event_handler     = wb_collect,
        .user_data         = session,
    };

    session->client = esp_http_client_init(&config);
    if (session->client == NULL) {
        return false;
    }

    return true;
}

static void wb_session_close(wb_session_t *session)
{
    if (session->client != NULL) {
        esp_http_client_cleanup(session->client);
        session->client = NULL;
    }
}

/**
 * The fault a transport error is worth.
 *
 * The TLS half is a *range* rather than three names, because mbedtls' errors do not have stable
 * names in this tree's headers: everything in `0x8000..0x9000` and everything below `-0x1000` is the
 * handshake or the certificate, and the rest -- `ESP_ERR_HTTP_*`, timeouts -- is a wire that never
 * got there. `board_cloud.c` draws the same line in the same place.
 */
static int32_t connection_fault(esp_err_t err)
{
    if ((err >= 0x8000 && err < 0x9000) || err <= -0x1000) {
        return HAL_WORKBUDDY_FAULT_TLS;
    }

    return HAL_WORKBUDDY_FAULT_UNREACHABLE;
}

/**
 * One call: set the path and the token, send it, leave the body in `s_response`.
 *
 * Returns `HAL_WORKBUDDY_FAULT_NONE` when a whole body came back, and the fault
 * otherwise. It draws nothing: whether a body is worth drawing is the caller's
 * decision.
 */
static int32_t wb_session_call(wb_session_t *session, const char *path, const char *token,
                               int *out_status)
{
    char authorization[HAL_WORKBUDDY_TOKEN_MAX + 16];
    snprintf(authorization, sizeof authorization, "Bearer %s", token);

    session->used = 0;
    session->out[0] = '\0';
    session->overflowed = false;

    esp_http_client_set_header(session->client, "Authorization", authorization);
    esp_http_client_set_header(session->client, "Content-Type", "application/json; charset=utf-8");
    esp_http_client_set_header(session->client, "Accept", "application/json");
    /* The client's own agent: the account API is the one place that looks at it. */
    esp_http_client_set_header(session->client, "User-Agent", "WorkBuddy/0.0.0");
    esp_http_client_set_header(session->client, "X-Product", "SaaS");
    esp_http_client_set_post_field(session->client, "{}", 2);

    char url[160];
    snprintf(url, sizeof url, "%s%s", session->base, path);
    esp_http_client_set_url(session->client, url);

    ESP_LOGI(TAG, "POST %s", path);

    const esp_err_t err = esp_http_client_perform(session->client);
    const int status = (err == ESP_OK) ? esp_http_client_get_status_code(session->client) : 0;

    *out_status = status;

    if (err != ESP_OK) {
        ESP_LOGW(TAG, "the call failed: %s", esp_err_to_name(err));
        esp_http_client_close(session->client);
        return connection_fault(err);
    }

    session->out[session->used] = '\0';

    if (session->overflowed) {
        ESP_LOGW(TAG, "the answer was cut at %d bytes", WB_RESPONSE_MAX - 1);
    }

    if (session->used == 0) {
        ESP_LOGW(TAG, "the API answered %d with an empty body", status);
        return HAL_WORKBUDDY_FAULT_HTTP;
    }

    return HAL_WORKBUDDY_FAULT_NONE;
}

static int32_t wb_fetch_retrying(const char *path, const char *token, int *out_status)
{
    wb_session_t session;

    if (!wb_session_open(&session, s_account.host)) {
        return HAL_WORKBUDDY_FAULT_STORAGE;
    }

    int32_t fault = HAL_WORKBUDDY_FAULT_NONE;

    for (int attempt = 1; attempt <= WB_TRIES; attempt++) {
        fault = wb_session_call(&session, path, token, out_status);

        if (fault == HAL_WORKBUDDY_FAULT_NONE) {
            break;
        }

        const bool again = fault == HAL_WORKBUDDY_FAULT_UNREACHABLE || fault == HAL_WORKBUDDY_FAULT_TLS;

        if (!again) {
            break;
        }

        if (attempt < WB_TRIES) {
            ESP_LOGW(TAG, "attempt %d of %d got no answer, asking again", attempt, WB_TRIES);
            vTaskDelay(pdMS_TO_TICKS(250));
        }
    }

    wb_session_close(&session);

    return fault;
}

// =============================================================================
// The answer
// =============================================================================

static void settle(int32_t fault, size_t length)
{
    lock();
    s_snap.state  = (fault == HAL_WORKBUDDY_FAULT_NONE) ? HAL_WORKBUDDY_DONE : HAL_WORKBUDDY_FAILED;
    s_snap.fault  = fault;
    s_snap.length = length;
    s_snap.busy   = false;
    unlock();
}

/** The reason a call failed, as the answer a page draws. */
static void answer_fault(int32_t fault)
{
    static const char *const CODES[] = {
        /* Indexed by HAL_WORKBUDDY_FAULT_*, and the first one is never asked for. */
        "workbuddy_failed", "workbuddy_args", "workbuddy_creds", "workbuddy_network",
        "workbuddy_unreachable", "workbuddy_tls", "workbuddy_http", "workbuddy_auth",
        "workbuddy_api", "workbuddy_storage",
    };

    const size_t index = (size_t)fault;
    const char *code = (index < sizeof CODES / sizeof CODES[0]) ? CODES[index] : "workbuddy_failed";

    const int made = snprintf(s_answer, WB_ANSWER_MAX, "error=%s\n", code);

    settle(fault, (made > 0) ? (size_t)made : 0);
}

/** `true`/`false` as the one character the answer uses. */
static const char *flag(bool value)
{
    return value ? "1" : "0";
}

/**
 * Whether the account API refused the call, and why.
 *
 * `{"code":0,...}` is a working answer whose fields are what the page draws;
 * anything else is the API saying no, with `message` in it when it has one. A body
 * with no `code` at all is taken as an answer: some of these calls are plain
 * documents.
 */
static bool body_is_refusal(const char *body, long long *out_code, char *message, size_t capacity)
{
    message[0] = '\0';

    long long code = 0;
    if (!json_number(json_value(body, "code"), &code)) {
        return false;
    }

    *out_code = code;

    if (code == WB_CODE_OK || code == WB_CODE_ALREADY) {
        return false;
    }

    if (!json_string(json_value(body, "message"), message, capacity)) {
        if (!json_string(json_value(body, "msg"), message, capacity)) {
            snprintf(message, capacity, "code %lld", code);
        }
    }

    return true;
}

/** Where in the body the account's fields are: under `data` when it is an object, else the root. */
static const char *account_scope(const char *body)
{
    const char *data = json_object(json_value(body, "data"));

    return (data != NULL) ? data : body;
}

/**
 * The status call's answer: the activity, the streak, and how long the token lasts.
 *
 * `credit_remaining` and `credit_total` are empty: the balance is a third endpoint
 * that nobody has read yet, and an empty field is drawn as a dash rather than as a
 * zero somebody would believe.
 */
static void render_status(void)
{
    const char *scope = account_scope(s_response);

    bool checked_in = false;
    json_boolean(json_value(scope, "today_checked_in"), &checked_in);

    long long streak = 0;
    long long daily = 0;
    long long total = 0;
    json_number(json_value(scope, "streak_days"), &streak);
    json_number(json_value(scope, "daily_credit"), &daily);
    json_number(json_value(scope, "total_credits"), &total);

    char activity[HAL_WORKBUDDY_ACTIVITY_MAX];
    if (!json_string(json_value(scope, "theme_name"), activity, sizeof activity)) {
        if (!json_string(json_value(scope, "activity_name"), activity, sizeof activity)) {
            activity[0] = '\0';
        }
    }

    const long long access_days  = days_until(jwt_expires_at(s_account.token));
    const long long refresh_days = s_account.refresh[0] != '\0'
                                       ? days_until(jwt_expires_at(s_account.refresh))
                                       : -1;

    const int made = snprintf(s_answer, WB_ANSWER_MAX,
                              "ok=1\n"
                              "checked_in=%s\n"
                              "streak=%lld\n"
                              "daily=%lld\n"
                              "activity_total=%lld\n"
                              "activity=%s\n"
                              "access_days=%lld\n"
                              "refresh_days=%lld\n"
                              "credit_remaining=\n"
                              "credit_total=\n",
                              flag(checked_in), streak, daily, total, activity, access_days,
                              refresh_days);

    if (made < 0 || (size_t)made >= WB_ANSWER_MAX) {
        answer_fault(HAL_WORKBUDDY_FAULT_STORAGE);
        return;
    }

    settle(HAL_WORKBUDDY_FAULT_NONE, (size_t)made);
}

/**
 * The check-in answer: what today's check-in gave, and the activity as it is now.
 *
 * A check-in that has already happened today is not a failure -- the API says
 * `10001` and the honest answer is "already done", which is exactly what the page
 * would have drawn anyway.
 */
static void render_checkin(long long code)
{
    bool checked_in = true;
    long long credit = 0;

    const char *scope = account_scope(s_response);

    json_number(json_value(scope, "today_credit"), &credit);
    if (credit == 0) {
        json_number(json_value(scope, "credit"), &credit);
    }

    long long streak = 0;
    long long total = 0;
    json_number(json_value(scope, "streak_days"), &streak);
    json_number(json_value(scope, "total_credits"), &total);

    char activity[HAL_WORKBUDDY_ACTIVITY_MAX];
    if (!json_string(json_value(scope, "theme_name"), activity, sizeof activity)) {
        activity[0] = '\0';
    }

    const long long access_days  = days_until(jwt_expires_at(s_account.token));
    const long long refresh_days = s_account.refresh[0] != '\0'
                                       ? days_until(jwt_expires_at(s_account.refresh))
                                       : -1;

    const int made = snprintf(s_answer, WB_ANSWER_MAX,
                              "ok=1\n"
                              "checked_in=%s\n"
                              "streak=%lld\n"
                              "daily=%lld\n"
                              "activity_total=%lld\n"
                              "activity=%s\n"
                              "access_days=%lld\n"
                              "refresh_days=%lld\n"
                              "credit_remaining=\n"
                              "credit_total=\n"
                              "credited=%lld\n"
                              "already=%s\n",
                              flag(checked_in), streak, credit, total, activity, access_days,
                              refresh_days, credit, flag(code == WB_CODE_ALREADY));

    if (made < 0 || (size_t)made >= WB_ANSWER_MAX) {
        answer_fault(HAL_WORKBUDDY_FAULT_STORAGE);
        return;
    }

    settle(HAL_WORKBUDDY_FAULT_NONE, (size_t)made);
}

/** Turns the body of a call into the answer, or into the fault that stopped it. */
static void answer_from_body(int32_t action, int status)
{
    if (status == 401 || status == 403) {
        answer_fault(HAL_WORKBUDDY_FAULT_AUTH);
        return;
    }

    long long code = 0;
    char message[256];

    const bool refused = body_is_refusal(s_response, &code, message, sizeof message);

    if (refused) {
        const int made = snprintf(s_answer, WB_ANSWER_MAX, "error=workbuddy_api\nmessage=%s\n", message);
        settle(HAL_WORKBUDDY_FAULT_API, (made > 0) ? (size_t)made : 0);
        return;
    }

    if (status < 200 || status >= 300) {
        answer_fault(HAL_WORKBUDDY_FAULT_HTTP);
        return;
    }

    if (action == HAL_WORKBUDDY_ACTION_CHECKIN) {
        render_checkin(code);
    } else {
        render_status();
    }
}

// =============================================================================
// The worker
// =============================================================================

/**
 * One request, start to finish.
 *
 * The token is read under the lock and used without a copy: `hal_workbuddy_credentials_set`
 * refuses to write while a request is in flight, so the buffer cannot move under this.
 */
static void wb_request(void)
{
    lock();
    const int32_t action = s_action;
    const bool cancelled = s_snap.cancelled;
    unlock();

    if (cancelled) {
        lock();
        s_snap.state  = HAL_WORKBUDDY_IDLE;
        s_snap.fault  = HAL_WORKBUDDY_FAULT_NONE;
        s_snap.length = 0;
        s_snap.busy   = false;
        unlock();
        return;
    }

    lock();
    const bool have_token = s_account.used && s_account.token[0] != '\0';
    unlock();

    if (!have_token) {
        answer_fault(HAL_WORKBUDDY_FAULT_NO_CREDS);
        return;
    }

    if (!network_is_up()) {
        answer_fault(HAL_WORKBUDDY_FAULT_NO_NETWORK);
        return;
    }

    const char *path = (action == HAL_WORKBUDDY_ACTION_CHECKIN) ? WB_CHECKIN_PATH : WB_STATUS_PATH;

    int status = 0;
    const int32_t fault = wb_fetch_retrying(path, s_account.token, &status);

    if (fault != HAL_WORKBUDDY_FAULT_NONE) {
        answer_fault(fault);
        return;
    }

    answer_from_body(action, status);
}

/**
 * The worker task: one request, and then the task itself.
 *
 * The `vTaskDelete` is not tidiness: a task function that returns trips a
 * `configASSERT` and the board reboots. See `board_cloud.c`'s worker, which ends the
 * same way for the same reason.
 */
static void wb_task(void *arg)
{
    (void)arg;

    wb_request();

    vTaskDelete(NULL);
}

// =============================================================================
// What board_hal.h publishes
// =============================================================================

esp_err_t hal_workbuddy_init(void)
{
    if (s_lock == NULL) {
        s_lock = xSemaphoreCreateMutex();
        if (s_lock == NULL) {
            return ESP_ERR_NO_MEM;
        }
    }

    if (s_answer == NULL) {
        s_answer = (char *)heap_caps_malloc(WB_ANSWER_MAX, MALLOC_CAP_SPIRAM);
        if (s_answer == NULL) {
            s_answer = (char *)malloc(WB_ANSWER_MAX);
        }

        /* Cleared once, on the allocation: every later call here is a request that happens to run
         * before something else, and a blanket clear would wipe the answer the *other* caller is
         * about to take -- `hal_workbuddy_take_answer` calls this too. */
        if (s_answer != NULL) {
            s_answer[0] = '\0';
        }
    }

    if (s_response == NULL) {
        s_response = (char *)heap_caps_malloc(WB_RESPONSE_MAX, MALLOC_CAP_SPIRAM);
        if (s_response == NULL) {
            s_response = (char *)malloc(WB_RESPONSE_MAX);
        }

        if (s_response != NULL) {
            s_response[0] = '\0';
        }
    }

    if (s_answer == NULL || s_response == NULL) {
        ESP_LOGE(TAG, "no room for the answer buffers");
        return ESP_ERR_NO_MEM;
    }

    wb_ensure_loaded();

    return ESP_OK;
}

esp_err_t hal_workbuddy_start(int32_t action)
{
    if (action != HAL_WORKBUDDY_ACTION_STATUS && action != HAL_WORKBUDDY_ACTION_CHECKIN) {
        return ESP_ERR_INVALID_ARG;
    }

    if (hal_workbuddy_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();
    if (s_snap.busy) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }

    s_action = action;
    s_snap.cancelled = false;
    s_snap.state  = HAL_WORKBUDDY_RUNNING;
    s_snap.fault  = HAL_WORKBUDDY_FAULT_NONE;
    s_snap.length = 0;
    s_snap.busy   = true;

    const bool have_token = s_account.used && s_account.token[0] != '\0';
    unlock();

    /* Answered before the task starts, so a page's error arrives in the same round trip
     * as a failed request rather than seconds later through the socket's timeout. */
    if (!have_token) {
        answer_fault(HAL_WORKBUDDY_FAULT_NO_CREDS);
        return ESP_OK;
    }

    if (!network_is_up()) {
        answer_fault(HAL_WORKBUDDY_FAULT_NO_NETWORK);
        return ESP_OK;
    }

    if (xTaskCreate(wb_task, "workbuddy", WB_TASK_STACK, NULL, 5, NULL) != pdPASS) {
        answer_fault(HAL_WORKBUDDY_FAULT_STORAGE);
        return ESP_ERR_NO_MEM;
    }

    return ESP_OK;
}

int32_t hal_workbuddy_state(void)
{
    lock();
    const int32_t state = s_snap.state;
    unlock();

    return state;
}

int32_t hal_workbuddy_fault(void)
{
    lock();
    const int32_t fault = s_snap.fault;
    unlock();

    return fault;
}

const char *hal_workbuddy_answer(size_t *out_length)
{
    lock();
    const bool ready = (s_snap.state == HAL_WORKBUDDY_DONE || s_snap.state == HAL_WORKBUDDY_FAILED) &&
                       !s_snap.busy;
    const size_t length = s_snap.length;
    unlock();

    if (!ready || s_answer == NULL) {
        return NULL;
    }

    if (out_length != NULL) {
        *out_length = length;
    }

    return s_answer;
}

void hal_workbuddy_take(void)
{
    lock();
    s_snap.state  = HAL_WORKBUDDY_IDLE;
    s_snap.fault  = HAL_WORKBUDDY_FAULT_NONE;
    s_snap.length = 0;
    unlock();
}

size_t hal_workbuddy_take_answer(char *out, size_t capacity)
{
    if (out == NULL || capacity == 0) {
        return 0;
    }

    out[0] = '\0';

    if (hal_workbuddy_init() != ESP_OK) {
        return 0;
    }

    lock();

    const bool ready = (s_snap.state == HAL_WORKBUDDY_DONE || s_snap.state == HAL_WORKBUDDY_FAILED) &&
                       !s_snap.busy;
    size_t length = 0;

    if (ready && s_answer != NULL) {
        length = s_snap.length;
        if (length >= capacity) {
            length = capacity - 1;
        }

        memcpy(out, s_answer, length);
        out[length] = '\0';
    }

    s_snap.state  = HAL_WORKBUDDY_IDLE;
    s_snap.fault  = HAL_WORKBUDDY_FAULT_NONE;
    s_snap.length = 0;

    unlock();

    return length;
}

void hal_workbuddy_cancel(void)
{
    lock();
    s_snap.cancelled = true;

    if (!s_snap.busy) {
        s_snap.state  = HAL_WORKBUDDY_IDLE;
        s_snap.fault  = HAL_WORKBUDDY_FAULT_NONE;
        s_snap.length = 0;
    }

    unlock();
}

/** Whether every character is one that can live in a quoted line of the file. */
static bool token_shape_ok(const char *text, size_t capacity)
{
    if (text == NULL || text[0] == '\0') {
        return false;
    }

    for (const char *at = text; *at != '\0'; at++) {
        if ((size_t)(at - text) >= capacity) {
            return false;
        }
        if (*at == '"' || *at == '\\' || *at == '\n' || *at == '\r' || (unsigned char)*at < 0x21 ||
            (unsigned char)*at > 0x7E) {
            return false;
        }
    }

    return true;
}

/**
 * The host, as a host name and nothing else: letters, digits, dots and dashes.
 *
 * The URL is built from this by a `snprintf`, so a host with a slash or a space in
 * it would be a URL with a second request in it -- which is the whole reason this
 * check exists rather than trusting the caller.
 */
static bool host_shape_ok(const char *host)
{
    if (host == NULL || host[0] == '\0' || strlen(host) >= HAL_WORKBUDDY_HOST_MAX) {
        return false;
    }

    for (const char *at = host; *at != '\0'; at++) {
        const bool ok = (*at >= 'a' && *at <= 'z') || (*at >= 'A' && *at <= 'Z') ||
                        (*at >= '0' && *at <= '9') || *at == '.' || *at == '-';
        if (!ok) {
            return false;
        }
    }

    return true;
}

/** The last four characters of the token, which is all a page gets to see. */
static void mask_token(const char *token, char *out, size_t capacity)
{
    const size_t length = (token != NULL) ? strlen(token) : 0;

    if (length == 0 || capacity == 0) {
        out[0] = '\0';
        return;
    }

    const size_t keep = (length < 4) ? length : 4;
    snprintf(out, capacity, "…%s", token + length - keep);
}

esp_err_t hal_workbuddy_credentials_get(char *out_masked, size_t capacity, char *out_host,
                                        size_t host_capacity)
{
    if (hal_workbuddy_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();
    const bool used = s_account.used && s_account.token[0] != '\0';
    const char *host = s_account.host;

    if (out_masked != NULL && capacity > 0) {
        out_masked[0] = '\0';
        if (used) {
            mask_token(s_account.token, out_masked, capacity);
        }
    }

    if (out_host != NULL && host_capacity > 0) {
        snprintf(out_host, host_capacity, "%s", used ? host : "");
    }

    unlock();

    return used ? ESP_OK : ESP_ERR_NOT_FOUND;
}

esp_err_t hal_workbuddy_credentials_set(const char *token, const char *refresh, const char *host)
{
    if (!token_shape_ok(token, HAL_WORKBUDDY_TOKEN_MAX)) {
        return ESP_ERR_INVALID_ARG;
    }

    if (refresh != NULL && refresh[0] != '\0' && !token_shape_ok(refresh, HAL_WORKBUDDY_REFRESH_MAX)) {
        return ESP_ERR_INVALID_ARG;
    }

    if (host != NULL && host[0] != '\0' && !host_shape_ok(host)) {
        return ESP_ERR_INVALID_ARG;
    }

    if (hal_workbuddy_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();

    /* The worker reads the token in place, so it must not move underneath it. */
    if (s_snap.busy) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }

    memset(&s_account, 0, sizeof s_account);
    s_account.used = true;
    snprintf(s_account.token, sizeof s_account.token, "%s", token);
    snprintf(s_account.refresh, sizeof s_account.refresh, "%s", refresh != NULL ? refresh : "");
    snprintf(s_account.host, sizeof s_account.host, "%s",
             (host != NULL && host[0] != '\0') ? host : WB_DEFAULT_HOST);

    unlock();

    return wb_store();
}

esp_err_t hal_workbuddy_credentials_forget(void)
{
    if (hal_workbuddy_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();
    if (s_snap.busy) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }

    memset(&s_account, 0, sizeof s_account);
    unlock();

    return wb_store();
}
