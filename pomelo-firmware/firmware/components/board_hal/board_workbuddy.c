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
#include "esp_timer.h"
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

/** The two calls this file knows. Both are POSTs with an empty object for a body.
 *
 * The check-in pair live under `/v2`; the credit pair do not -- that is the API's own inconsistency,
 * read off a working client, and it is written down here so that nobody "tidies" one to match the
 * other. */
static const char *WB_STATUS_PATH  = "/v2/billing/meter/checkin-activity-status";
static const char *WB_CHECKIN_PATH = "/v2/billing/meter/daily-checkin";
static const char *WB_SUMMARY_PATH = "/billing/meter/get-user-resource-summary";
static const char *WB_USAGE_PATH   = "/billing/meter/get-user-resource";

/** How long the whole call may take: resolve, connect, handshake, POST, answer. */
#define WB_TIMEOUT_MS 15000

/** The worker's stack. The deep part is the handshake; `board_cloud.c` pays the same. */
#define WB_TASK_STACK 8192

/** The socket's read buffer, and the one headers and the request body are written through. */
#define WB_IO_CHUNK 2048

/* 一份答案能有多长：签到的几百字节，用量的四十多 KB —— 那份文档每个资源一个对象，而一个用了
 * 一年的账号有二十九个，每个对象都带着自己的字段名。它落在 PSRAM 上，那里有三兆空闲，所以给足
 * 比「读到一半就算故障」便宜。 */
/** What one answer may be. See the note above: the usage document is the long one. */
#define WB_RESPONSE_MAX 98304

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

                /* A surrogate pair is one character written as two escapes, and it is what an emoji
                 * in an activity's name arrives as. Taking the halves one at a time writes two
                 * three-byte sequences that are not valid UTF-8 -- which is the "garbled name" the
                 * page showed, not a font problem. Combined here, it is one four-byte character. */
                if (code >= 0xD800 && code <= 0xDBFF && value[0] == '\\' && value[1] == 'u') {
                    unsigned long low = 0;
                    const char *after = value + 2;

                    for (int i = 0; i < 4 && *after != '\0'; i++, after++) {
                        const char digit = *after;
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
                        low = (low << 4) | nibble;
                    }

                    if (low >= 0xDC00 && low <= 0xDFFF) {
                        code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                        value = after;
                    }
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
        } else if (code < 0x10000) {
            utf8[0] = (char)(0xE0 | (code >> 12));
            utf8[1] = (char)(0x80 | ((code >> 6) & 0x3F));
            utf8[2] = (char)(0x80 | (code & 0x3F));
            width = 3;
        } else {
            utf8[0] = (char)(0xF0 | (code >> 18));
            utf8[1] = (char)(0x80 | ((code >> 12) & 0x3F));
            utf8[2] = (char)(0x80 | ((code >> 6) & 0x3F));
            utf8[3] = (char)(0x80 | (code & 0x3F));
            width = 4;
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

/**
 * The characters of a name this panel has a glyph for, and nothing else.
 *
 * The baked table holds Latin and the CJK block; an activity's name arrives with an emoji in it
 * ("Buddy🎁签到有礼" is the kind of thing these are called), and a character with no glyph is drawn
 * as a box. A box is not the name, so the box is dropped: what is left is the part somebody can
 * read, which is better than a row of squares in the middle of it.
 *
 * Kept: ASCII, the CJK unified block, CJK punctuation, and the fullwidth forms that go with them.
 * Dropped: everything else, including the four-byte ones.
 */
static void keep_drawable(char *text)
{
    char *write = text;
    const unsigned char *at = (const unsigned char *)text;

    while (*at != 0) {
        unsigned int code;
        size_t width;

        if (*at < 0x80) {
            code = *at;
            width = 1;
        } else if ((*at & 0xE0) == 0xC0) {
            code = (unsigned int)(*at & 0x1F);
            width = 2;
        } else if ((*at & 0xF0) == 0xE0) {
            code = (unsigned int)(*at & 0x0F);
            width = 3;
        } else if ((*at & 0xF8) == 0xF0) {
            code = (unsigned int)(*at & 0x07);
            width = 4;
        } else {
            at++;
            continue;
        }

        for (size_t i = 1; i < width; i++) {
            code = (code << 6) | (unsigned int)(at[i] & 0x3F);
        }

        const bool drawable = (code < 0x80) || (code >= 0x3000 && code <= 0x303F) ||
                              (code >= 0x4E00 && code <= 0x9FFF) || (code >= 0xFF00 && code <= 0xFFEF);

        if (drawable) {
            for (size_t i = 0; i < width; i++) {
                *write++ = (char)at[i];
            }
        }

        at += width;
    }

    *write = '\0';
}

/**
 * Whether a run of bytes is UTF-8 this panel can draw.
 *
 * The check is the shape of the encoding and not a table of characters: overlong forms, a lead byte
 * with no continuation after it and a code point past `U+10FFFF` are refused, and everything else
 * passes. A name that fails is a name the account gave in some other encoding, and a tile can do
 * without it -- what it cannot do is draw it.
 */
static bool utf8_ok(const char *text)
{
    const unsigned char *at = (const unsigned char *)text;

    while (*at != 0) {
        unsigned int need;
        unsigned int code;

        if (*at < 0x80) {
            at++;
            continue;
        }

        if ((*at & 0xE0) == 0xC0) {
            need = 1;
            code = (unsigned int)(*at & 0x1F);
        } else if ((*at & 0xF0) == 0xE0) {
            need = 2;
            code = (unsigned int)(*at & 0x0F);
        } else if ((*at & 0xF8) == 0xF0) {
            need = 3;
            code = (unsigned int)(*at & 0x07);
        } else {
            return false;
        }

        at++;

        for (unsigned int i = 0; i < need; i++, at++) {
            if ((*at & 0xC0) != 0x80) {
                return false;
            }
            code = (code << 6) | (unsigned int)(*at & 0x3F);
        }

        if (code > 0x10FFFF) {
            return false;
        }
    }

    return true;
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

/**
 * Where an array ends: one past its matching `]`, or NULL.
 *
 * The credit document says the same thing twice -- `Packages` one entry per package, and
 * `CreditGroups` the same amounts split by kind -- and a scan that runs past the end of `Packages`
 * adds both, which doubles every number on the tile. Walking to the end is what stops it at the
 * boundary the account drew.
 */
static const char *array_end(const char *at)
{
    if (at == NULL || *at != '[') {
        return NULL;
    }

    int depth = 0;
    bool in_string = false;

    for (const char *p = at; *p != '\0'; p++) {
        if (in_string) {
            if (*p == '\\' && p[1] != '\0') {
                p++;
                continue;
            }
            if (*p == '"') {
                in_string = false;
            }
            continue;
        }

        if (*p == '"') {
            in_string = true;
        } else if (*p == '[' || *p == '{') {
            depth++;
        } else if (*p == ']' || *p == '}') {
            depth--;
            if (depth == 0) {
                return p + 1;
            }
        }
    }

    return NULL;
}

/**
 * A number that may arrive as a number or as a quoted string.
 *
 * The billing document writes its capacities as strings -- `"CycleRemainCapacity":"2200"` -- and a
 * `strtod` on one of those reads the quote, not the number, and answers zero. Which is what this
 * file did, and what drew a true account as "0.0 / 0.0".
 */
static bool json_decimal(const char *value, double *out)
{
    if (value == NULL) {
        return false;
    }

    if (*value == '"') {
        value++;
    }

    char *end = NULL;
    const double number = strtod(value, &end);

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

/**
 * The value of `key = "..."` in the file, with escapes resolved.
 *
 * The file is written as `host = "..."` and not as JSON, so the JSON scanner beside this one finds
 * nothing in it: it looks for the quoted key. Reading it back was the bug that lost a sign-in on
 * every reboot -- the token was stored, and then the next boot could not find it. Both shapes are
 * accepted here, because a file written by an older build is still a file this build must read.
 */
static bool conf_string(const char *from, const char *key, char *out, size_t capacity)
{
    const char *value = json_value(from, key);

    if (json_string(value, out, capacity)) {
        return true;
    }

    /* `key = "value"`: find the key, skip to the `=`, then hand the quoted string to the same reader
     * the JSON path uses, so escapes are resolved one way in both. */
    const char *at = from;

    while ((at = strstr(at, key)) != NULL) {
        /* The bare word must not be the tail of a longer one: `token` inside `refresh_token`. */
        const bool starts_word = (at == from) || (at[-1] != '_' && at[-1] != '"' && at[-1] != '\'');

        const char *after = at + strlen(key);
        while (*after == ' ' || *after == '\t') {
            after++;
        }

        if (starts_word && *after == '=') {
            after++;
            while (*after == ' ' || *after == '\t') {
                after++;
            }

            if (*after == '"' && json_string(after, out, capacity)) {
                return true;
            }
        }

        at += strlen(key);
    }

    out[0] = '\0';

    return false;
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
                               esp_http_client_method_t method, const char *body, int *out_status)
{
    session->used = 0;
    session->out[0] = '\0';
    session->overflowed = false;

    /* 授权之前没有令牌可带，带上空的那个反而被网关当成「有凭证但不合法」。 */
    if (token != NULL && token[0] != '\0') {
        char authorization[HAL_WORKBUDDY_TOKEN_MAX + 16];
        snprintf(authorization, sizeof authorization, "Bearer %s", token);
        esp_http_client_set_header(session->client, "Authorization", authorization);
    } else {
        esp_http_client_set_header(session->client, "Authorization", NULL);
        esp_http_client_set_header(session->client, "X-No-Authorization", "true");
        esp_http_client_set_header(session->client, "X-No-User-Id", "true");
        esp_http_client_set_header(session->client, "X-No-Enterprise-Id", "true");
        esp_http_client_set_header(session->client, "X-No-Department-Info", "true");
    }

    esp_http_client_set_header(session->client, "Content-Type", "application/json; charset=utf-8");
    esp_http_client_set_header(session->client, "Accept", "application/json");
    /* The client's own agent: the account API is the one place that looks at it. */
    esp_http_client_set_header(session->client, "User-Agent", "WorkBuddy/5.4.2");
    esp_http_client_set_header(session->client, "X-Product", "SaaS");
    esp_http_client_set_header(session->client, "X-Domain", s_account.host[0] != '\0'
                                                            ? s_account.host
                                                            : WB_DEFAULT_HOST);
    esp_http_client_set_method(session->client, method);

    if (method == HTTP_METHOD_POST) {
        const char *payload = (body != NULL) ? body : "{}";
        esp_http_client_set_post_field(session->client, payload, (int)strlen(payload));
    }

    char url[160];
    snprintf(url, sizeof url, "%s%s", session->base, path);
    esp_http_client_set_url(session->client, url);

    ESP_LOGI(TAG, "%s %s", method == HTTP_METHOD_GET ? "GET" : "POST", path);

    const esp_err_t err = esp_http_client_perform(session->client);

    /* The status is read even when `perform` failed, because a 401 is not a failed call: the
     * account answered, and its answer is "that token is not one of mine". `esp_http_client` calls
     * it `ESP_ERR_NOT_SUPPORTED` and throws the body away when the 401 carries a
     * `WWW-Authenticate: Bearer ...` header -- which is exactly the shape this API answers with --
     * so a status read here is the only thing that tells a refused token apart from a dead wire. */
    const int status = esp_http_client_get_status_code(session->client);

    *out_status = status;

    if (err != ESP_OK) {
        if (status == 401 || status == 403) {
            ESP_LOGW(TAG, "the account refused the token (%d)", status);
            esp_http_client_close(session->client);
            return HAL_WORKBUDDY_FAULT_AUTH;
        }

        if (status >= 400) {
            ESP_LOGW(TAG, "the account answered %d: %s", status, esp_err_to_name(err));
            esp_http_client_close(session->client);
            return HAL_WORKBUDDY_FAULT_HTTP;
        }

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

/**
 * One call with a body of its own, no retrying.
 *
 * `get-user-request-usage` is the only call here that takes a document rather than `{}`: a range of
 * time and a page. It is asked from the probe below, because a page of it is tens of kilobytes and
 * this board has no room to want many.
 */
static int32_t wb_fetch_body(const char *path, const char *token, const char *body, int *out_status)
{
    wb_session_t session;

    if (!wb_session_open(&session, s_account.host)) {
        return HAL_WORKBUDDY_FAULT_STORAGE;
    }

    const int32_t fault = wb_session_call(&session, path, token, HTTP_METHOD_POST, body, out_status);
    wb_session_close(&session);

    return fault;
}

static int32_t wb_fetch_retrying(const char *path, const char *token, int *out_status)
{
    wb_session_t session;

    if (!wb_session_open(&session, s_account.host)) {
        return HAL_WORKBUDDY_FAULT_STORAGE;
    }

    int32_t fault = HAL_WORKBUDDY_FAULT_NONE;

    for (int attempt = 1; attempt <= WB_TRIES; attempt++) {
        fault = wb_session_call(&session, path, token, HTTP_METHOD_POST, NULL, out_status);

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
 * What is left of the account's credits, and what it was given.
 *
 * `get-user-resource-summary` answers one entry per package -- the monthly plan and anything bought
 * on top of it -- and the App's own "剩余积分" is the sum of `CycleRemainCapacity` across them. So is
 * this: one number for "剩余" and one for "总额", which is what a tile has room for.
 *
 * Returns false when the call did not answer; the fields are then drawn as dashes rather than as
 * a zero, because "no answer" and "nothing left" are different sentences.
 */
static bool fetch_credits(double *out_remaining, double *out_total)
{
    int status = 0;
    const int32_t fault = wb_fetch_retrying(WB_SUMMARY_PATH, s_account.token, &status);

    if (fault != HAL_WORKBUDDY_FAULT_NONE || status < 200 || status >= 300) {
        ESP_LOGW(TAG, "the credit summary did not answer (%d)", status);
        return false;
    }

    const char *scope = account_scope(s_response);
    const char *packages = json_value(scope, "Packages");
    const char *end = array_end(packages);

    if (end == NULL) {
        ESP_LOGW(TAG, "the credit summary has no packages");
        return false;
    }

    /* 剩余就看 `TotalCredits`：接口自己把它算好了，重算一遍只是给自己多一个出错的机会。 */
    double remaining = 0.0;

    if (!json_decimal(json_value(scope, "TotalCredits"), &remaining)) {
        ESP_LOGW(TAG, "the credit summary has no TotalCredits");
        return false;
    }

    /* 总额是这个数组里各套餐的容量之和；数组之外还有一份同样的数额（`CreditGroups`，
     * 按种类拆开写），所以这里必须停在 `]` 上，否则每个数都会翻倍。 */
    double total = 0.0;
    int seen = 0;
    const char *at = packages;

    while (at < end) {
        const char *whole = json_value(at, "CycleTotalCapacity");

        if (whole == NULL || whole >= end) {
            break;
        }

        double given = 0.0;

        if (json_decimal(whole, &given)) {
            total += given;
            seen++;
        }

        at = whole + 1;
    }

    if (seen == 0) {
        ESP_LOGW(TAG, "the credit summary has no capacity this file can read");
        return false;
    }

    *out_remaining = remaining;
    *out_total = total;

    return true;
}

/**
 * How much the account has spent: how many kinds of resource it has, and how many uses this month.
 *
 * `get-user-resource` is the same document the App's usage page is drawn from. `TotalDosage` is
 * "uses", and it lags by two or three hours on the API's own side -- which is why the tile says
 * 「本月」 and not 「今天」.
 */
typedef struct {
    long long kinds;   /** 有几种资源 */
    long long uses;    /** 这个月用了多少次 */
    long long live;    /** 还有余额的积分包有几个 */
    char expire[16];   /** 最近一份到期日，`MM-DD` */
    double expire_credit; /** 那一天到期多少分 */
} wb_usage_t;

static bool fetch_usage(wb_usage_t *out)
{
    int status = 0;
    const int32_t fault = wb_fetch_retrying(WB_USAGE_PATH, s_account.token, &status);

    if (fault != HAL_WORKBUDDY_FAULT_NONE || status < 200 || status >= 300) {
        ESP_LOGW(TAG, "the usage summary did not answer (%d)", status);
        return false;
    }

    const char *data = account_scope(s_response);
    const char *inner = json_object(json_value(data, "Response"));

    if (inner != NULL) {
        data = json_object(json_value(inner, "Data"));
        if (data == NULL) {
            return false;
        }
    }

    long long kinds = 0;
    long long uses = 0;

    if (!json_number(json_value(data, "TotalCount"), &kinds) ||
        !json_number(json_value(data, "TotalDosage"), &uses)) {
        return false;
    }

    out->kinds = kinds;
    out->uses = uses;

    /* 同一份文档里带着每个积分包的到期时间，顺手把「最近到期」挑出来：这是整页唯一一个会让人
     * 动手的数字（分要过期了，今天该多用一点），而它不需要第三个请求。 */
    const char *accounts = json_value(data, "Accounts");
    const char *end = array_end(accounts);

    /* 文档被截断时 `array_end` 找不到那个 `]`。截断不等于空：读到的那些对象照样算数，
     * 所以尽头是缓冲区的尽头。 */
    if (end == NULL && accounts != NULL) {
        end = s_response + strlen(s_response);
    }

    out->live = 0;
    out->expire[0] = '\0';
    out->expire_credit = 0.0;

    const char *at = accounts;

    while (at != NULL && end != NULL && at < end) {
        const char *remain_at = json_value(at, "CycleCapacityRemain");

        if (remain_at == NULL || remain_at >= end) {
            break;
        }

        double remain = 0.0;
        json_decimal(remain_at, &remain);

        if (remain > 0.0) {
            out->live++;

            const char *end_at = json_value(at, "CycleEndTime");
            char stamp[32];

            if (end_at != NULL && end_at < end && json_string(end_at, stamp, sizeof stamp) &&
                strlen(stamp) >= 10) {
                /* `2026-10-22 15:42:52` 这种串按字典序比就是按时间比。 */
                if (out->expire[0] == '\0' || strcmp(stamp, out->expire) < 0) {
                    snprintf(out->expire, sizeof out->expire, "%.5s", stamp + 5);
                    out->expire_credit = remain;
                }
            }
        }

        at = remain_at + 1;
    }

    return true;
}

/**
 * What today has cost so far, and how many requests it took.
 *
 * The billing document says one thing per request -- `credit`, `model`, `requestTime` -- and there is
 * no total anywhere: the number a person wants has to be summed here. Two consequences worth
 * knowing before reading the code:
 *
 *  - a page of this document is large, because each row also carries the prompt that produced it
 *    (which this file never copies anywhere: only the three fields above are read);
 *  - so it is asked in pages of `WB_TODAY_PAGE_SIZE`, at most `WB_TODAY_PAGES` of them, and it is
 *    cached for `WB_TODAY_FRESH_MS`. A refresh every thirty seconds must not re-download a day's
 *    worth of prompts every thirty seconds.
 */
typedef struct {
    long long requests;
    double credit;
    char top_model[28];
    double top_credit;
} wb_today_t;

/** How many request rows come back in one page. See the note above: they are big. */
#define WB_TODAY_PAGE_SIZE 50

/** How many pages a day is worth reading. Beyond this the number is a floor, not a total. */
#define WB_TODAY_PAGES 4

/** How long a day's total stays good enough to draw. */
#define WB_TODAY_FRESH_MS (5 * 60 * 1000)

static struct {
    bool valid;
    char day[12];
    int64_t at_ms;
    wb_today_t value;
} s_today;

/**
 * The day the account bills in, as `YYYY-MM-DD`.
 *
 * Beijing +08:00, and not the board's own timezone: the request times come back as
 * `2026-10-21 15:20:00` in the account's zone, so "today" has to be measured in that same one or the
 * window is eight hours off.
 */
static void today_text(char *out, size_t capacity)
{
    const time_t now = time(NULL) + 8 * 3600;
    struct tm tm;
    char stamp[32];

    if (capacity == 0) {
        return;
    }

    if (gmtime_r(&now, &tm) == NULL) {
        snprintf(stamp, sizeof stamp, "1970-01-01");
    } else {
        snprintf(stamp, sizeof stamp, "%04d-%02d-%02d", tm.tm_year + 1900, tm.tm_mon + 1,
                 tm.tm_mday);
    }

    /* 手抄而不是再 snprintf 一次：一个 `%s` 往运行时长度的缓冲里写，编译器的
     * -Wformat-truncation 会当成可能的截断而拒绝编译，而这里截断是有意的。 */
    const size_t length = strlen(stamp);
    const size_t take = (length < capacity - 1) ? length : capacity - 1;

    memcpy(out, stamp, take);
    out[take] = '\0';
}

/** Whether a model's name is one this file is willing to remember: ASCII, and short. */
static void keep_model_name(const char *from, char *out, size_t capacity)
{
    out[0] = '\0';

    if (from == NULL) {
        return;
    }

    size_t used = 0;

    for (const char *at = from; *at != '\0' && used + 1 < capacity; at++) {
        if (*at < 0x20 || (unsigned char)*at > 0x7E) {
            continue;
        }
        out[used++] = *at;
    }

    out[used] = '\0';
}

static bool fetch_today_usage(wb_today_t *out)
{
    char day[12];
    today_text(day, sizeof day);

    const int64_t now_ms = esp_timer_get_time() / 1000;

    lock();
    const bool fresh = s_today.valid && strcmp(s_today.day, day) == 0 &&
                       (now_ms - s_today.at_ms) < WB_TODAY_FRESH_MS;
    const wb_today_t cached = s_today.value;
    unlock();

    if (fresh) {
        *out = cached;
        return true;
    }

    long long requests = 0;
    double credit = 0.0;
    char top_model[28] = "";
    double top_credit = 0.0;
    long long total = -1;
    bool answered = false;

    for (int page = 1; page <= WB_TODAY_PAGES; page++) {
        char body[224];
        snprintf(body, sizeof body,
                 "{\"startTime\":\"%s 00:00:00\",\"endTime\":\"%s 23:59:59\","
                 "\"pageNum\":%d,\"pageSize\":%d}",
                 day, day, page, WB_TODAY_PAGE_SIZE);

        int status = 0;
        const int32_t fault =
            wb_fetch_body("/billing/meter/get-user-request-usage", s_account.token, body, &status);

        if (fault != HAL_WORKBUDDY_FAULT_NONE || status < 200 || status >= 300) {
            ESP_LOGW(TAG, "today's usage did not answer (%d)", status);
            break;
        }

        const char *scope = account_scope(s_response);
        long long rows_total = 0;

        if (total < 0) {
            json_number(json_value(scope, "total"), &total);
        }

        const char *rows = json_value(scope, "data");
        const char *end = array_end(rows);

        if (end == NULL && rows != NULL) {
            end = s_response + strlen(s_response);
        }

        if (rows == NULL || end == NULL) {
            break;
        }

        const char *at = rows;
        long long in_page = 0;

        while (at < end) {
            const char *credit_at = json_value(at, "credit");
            const char *model_at = json_value(at, "model");

            if (credit_at == NULL || credit_at >= end) {
                break;
            }

            double value = 0.0;
            char model[28] = "";
            char unquoted[48];

            json_decimal(credit_at, &value);

            if (json_string(model_at, unquoted, sizeof unquoted)) {
                keep_model_name(unquoted, model, sizeof model);
            }

            credit += value;
            requests++;
            in_page++;

            if (model[0] != '\0') {
                /* 一个模型一行不出怪事就够了：这不是排行榜，是「今天主要花在哪个模型上」。 */
                if (top_model[0] == '\0') {
                    snprintf(top_model, sizeof top_model, "%s", model);
                    top_credit = value;
                } else if (strcmp(model, top_model) == 0) {
                    top_credit += value;
                }
            }

            at = credit_at + 1;
        }

        answered = true;

        ESP_LOGI(TAG, "today page %d: %lld rows, %lld of %lld in total", page, in_page, requests,
                 rows_total);

        if (rows_total >= 0 && requests >= rows_total) {
            break;
        }
        if (in_page < WB_TODAY_PAGE_SIZE) {
            break;
        }
    }

    if (!answered) {
        return false;
    }

    out->requests = requests;
    out->credit = credit;
    snprintf(out->top_model, sizeof out->top_model, "%s", top_model);
    out->top_credit = top_credit;

    lock();
    s_today.valid = true;
    s_today.at_ms = now_ms;
    snprintf(s_today.day, sizeof s_today.day, "%s", day);
    s_today.value = *out;
    unlock();

    return true;
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

    /* 名字要是坏字节就一个字也不发：应用画它自己的「今日签到」，网页少一行，两边都还是能看的
     * 东西 —— 一串问号不是。 */
    if (activity[0] != '\0' && !utf8_ok(activity)) {
        ESP_LOGW(TAG, "the activity's name is not UTF-8 and is dropped");
        activity[0] = '\0';
    }

    /* 字体里没有的字画出来是方框，不是名字：留下画得出的那部分。 */
    keep_drawable(activity);

    const long long access_days  = days_until(jwt_expires_at(s_account.token));
    const long long refresh_days = s_account.refresh[0] != '\0'
                                       ? days_until(jwt_expires_at(s_account.refresh))
                                       : -1;

    /* 两张表各一次请求，各自一格缓冲：拼答案的 `snprintf` 只认活得比它久的字符串。 */
    char remaining_text[16] = "";
    char total_text[16] = "";

    double remaining = 0.0;
    double credit_total = 0.0;

    if (fetch_credits(&remaining, &credit_total)) {
        snprintf(remaining_text, sizeof remaining_text, "%.1f", remaining);
        snprintf(total_text, sizeof total_text, "%.1f", credit_total);
    }

    wb_usage_t usage = { 0 };
    const bool have_usage = fetch_usage(&usage);

    wb_today_t today = { 0 };
    const bool have_today = fetch_today_usage(&today);

    char today_requests_text[16] = "";
    char today_credit_text[16] = "";
    char today_model_text[40] = "";

    if (have_today) {
        snprintf(today_requests_text, sizeof today_requests_text, "%lld", today.requests);
        snprintf(today_credit_text, sizeof today_credit_text, "%.1f", today.credit);
        snprintf(today_model_text, sizeof today_model_text, "%s", today.top_model);
    }

    char kinds_text[16] = "";
    char uses_text[16] = "";
    char live_text[16] = "";
    char expire_text[16] = "";
    char expire_credit_text[16] = "";

    if (have_usage) {
        snprintf(kinds_text, sizeof kinds_text, "%lld", usage.kinds);
        snprintf(uses_text, sizeof uses_text, "%lld", usage.uses);
        snprintf(live_text, sizeof live_text, "%lld", usage.live);
        snprintf(expire_text, sizeof expire_text, "%s", usage.expire);

        if (usage.expire[0] != '\0') {
            snprintf(expire_credit_text, sizeof expire_credit_text, "%.1f", usage.expire_credit);
        }
    }

    const int made = snprintf(s_answer, WB_ANSWER_MAX,
                              "ok=1\n"
                              "checked_in=%s\n"
                              "streak=%lld\n"
                              "daily=%lld\n"
                              "activity_total=%lld\n"
                              "activity=%s\n"
                              "access_days=%lld\n"
                              "refresh_days=%lld\n"
                              "credit_remaining=%s\n"
                              "credit_total=%s\n"
                              "usage_kinds=%s\n"
                              "usage_calls=%s\n"
                              "packages=%s\n"
                              "expire=%s\n"
                              "expire_credit=%s\n"
                              "today_requests=%s\n"
                              "today_credit=%s\n"
                              "today_model=%s\n",
                              flag(checked_in), streak, daily, total, activity, access_days,
                              refresh_days, remaining_text, total_text, kinds_text, uses_text,
                              live_text, expire_text, expire_credit_text, today_requests_text,
                              today_credit_text, today_model_text);

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
// Signing in: the device-authorisation flow
// -----------------------------------------------------------------------------
// This is the answer to "nobody is going to type two kilobytes on that panel", and it is the
// App's own sign-in, watched from the outside:
//
//     POST /v2/plugin/auth/state?platform=workbuddy-ai  ->  {data:{state, authUrl}}
//     (a person opens authUrl and authorises -- the QR the App draws is that URL)
//     GET  /v2/plugin/auth/token?state=<state>          ->  11217 while waiting,
//                                                          {data:{accessToken, refreshToken}} after
//
// The URL is carried back to whoever asked, so it can be shown as a link or as a QR; the polling is
// a separate call because a person takes ten seconds or a minute, and no HTTP handler here is going
// to sit on that.
// =============================================================================

/** How long the URL may be. `authUrl` is a sign-in redirect with a state in it: a few hundred. */
#define WB_LOGIN_URL_MAX 768

/** The `state` that ties the two calls together. */
#define WB_LOGIN_STATE_MAX 128

/** The API's "still waiting for the person" code. */
#define WB_CODE_WAITING 11217

static struct {
    int32_t state;
    char state_token[WB_LOGIN_STATE_MAX];
    char url[WB_LOGIN_URL_MAX];
    char message[192];
} s_login = { .state = HAL_WORKBUDDY_LOGIN_IDLE };

/** The platforms this API has answered on, in the order they are tried. */
static const char *const WB_LOGIN_PLATFORMS[] = { "workbuddy-ai", "workbuddy", "codebuddy" };

/** The host a token is being asked for. The account's own, or the default. */
static const char *login_host(void)
{
    return (s_account.host[0] != '\0') ? s_account.host : WB_DEFAULT_HOST;
}

static void login_reset(int32_t state, const char *message)
{
    lock();
    s_login.state = state;
    snprintf(s_login.message, sizeof s_login.message, "%s", message != NULL ? message : "");
    if (state != HAL_WORKBUDDY_LOGIN_DONE) {
        s_login.url[0] = '\0';
    }
    unlock();
}

esp_err_t hal_workbuddy_login_start(void)
{
    if (hal_workbuddy_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    if (!network_is_up()) {
        login_reset(HAL_WORKBUDDY_LOGIN_FAILED, "network");
        return ESP_ERR_INVALID_STATE;
    }

    lock();
    if (s_snap.busy) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }
    s_snap.busy = true;
    unlock();

    esp_err_t result = ESP_ERR_NOT_FOUND;
    int last_status = 0;

    for (size_t i = 0; i < sizeof WB_LOGIN_PLATFORMS / sizeof WB_LOGIN_PLATFORMS[0]; i++) {
        wb_session_t session;

        if (!wb_session_open(&session, login_host())) {
            result = ESP_ERR_NO_MEM;
            break;
        }

        char path[96];
        snprintf(path, sizeof path, "/v2/plugin/auth/state?platform=%s", WB_LOGIN_PLATFORMS[i]);

        int status = 0;
        const int32_t fault = wb_session_call(&session, path, NULL, HTTP_METHOD_POST, NULL, &status);
        wb_session_close(&session);

        last_status = status;

        if (fault != HAL_WORKBUDDY_FAULT_NONE) {
            continue;
        }

        const char *data = account_scope(s_response);
        char state[WB_LOGIN_STATE_MAX];
        char url[WB_LOGIN_URL_MAX];

        if (!json_string(json_value(data, "state"), state, sizeof state) ||
            !json_string(json_value(data, "authUrl"), url, sizeof url)) {
            continue;
        }

        lock();
        snprintf(s_login.state_token, sizeof s_login.state_token, "%s", state);
        snprintf(s_login.url, sizeof s_login.url, "%s", url);
        s_login.state = HAL_WORKBUDDY_LOGIN_WAITING;
        s_login.message[0] = '\0';
        unlock();

        result = ESP_OK;
        break;
    }

    if (result != ESP_OK) {
        char message[64];
        snprintf(message, sizeof message, "auth/state answered %d", last_status);
        login_reset(HAL_WORKBUDDY_LOGIN_FAILED, message);
    }

    lock();
    s_snap.busy = false;
    unlock();

    return result;
}

esp_err_t hal_workbuddy_login_poll(void)
{
    if (hal_workbuddy_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();
    const int32_t state = s_login.state;
    char ticket[WB_LOGIN_STATE_MAX];
    snprintf(ticket, sizeof ticket, "%s", s_login.state_token);

    if (state != HAL_WORKBUDDY_LOGIN_WAITING) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }
    if (s_snap.busy) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }
    s_snap.busy = true;
    unlock();

    esp_err_t result = ESP_OK;
    wb_session_t session;

    if (!wb_session_open(&session, login_host())) {
        lock();
        s_snap.busy = false;
        unlock();
        return ESP_ERR_NO_MEM;
    }

    char path[WB_LOGIN_STATE_MAX + 32];
    snprintf(path, sizeof path, "/v2/plugin/auth/token?state=%s", ticket);

    int status = 0;
    const int32_t fault = wb_session_call(&session, path, NULL, HTTP_METHOD_GET, NULL, &status);
    wb_session_close(&session);

    if (fault != HAL_WORKBUDDY_FAULT_NONE) {
        /* 「没答上来」不是「登录失败」：人可能还在扫码，网也可能刚断了一下。下一次再问。 */
        lock();
        s_snap.busy = false;
        unlock();
        return ESP_ERR_TIMEOUT;
    }

    const char *data = account_scope(s_response);

    /* Static, because two kilobytes twice over is the whole of a worker's stack and the handshake
     * that happens above has to stand somewhere. Only one sign-in runs at a time -- `s_login_job_live`
     * in `board_web.c` is what makes that true -- so there is nobody to race with. */
    static char token[HAL_WORKBUDDY_TOKEN_MAX];
    static char refresh[HAL_WORKBUDDY_REFRESH_MAX];

    if (!json_string(json_value(data, "accessToken"), token, sizeof token)) {
        long long code = -1;
        json_number(json_value(s_response, "code"), &code);

        if (code == WB_CODE_WAITING || code == WB_CODE_OK || code == -1) {
            lock();
            s_snap.busy = false;
            unlock();
            return ESP_ERR_NOT_FINISHED;
        }

        char message[160];
        if (!json_string(json_value(s_response, "msg"), message, sizeof message)) {
            snprintf(message, sizeof message, "code %lld", code);
        }

        login_reset(HAL_WORKBUDDY_LOGIN_FAILED, message);
        result = ESP_ERR_INVALID_RESPONSE;
    } else {
        if (!json_string(json_value(data, "refreshToken"), refresh, sizeof refresh)) {
            refresh[0] = '\0';
        }

        lock();
        memset(&s_account, 0, sizeof s_account);
        s_account.used = true;
        snprintf(s_account.token, sizeof s_account.token, "%s", token);
        snprintf(s_account.refresh, sizeof s_account.refresh, "%s", refresh);
        snprintf(s_account.host, sizeof s_account.host, "%s", login_host());
        unlock();

        const esp_err_t stored = wb_store();

        login_reset(HAL_WORKBUDDY_LOGIN_DONE,
                    stored == ESP_OK ? "" : esp_err_to_name(stored));
        ESP_LOGI(TAG, "signed in; the token is %s", stored == ESP_OK ? "saved" : "not saved");

        result = stored;
    }

    lock();
    s_snap.busy = false;
    unlock();

    return result;
}

int32_t hal_workbuddy_login_state(void)
{
    lock();
    const int32_t state = s_login.state;
    unlock();

    return state;
}

size_t hal_workbuddy_login_url(char *out, size_t capacity)
{
    if (out == NULL || capacity == 0) {
        return 0;
    }

    lock();
    const size_t length = strlen(s_login.url);
    const size_t take = (length < capacity) ? length : capacity - 1;
    memcpy(out, s_login.url, take);
    unlock();

    out[take] = '\0';

    return take;
}

size_t hal_workbuddy_login_message(char *out, size_t capacity)
{
    if (out == NULL || capacity == 0) {
        return 0;
    }

    lock();
    const size_t length = strlen(s_login.message);
    const size_t take = (length < capacity) ? length : capacity - 1;
    memcpy(out, s_login.message, take);
    unlock();

    out[take] = '\0';

    return take;
}

void hal_workbuddy_login_cancel(void)
{
    login_reset(HAL_WORKBUDDY_LOGIN_IDLE, "");
}

// =============================================================================
// What the account actually said
// =============================================================================
// One function, and it exists for the two fields this file had to guess at: the credit summary and
// the usage document are written down from somebody else's client, and a field name that is wrong
// looks exactly like an account with nothing in it. `hal_workbuddy_probe` answers the raw body so
// the guess can be checked instead of believed.
// =============================================================================

/**
 * 删掉一个字符串字段和它的值。
 *
 * 逐请求流水里每条都带着整段提示词（`input` 是全文，`inputTrunc` 是前几百字）。那是用户自己的
 * 东西，而这个页面在局域网上没有口令 —— 探针要的是形状，不是别人的对话。
 */
static void drop_field(char *text, const char *key)
{
    const size_t key_len = strlen(key);
    char *at = text;

    while ((at = strstr(at, key)) != NULL) {
        char *colon = strchr(at + key_len, ':');

        if (colon == NULL) {
            return;
        }

        char *value = colon + 1;
        while (*value == ' ' || *value == '\t') {
            value++;
        }

        if (*value != '"') {
            at += key_len;
            continue;
        }

        char *end = value + 1;
        while (*end != '\0' && *end != '"') {
            if (*end == '\\' && end[1] != '\0') {
                end++;
            }
            end++;
        }

        if (*end == '"') {
            end++;
        }
        if (*end == ',') {
            end++;
        }

        memmove(at, end, strlen(end) + 1);
    }
}

esp_err_t hal_workbuddy_probe_requests(const char *day, int32_t page_size, int32_t page,
                                       char *out, size_t capacity, size_t *out_length)
{
    if (out == NULL || capacity == 0 || day == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    out[0] = '\0';

    if (out_length != NULL) {
        *out_length = 0;
    }

    if (hal_workbuddy_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();
    const bool have_token = s_account.used && s_account.token[0] != '\0';
    unlock();

    if (!have_token) {
        return ESP_ERR_NOT_FOUND;
    }

    if (page_size <= 0) {
        page_size = 3;
    }
    if (page <= 0) {
        page = 1;
    }

    char body[192];
    snprintf(body, sizeof body,
             "{\"startTime\":\"%s 00:00:00\",\"endTime\":\"%s 23:59:59\","
             "\"pageNum\":%d,\"pageSize\":%d}",
             day, day, (int)page, (int)page_size);

    int status = 0;
    const int32_t fault =
        wb_fetch_body("/billing/meter/get-user-request-usage", s_account.token, body, &status);

    if (fault != HAL_WORKBUDDY_FAULT_NONE) {
        return ESP_FAIL;
    }

    const size_t length = strlen(s_response);
    const size_t take = (length < capacity - 1) ? length : capacity - 1;

    memcpy(out, s_response, take);
    out[take] = '\0';

    /* 提示词不属于这条流水该被看见的部分。 */
    drop_field(out, "\"inputTrunc\"");
    drop_field(out, "\"input\"");
    drop_field(out, "\"agentPurpose\"");

    if (out_length != NULL) {
        *out_length = strlen(out);
    }

    return ESP_OK;
}

esp_err_t hal_workbuddy_probe(int32_t which, char *out, size_t capacity, size_t *out_length)
{
    if (out == NULL || capacity == 0) {
        return ESP_ERR_INVALID_ARG;
    }

    out[0] = '\0';

    if (out_length != NULL) {
        *out_length = 0;
    }

    if (hal_workbuddy_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    const char *path = NULL;
    switch (which) {
    case 0: path = WB_STATUS_PATH; break;
    case 1: path = WB_SUMMARY_PATH; break;
    case 2: path = WB_USAGE_PATH; break;
    default: return ESP_ERR_INVALID_ARG;
    }

    lock();
    const bool have_token = s_account.used && s_account.token[0] != '\0';
    unlock();

    if (!have_token) {
        return ESP_ERR_NOT_FOUND;
    }

    int status = 0;
    const int32_t fault = wb_fetch_retrying(path, s_account.token, &status);

    if (fault != HAL_WORKBUDDY_FAULT_NONE) {
        return ESP_FAIL;
    }

    const size_t length = strlen(s_response);
    const size_t take = (length < capacity - 1) ? length : capacity - 1;

    memcpy(out, s_response, take);
    out[take] = '\0';

    if (out_length != NULL) {
        *out_length = take;
    }

    return ESP_OK;
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
