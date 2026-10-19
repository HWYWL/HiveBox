// =============================================================================
// Cloud servers (Tencent Cloud)
// -----------------------------------------------------------------------------
// "Turn my server off from the bed." The board holds two cloud credentials and
// can list what they own, stop it, start it and reboot it. `hal_cloud_start`
// names a provider and an action and returns; a worker does the signing, the
// handshake and the read, and the answer is *taken* rather than read twice --
// the same shape as `board_http.c`, and for the same reason: this is called
// from a frame loop and from an HTTP handler, and neither can wait three
// seconds for a TLS handshake.
//
// -----------------------------------------------------------------------------
// Why this is not `board_http.c`
// -----------------------------------------------------------------------------
// The other client is a *fetch*: a URL goes in and bytes come out, which is what
// both its callers want (a weather page, a firmware image). This one is not --
// a Tencent Cloud call is a POST with a JSON body and five headers of its own,
// and the board's `HttpBackend` is deliberately GET-only (`pomelo-hal`'s
// `traits/http.rs` says so out loud). Widening that trait for one caller would
// put a body and a header list in a parameter every implementation has to look
// at to find out it is empty, so this file owns its own `esp_http_client`
// instead, and the Rust side reaches it through `hal_cloud_*` rather than
// through `board.http()`.
//
// -----------------------------------------------------------------------------
// The signing (TC3-HMAC-SHA256)
// -----------------------------------------------------------------------------
// Tencent's scheme, and the whole of it: hash the body, build a canonical
// request out of the method, the path, the query, the two headers that matter
// and that body hash, then sign that with a key derived from the SecretKey
// through four HMACs. The result is an `Authorization` header and no session:
// there is nothing to keep, nothing to refresh and nothing to log in to, which
// is the reason a board with four sockets can do this at all.
//
// It is done here, in C, against mbedTLS (both SHA-256 and HMAC are already on
// for the TLS stack, and the chip has an SHA accelerator behind them). The
// alternative was a pure-Rust signer -- better for testing, since Tencent
// publishes worked examples to pin it against -- but it would have meant a new
// crate in the image and a new trait, a new sim and a new FFI surface for
// something only two callers in this tree ever ask for. mbedTLS was already
// paid for.
//
// Two things have to be true before this works at all, and both are refused
// here rather than dressed up as "connection failed":
//
//   * The board must be on a network. Signing needs no network; sending does.
//   * The clock must have been set. The signature carries a timestamp and
//     Tencent's server refuses one more than five minutes out of step, so a
//     board reading 1970 gets `AuthFailure.SignatureExpire` on every call.
//     `hal_time_sync()` is what fixes that.
//
// -----------------------------------------------------------------------------
// The credentials
// -----------------------------------------------------------------------------
// One plaintext TOML file, `/internal/AppData/CLOUD/cloud.conf`, with a section
// per provider:
//
//     [cvm]
//     region = "ap-guangzhou"
//     secret_id = "AKID..."
//     secret_key = "..."
//
// Plaintext, like the Wi-Fi password and the NAS logins, and for the same
// reason: this board has no secure element and no key store, and a key that has
// to be retyped after every flash is a key nobody keeps. What that costs is
// written down where a person will see it -- on the page that asks for one (see
// `web_page.html`) -- rather than hidden behind a lock that is not there.
//
// Nothing here hands a SecretKey back out. `hal_cloud_credentials_get` answers
// with a masked SecretId and the region, which is everything a page needs to
// draw "one is saved" without the page ever holding the key again.
// =============================================================================

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
/* `gmtime_r` and `strftime`: the signature carries a timestamp and Tencent's scheme wants the date
 * it falls on, in UTC, as one of the derived keys. */
#include <time.h>

#include "esp_crt_bundle.h"
#include "esp_err.h"
#include "esp_heap_caps.h"
#include "esp_http_client.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"

#include "mbedtls/md.h"

#include "board_hal.h"

static const char *TAG = "board_cloud";

/** Where the two keys live. Plaintext, and the whole of the reason is at the top of this file. */
static const char *CLOUD_CONF_PATH = "/internal/AppData/CLOUD/cloud.conf";

/** How long the whole call may take: resolve, connect, handshake, POST, answer. The handshake is
 * most of it -- Tencent's API is one host and a small JSON document. */
#define CLOUD_TIMEOUT_MS 15000

/** The worker's stack. The deep part is the handshake; see `board_http.c`, which pays the same. */
#define CLOUD_TASK_STACK 8192

/** The socket's read buffer, and the one headers and the request body are written through. */
#define CLOUD_IO_CHUNK 2048

/** The most instances one answer may list. Twenty is a page, and more would be a buffer nobody
 * scrolls to the bottom of. */
#define CLOUD_MAX_INSTANCES 20

/** What one API answer may be. A describe of twenty instances is about four kilobytes; the rest is
 * headroom for the fields this file ignores. */
#define CLOUD_RESPONSE_MAX 8192

/** What the rendered answer may be. Larger than the response it is made from, because the summary of
 * an instance is shorter than the instance -- and the bound is what keeps a page from being handed a
 * document that was cut in half by a buffer. */
#define CLOUD_ANSWER_MAX 6144

/** How long an `Authorization` header this file will build. The parts are a 36-character SecretId, a
 * date, a service name, and 64 hex characters of signature. */
#define CLOUD_AUTH_MAX 256

/** How many times one call is tried, and only when the wire is what failed. Two is enough to turn
 * the odd answer that never arrives into a wait, and a second 15-second timeout on a network that is
 * down is worse than saying so. */
#define CLOUD_TRIES 2

/**
 * The data centres a list is gathered from.
 *
 * Both APIs are regional: `DescribeInstances` answers about *one* data centre per call, which is
 * why a machine in Tokyo never appeared while only the key's own `ap-guangzhou` was asked. "All my
 * servers" therefore means one call per region, and this is the list of them.
 *
 * The two products spell some of the same places differently (`ap-siliconvalley` is Lighthouse's,
 * `na-siliconvalley` is CVM's), so each has its own list. A region an account has not opened
 * answers with an `Error` object and is skipped, so a name that is not sold there costs one call
 * and no rows, not a failure.
 */
static const char *const CLOUD_REGIONS_CVM[] = {
    "ap-beijing",      "ap-chengdu",  "ap-chongqing",   "ap-guangzhou", "ap-hongkong",
    "ap-jakarta",      "ap-nanjing",  "ap-seoul",       "ap-shanghai",  "ap-singapore",
    "ap-tokyo",        "ap-bangkok",  "ap-mumbai",      "ap-taipei",    "ap-shenzhen",
    "na-ashburn",      "na-siliconvalley", "na-toronto", "eu-frankfurt", "eu-moscow",
    "sa-saopaulo",
};

static const char *const CLOUD_REGIONS_LIGHTHOUSE[] = {
    "ap-beijing",  "ap-chengdu", "ap-chongqing", "ap-guangzhou", "ap-hongkong",
    "ap-jakarta",  "ap-nanjing", "ap-seoul",     "ap-shanghai",  "ap-singapore",
    "ap-tokyo",    "ap-bangkok", "ap-frankfurt", "ap-moscow",    "ap-siliconvalley",
    "ap-saopaulo",
};

/** The regions of one provider, and how many. */
static void cloud_regions(int32_t provider, const char *const **out_list, size_t *out_count)
{
    if (provider == HAL_CLOUD_LIGHTHOUSE) {
        *out_list  = CLOUD_REGIONS_LIGHTHOUSE;
        *out_count = sizeof CLOUD_REGIONS_LIGHTHOUSE / sizeof CLOUD_REGIONS_LIGHTHOUSE[0];
    } else {
        *out_list  = CLOUD_REGIONS_CVM;
        *out_count = sizeof CLOUD_REGIONS_CVM / sizeof CLOUD_REGIONS_CVM[0];
    }
}

/** How long a whole merged list may scan for. Two products over every region is a minute of calls on
 * a bad day; past this the rows that did arrive are the answer and the rest are named as failed. */
#define CLOUD_SCAN_BUDGET_MS 45000

// =============================================================================
// State
// =============================================================================

/**
 * One provider's key, as it is on file and as the worker uses it.
 *
 * `used` is whether there is one at all: a board that has been flashed and never told a key has a
 * zeroed table, and this is how the difference between "no key" and "a key whose SecretId happens to
 * be empty" is kept -- the second is refused at the door, so in practice `used` means the table's
 * entry for this provider has been filled in.
 */
typedef struct {
    bool used;
    char region[HAL_CLOUD_REGION_MAX];
    char secret_id[HAL_CLOUD_SECRET_ID_MAX];
    char secret_key[HAL_CLOUD_SECRET_KEY_MAX];
} cloud_account_t;

typedef struct {
    int32_t state;
    int32_t fault;
    /** Length of the answer in `s_answer`, meaningful while the state is DONE. */
    size_t length;
    /** A worker task is alive. See `board_http.c`: a cancel clears the state while the worker is
     * still closing its socket, and the slot is not free until that is done. */
    bool busy;
    /** Asked to stop since the worker started. The worker looks before it sends. */
    bool cancelled;
} cloud_snapshot_t;

static cloud_snapshot_t s_snap = { .state = HAL_CLOUD_IDLE };

static SemaphoreHandle_t s_lock = NULL;

/** The rendered answer, out of PSRAM by preference: the largest thing this file owns and nothing
 * needs it fast. */
static char *s_answer = NULL;

/** The raw body of the last answer, so the worker has somewhere to read into. Allocated beside
 * `s_answer` and with the same lifetime. */
static char *s_response = NULL;

/** What the worker is doing. Written under the lock, read by the worker while `busy`. */
static int32_t s_provider = HAL_CLOUD_CVM;
static int32_t s_action   = HAL_CLOUD_ACTION_LIST;
static char    s_region[HAL_CLOUD_REGION_MAX];
static char    s_instance[HAL_CLOUD_INSTANCE_MAX];

/** The two keys, as loaded from the file. */
static cloud_account_t s_accounts[HAL_CLOUD_PROVIDER_COUNT];

/** Whether the table above has been read from the file yet. Loading is deferred to the first call
 * that needs it, because the filesystem is not up when this file is compiled and may not be up when
 * the first request is made. */
static bool s_loaded = false;

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

/** Whether the board is on a network. Wi-Fi is the only one this board has. */
static bool network_is_up(void)
{
    hal_wifi_status_t status;
    if (hal_wifi_get_status(&status) != ESP_OK) {
        return false;
    }
    return status.state == HAL_WIFI_STATE_CONNECTED;
}

/** Whether the board knows what time it is. See the note at the top of this file. */
static bool clock_is_set(void)
{
    int64_t epoch = 0;
    return hal_system_get_epoch(&epoch) == ESP_OK;
}

// =============================================================================
// The primitives the signature is built out of
// =============================================================================

static void hex_encode(const uint8_t *in, size_t length, char *out)
{
    static const char digits[] = "0123456789abcdef";

    for (size_t i = 0; i < length; i++) {
        out[i * 2]     = digits[(in[i] >> 4) & 0x0F];
        out[i * 2 + 1] = digits[in[i] & 0x0F];
    }

    out[length * 2] = '\0';
}

/** The SHA-256 of a run of bytes, as 64 lowercase hex characters. */
static bool sha256_hex(const char *data, size_t length, char *out)
{
    uint8_t digest[32];

    const mbedtls_md_info_t *info = mbedtls_md_info_from_type(MBEDTLS_MD_SHA256);
    if (info == NULL) {
        return false;
    }
    if (mbedtls_md(info, (const unsigned char *)data, length, digest) != 0) {
        return false;
    }

    hex_encode(digest, sizeof digest, out);

    return true;
}

/** The HMAC-SHA-256 of a run of bytes, as 32 raw bytes. The key is raw and not hex: the four keys in
 * the derivation below are hashes and one string, and neither is text. */
static bool hmac_sha256(const uint8_t *key, size_t key_length, const char *data, size_t length,
                        uint8_t *out)
{
    const mbedtls_md_info_t *info = mbedtls_md_info_from_type(MBEDTLS_MD_SHA256);
    if (info == NULL) {
        return false;
    }

    return mbedtls_md_hmac(info, key, key_length, (const unsigned char *)data, length, out) == 0;
}

// =============================================================================
// The signature
// =============================================================================

/**
 * The TC3-HMAC-SHA256 signature of one request, ready to be a header.
 *
 * The canonical request is written out by hand rather than assembled from parts, because the parts
 * are all constants: the method is always POST, the path is always `/`, and the query is always
 * empty -- everything the API wants is in the body or in a header this file sets. The only two
 * headers in the signature are `content-type` and `host`, and both are fixed by the endpoint. So the
 * one thing that varies is the body hash, and that is the last line.
 *
 * The whitespace is load-bearing: a `\n` between every part, and the blank line between the headers
 * and the signed-header list is two of them in a row. It is exactly what Tencent's documentation
 * shows, and a signature built from anything else is refused with `AuthFailure.SignatureFailure` --
 * a failure that says nothing about which newline was wrong.
 *
 * @param service   `cvm` or `lighthouse` -- the API family, and part of the key derivation.
 * @param host      The endpoint's hostname, which is also the value of the `host` line.
 * @param timestamp Seconds since the epoch; the UTC date is derived from it and both go in.
 * @param body      The request body; only its hash enters, but the hash is of the whole of it.
 */
static bool cloud_authorize(const char *service, const char *host, const cloud_account_t *account,
                            int64_t timestamp, const char *body, char *out, size_t capacity)
{
    bool ok = false;
    char *canonical = NULL;
    char *to_sign = NULL;

    const time_t stamp = (time_t)timestamp;
    struct tm utc;
    if (gmtime_r(&stamp, &utc) == NULL) {
        return false;
    }

    char date[16];
    if (strftime(date, sizeof date, "%Y-%m-%d", &utc) == 0) {
        return false;
    }

    /* The body's hash: the last line of the canonical request. */
    char payload_hash[65];
    if (!sha256_hex(body, strlen(body), payload_hash)) {
        return false;
    }

    size_t length = strlen(host) + strlen(payload_hash) + 256;
    canonical = (char *)malloc(length);
    if (canonical == NULL) {
        goto out;
    }

    int written = snprintf(canonical, length,
                           "POST\n"
                           "/\n"
                           "\n"
                           "content-type:application/json; charset=utf-8\n"
                           "host:%s\n"
                           "\n"
                           "content-type;host\n"
                           "%s",
                           host, payload_hash);
    if (written < 0 || (size_t)written >= length) {
        goto out;
    }

    char canonical_hash[65];
    if (!sha256_hex(canonical, (size_t)written, canonical_hash)) {
        goto out;
    }

    /* `<date>/<service>/tc3_request` is a fixed suffix with two variables in it. */
    length = strlen(date) + strlen(service) + strlen(canonical_hash) + 64;
    to_sign = (char *)malloc(length);
    if (to_sign == NULL) {
        goto out;
    }

    written = snprintf(to_sign, length, "TC3-HMAC-SHA256\n%lld\n%s/%s/tc3_request\n%s",
                       (long long)timestamp, date, service, canonical_hash);
    if (written < 0 || (size_t)written >= length) {
        goto out;
    }

    /* The four HMACs. Each one's *output* is the next one's key, and the last key signs the string
     * above: a SecretKey never signs anything directly, which is what makes the date and the service
     * part of the key rather than part of the message. */
    uint8_t secret_date[32], secret_service[32], secret_signing[32], signature[32];

    char key_date[128];
    written = snprintf(key_date, sizeof key_date, "TC3%s", account->secret_key);
    if (written < 0 || (size_t)written >= sizeof key_date) {
        goto out;
    }

    if (!hmac_sha256((const uint8_t *)key_date, strlen(key_date), date, strlen(date), secret_date)) {
        goto out;
    }
    if (!hmac_sha256(secret_date, sizeof secret_date, service, strlen(service), secret_service)) {
        goto out;
    }
    if (!hmac_sha256(secret_service, sizeof secret_service, "tc3_request", 11, secret_signing)) {
        goto out;
    }
    if (!hmac_sha256(secret_signing, sizeof secret_signing, to_sign, strlen(to_sign), signature)) {
        goto out;
    }

    char signature_hex[65];
    hex_encode(signature, sizeof signature, signature_hex);

    written = snprintf(out, capacity,
                       "TC3-HMAC-SHA256 Credential=%s/%s/%s/tc3_request, "
                       "SignedHeaders=content-type;host, Signature=%s",
                       account->secret_id, date, service, signature_hex);
    if (written < 0 || (size_t)written >= capacity) {
        goto out;
    }

    ok = true;

out:
    free(canonical);
    free(to_sign);

    return ok;
}

// =============================================================================
// Reading the answer
// -----------------------------------------------------------------------------
// Just enough of a JSON reader to walk one Tencent Cloud answer, and not a parser: this file knows
// the four keys it wants and the shape they are in, and the shape does not change between releases
// of the API. A general parser would be a general parser's worth of code in the image, and a table
// of structs to keep in step with a service this board uses three calls of.
//
// Every function here answers with a pointer into the text it was given -- nothing is copied and
// nothing is allocated -- so the answer has to outlive the walk, which it does: the walk and the
// render both happen inside the worker, with `s_response` untouched between them.
// =============================================================================

/**
 * The value that follows `"key"`, or NULL.
 *
 * `strstr` and not a scan, because the key is looked for with its quotes *and* the colon after it:
 * a key name appearing inside another key's value (`"InstanceName":"InstanceId"`) fails the colon
 * test and the search carries on, which is the one case that matters here -- Tencent echoes a name
 * back inside the same object it names.
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
 * A `\uXXXX` escape is turned into UTF-8 rather than dropped, because an instance somebody named in
 * Chinese comes back as either one or the other depending on which server answered, and a row with
 * the name missing is worse than a row with the name in it. A surrogate pair is beyond this: it
 * would need the second half, which means the copy would need to look ahead, and an emoji in an
 * instance name is not worth that.
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
                /* `\"`, `\\`, `\/` and anything unknown: the character itself. */
                code = (unsigned char)*value;
                value++;
                break;
            }
        } else {
            code = (unsigned char)*value;
            value++;
        }

        /* UTF-8, one to three bytes: everything above that is outside the basic plane and was
         * decoded as two zeroes above. */
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

/** The first string inside an array value, or NULL. `PublicIpAddresses` is `["1.2.3.4"]` and this is
 * the `"1.2.3.4"`. */
static const char *json_first_string(const char *value)
{
    if (value == NULL || *value != '[') {
        return NULL;
    }

    value++;
    while (*value != '\0' && *value != '"' && *value != ']') {
        value++;
    }

    return (*value == '"') ? value : NULL;
}

/**
 * The end of the object that starts at `at`, or NULL when it never closes.
 *
 * Braces are counted, and braces inside strings do not count: an instance named `"a}b"` is one
 * object and not two, and reading it as two would take the name for a field.
 */
static const char *json_object_end(const char *at)
{
    if (at == NULL || *at != '{') {
        return NULL;
    }

    int depth = 0;
    bool in_string = false;

    for (; *at != '\0'; at++) {
        if (in_string) {
            if (*at == '\\' && at[1] != '\0') {
                at++;
            } else if (*at == '"') {
                in_string = false;
            }
            continue;
        }

        if (*at == '"') {
            in_string = true;
        } else if (*at == '{') {
            depth++;
        } else if (*at == '}') {
            if (--depth == 0) {
                return at;
            }
        }
    }

    return NULL;
}

// =============================================================================
// Writing the answer
// -----------------------------------------------------------------------------
// The answer is JSON and it is written straight out for whoever asked: the C side knows what the
// request was and what came back, and handing that up as a document rather than as a struct means
// the Rust side and the page read the *same* thing, in the same words, with no second struct to
// keep in step and no marshalling at the FFI boundary.
//
// The writer is the bound: every append goes through one place that knows how much room is left, so
// a long instance name ends the document rather than the buffer. A document that ran off the end is
// reported as a failure and not sent -- half a JSON document parses as nothing at all, and a page
// that drew it would be drawing its own guess.
// =============================================================================

typedef struct {
    char  *out;
    size_t capacity;
    size_t used;
    bool   over;
} writer_t;

static void w_printf(writer_t *w, const char *format, ...) __attribute__((format(printf, 2, 3)));

static void w_printf(writer_t *w, const char *format, ...)
{
    if (w->over) {
        return;
    }
    if (w->used + 1 >= w->capacity) {
        w->over = true;
        return;
    }

    const int room = (int)(w->capacity - w->used);

    va_list args;
    va_start(args, format);
    const int made = vsnprintf(w->out + w->used, (size_t)room, format, args);
    va_end(args);

    if (made < 0 || made >= room) {
        /* `vsnprintf` truncated, and what is in the buffer now is not the document anybody asked
         * for. `over` is what turns that into a failure instead of an answer. */
        w->over = true;
        w->out[w->capacity - 1] = '\0';
        return;
    }

    w->used += (size_t)made;
}

/** A string appended with its escapes. The text is UTF-8 already and goes through a byte at a time:
 * re-encoding it here would be this file's chance to get it wrong. */
static void w_string(writer_t *w, const char *text)
{
    w_printf(w, "\"");

    for (const unsigned char *at = (const unsigned char *)text; *at != '\0' && !w->over; at++) {
        switch (*at) {
        case '"':  w_printf(w, "\\\""); break;
        case '\\': w_printf(w, "\\\\"); break;
        case '\n': w_printf(w, "\\n"); break;
        case '\r': w_printf(w, "\\r"); break;
        case '\t': w_printf(w, "\\t"); break;
        default:
            if (*at < 0x20) {
                w_printf(w, "\\u%04x", (unsigned)*at);
            } else {
                w_printf(w, "%c", (char)*at);
            }
            break;
        }
    }

    w_printf(w, "\"");
}

/** The provider's name as everything outside this file spells it: the page, the file, the log. */
static const char *provider_name(int32_t provider)
{
    if (provider == HAL_CLOUD_ALL) {
        return "all";
    }

    return (provider == HAL_CLOUD_LIGHTHOUSE) ? "lighthouse" : "cvm";
}

/** The action as everything outside this file spells it. */
static const char *action_name(int32_t action)
{
    switch (action) {
    case HAL_CLOUD_ACTION_LIST:   return "list";
    case HAL_CLOUD_ACTION_START:  return "start";
    case HAL_CLOUD_ACTION_STOP:   return "stop";
    case HAL_CLOUD_ACTION_REBOOT: return "reboot";
    default:                      return "list";
    }
}

/** Record what the answer came to, and let go of the slot. */
static void settle(int32_t fault, size_t length)
{
    lock();
    if (s_snap.cancelled) {
        /* Somebody let go while this was on the wire. The answer is real and nobody is waiting for
         * it -- and the slot goes back to *idle* rather than to DONE, so the next caller does not
         * pick up an answer to a question it did not ask. */
        s_snap.state  = HAL_CLOUD_IDLE;
        s_snap.fault  = HAL_CLOUD_FAULT_NONE;
        s_snap.length = 0;
    } else {
        s_snap.fault  = fault;
        s_snap.length = length;
        s_snap.state  = (fault == HAL_CLOUD_FAULT_NONE) ? HAL_CLOUD_DONE : HAL_CLOUD_FAILED;
    }
    s_snap.busy = false;
    unlock();
}

/**
 * The answer to a request that never left the board, or one whose answer made no sense.
 *
 * The code is the fault's own name as the page spells it, and the page has a sentence for each --
 * which is the whole reason the faults are a closed list here rather than an `esp_err_t` handed up
 * for somebody else to interpret.
 */
static void answer_fault(int32_t fault)
{
    static const char *const codes[] = {
        [HAL_CLOUD_FAULT_NONE]        = "cloud_failed",
        [HAL_CLOUD_FAULT_ARGS]        = "cloud_args",
        [HAL_CLOUD_FAULT_NO_CREDS]    = "cloud_creds",
        [HAL_CLOUD_FAULT_NO_NETWORK]  = "cloud_offline",
        [HAL_CLOUD_FAULT_CLOCK]       = "cloud_clock",
        [HAL_CLOUD_FAULT_UNREACHABLE] = "cloud_unreachable",
        [HAL_CLOUD_FAULT_TLS]         = "cloud_tls",
        [HAL_CLOUD_FAULT_HTTP]        = "cloud_http",
        [HAL_CLOUD_FAULT_API]         = "cloud_api",
        [HAL_CLOUD_FAULT_STORAGE]     = "cloud_storage",
    };

    const char *code = "cloud_failed";

    if (fault >= 0 && (size_t)fault < sizeof codes / sizeof codes[0] && codes[fault] != NULL) {
        code = codes[fault];
    }

    writer_t w = { .out = s_answer, .capacity = CLOUD_ANSWER_MAX };
    w_printf(&w, "{\"error\":\"%s\"}", code);

    settle(fault, w.over ? 0 : w.used);
}

/** The API's own refusal: `{"Response":{"Error":{"Code":"...","Message":"..."}}}`. Both halves are
 * kept, because the code is what a person searches for and the message is what they read. */
static void answer_api_error(const char *code, const char *message)
{
    writer_t w = { .out = s_answer, .capacity = CLOUD_ANSWER_MAX };

    w_printf(&w, "{\"error\":\"cloud_api\",\"code\":");
    w_string(&w, code);
    w_printf(&w, ",\"message\":");
    w_string(&w, message);
    w_printf(&w, "}");

    settle(HAL_CLOUD_FAULT_API, w.over ? 0 : w.used);
}

/** Whether the answer was the API refusing, and if so, what it said.
 *
 * `code` is what a person searches for and `message` is what they read, so both are carried out --
 * but nothing is written here. A merged list has to be able to note one provider's refusal and still
 * draw the rows the other one brought back.
 */
static bool body_is_refusal(const char *response, char *code, size_t code_size, char *message,
                            size_t message_size)
{
    const char *error = json_value(response, "Error");
    if (error == NULL || *error != '{') {
        return false;
    }

    code[0] = '\0';
    message[0] = '\0';

    json_string(json_value(error, "Code"), code, code_size);
    json_string(json_value(error, "Message"), message, message_size);

    if (code[0] == '\0') {
        snprintf(code, code_size, "UnknownError");
    }
    if (message[0] == '\0') {
        snprintf(message, message_size, "接口返回了一个错误，但没有说是什么");
    }

    ESP_LOGW(TAG, "the API refused: %s -- %s", code, message);

    return true;
}

/**
 * Append one `DescribeInstances` answer's rows to the answer being built.
 *
 * The two providers put the same things under different names -- CVM says `PublicIpAddresses` and
 * Lighthouse says `PublicAddresses`; Lighthouse repeats the region in every instance and CVM keeps
 * it in the request -- so each field is looked for under both names. What comes out is the same
 * shape either way, which is what lets one page draw both, and `kind` says which API a row came
 * from: a merged list of two APIs has buttons, and a button has to know where to send the call.
 *
 * Answers false when the body has no `InstanceSet` at all: that is not "no instances", it is an
 * answer this file cannot read, and a page needs to hear the difference.
 */
static bool append_instances(writer_t *w, const char *response, int32_t provider,
                             const char *region, int *listed)
{
    const char *set = json_value(response, "InstanceSet");
    if (set == NULL || *set != '[') {
        return false;
    }

    const char *at = set + 1;

    while (*at != '\0' && *listed < CLOUD_MAX_INSTANCES) {
        while (*at != '\0' && *at != '{' && *at != ']') {
            at++;
        }
        if (*at != '{') {
            break;
        }

        const char *end = json_object_end(at);
        if (end == NULL) {
            break;
        }

        /* Each field is read out of the object, and the walk continues from its end: the object is
         * one region of the response and the next one starts after it. */
        char id[HAL_CLOUD_INSTANCE_MAX];
        char name[HAL_CLOUD_NAME_MAX];
        char state[24];
        char ip[64];

        id[0] = '\0';
        name[0] = '\0';
        state[0] = '\0';
        ip[0] = '\0';

        json_string(json_value(at, "InstanceId"), id, sizeof id);
        if (id[0] == '\0') {
            /* An object with no id is not an instance: there is nothing to draw and nothing for a
             * button to name. */
            at = end + 1;
            continue;
        }

        json_string(json_value(at, "InstanceName"), name, sizeof name);
        json_string(json_value(at, "InstanceState"), state, sizeof state);

        /* CVM's public addresses first, then Lighthouse's, then the private ones: an instance with
         * no public address still has one worth showing, and an empty string is what a row shows
         * when it has neither. */
        const char *address = json_first_string(json_value(at, "PublicIpAddresses"));
        if (address == NULL) {
            address = json_first_string(json_value(at, "PublicAddresses"));
        }
        if (address == NULL) {
            address = json_first_string(json_value(at, "PrivateIpAddresses"));
        }
        if (address == NULL) {
            address = json_first_string(json_value(at, "PrivateAddresses"));
        }
        if (address != NULL) {
            json_string(address, ip, sizeof ip);
        }

        if (*listed > 0) {
            w_printf(w, ",");
        }

        w_printf(w, "{\"id\":");
        w_string(w, id);
        w_printf(w, ",\"name\":");
        w_string(w, name);
        w_printf(w, ",\"state\":");
        w_string(w, state);
        w_printf(w, ",\"ip\":");
        w_string(w, ip);
        /* The region is on the row because a power command needs it: the same account's machines
         * are in more than one data centre now, and `StartInstances` is addressed to one of them. */
        w_printf(w, ",\"kind\":\"%s\",\"region\":", provider_name(provider));
        w_string(w, region);
        w_printf(w, "}");

        (*listed)++;
        at = end + 1;
    }

    return true;
}

/** The list of one provider, as a page draws it. */
static void render_instances(const char *response, int32_t provider, const char *region)
{
    writer_t w = { .out = s_answer, .capacity = CLOUD_ANSWER_MAX };
    int listed = 0;

    w_printf(&w, "{\"ok\":1,\"provider\":\"%s\",\"region\":", provider_name(provider));
    w_string(&w, region);
    w_printf(&w, ",\"instances\":[");

    if (!append_instances(&w, response, provider, region, &listed)) {
        answer_api_error("MalformedAnswer", "接口没有返回实例列表");
        return;
    }

    w_printf(&w, "],\"count\":%d}", listed);

    if (w.over) {
        /* Not "some of the instances": a page that drew nine of ten would be a page somebody acts on
         * with the wrong picture of what they own. */
        ESP_LOGW(TAG, "the answer for %d instances did not fit in %d bytes", listed, CLOUD_ANSWER_MAX);
        answer_fault(HAL_CLOUD_FAULT_STORAGE);
        return;
    }

    settle(HAL_CLOUD_FAULT_NONE, w.used);
}

/** The answer to a start, a stop or a reboot. There is no instance to draw -- the page asks for the
 * list again once the machine has had a moment -- so this is the request ID and nothing else. */
static void render_accepted(const char *response, int32_t provider, int32_t action)
{
    char request_id[64];
    request_id[0] = '\0';
    json_string(json_value(response, "RequestId"), request_id, sizeof request_id);

    writer_t w = { .out = s_answer, .capacity = CLOUD_ANSWER_MAX };

    w_printf(&w, "{\"ok\":1,\"provider\":\"%s\",\"action\":\"%s\",\"request_id\":",
             provider_name(provider), action_name(action));
    w_string(&w, request_id);
    w_printf(&w, "}");

    if (w.over) {
        answer_fault(HAL_CLOUD_FAULT_STORAGE);
        return;
    }

    settle(HAL_CLOUD_FAULT_NONE, w.used);
}

// =============================================================================
// The credentials file
// -----------------------------------------------------------------------------
// Read and written whole, both sections at a time. The file is two short blocks and this board is
// the only thing that ever writes it, so there is nothing to preserve by editing it in place --
// and rewriting it whole is what makes "forget one provider" and "change the other" the same piece
// of code.
//
// The reader is not a TOML parser either, for the same reason the JSON reader is not a JSON
// parser: this file wrote the file, so it knows the shape. A section header, then `key = "value"`
// lines, and nothing else -- the whole of the grammar it will accept.
// =============================================================================

/** The two section names, in the order they are looked for and written. */
static const char *const CLOUD_SECTIONS[HAL_CLOUD_PROVIDER_COUNT] = { "cvm", "lighthouse" };

/**
 * The span of one `[name]` section, `out_start` at the first character after the header's newline.
 *
 * A section runs to the next header or to the end of the text, which is what keeps a key looked for
 * in one section from being found in the other -- the two hold different SecretKeys under the same
 * key names, so a search that ran past the header would hand the second provider the first one's key
 * and the failure would be an authentication error with nothing pointing here.
 */
static bool toml_section(const char *text, const char *name, const char **out_start,
                         const char **out_end)
{
    char header[32];
    const int made = snprintf(header, sizeof header, "[%s]", name);
    if (made < 0 || (size_t)made >= sizeof header) {
        return false;
    }

    const char *at = strstr(text, header);
    if (at == NULL) {
        return false;
    }

    const char *start = strchr(at, '\n');
    if (start == NULL) {
        start = at + strlen(header);
    } else {
        start++;
    }

    const char *next = strchr(start, '[');
    *out_start = start;
    *out_end = (next != NULL) ? next : (start + strlen(start));

    return true;
}

/** One `key = "value"` line inside a span, with its escapes resolved. */
static bool toml_string(const char *start, const char *end, const char *key, char *out,
                        size_t capacity)
{
    char pattern[32];
    const int made = snprintf(pattern, sizeof pattern, "%s ", key);
    if (made < 0 || (size_t)made >= sizeof pattern) {
        return false;
    }

    const char *at = start;
    while ((at = strstr(at, pattern)) != NULL && at < end) {
        const char *value = at + strlen(pattern);

        while (*value == ' ' || *value == '\t') {
            value++;
        }
        if (*value != '=') {
            at += strlen(pattern);
            continue;
        }

        value++;
        while (*value == ' ' || *value == '\t') {
            value++;
        }
        if (*value != '"') {
            at += strlen(pattern);
            continue;
        }

        /* The value is copied out here rather than handed to `json_string`: the escape set TOML's
         * writer below emits is the same two, but the reader that resolves them is one function and
         * a second copy of it would be a second place for them to disagree. */
        return json_string(value, out, capacity);
    }

    return false;
}

/** Read the file into the table. A file that is not there is the ordinary answer -- a board that has
 * never been told a key -- and leaves the table zeroed. */
static esp_err_t cloud_load(void)
{
    char *text = (char *)malloc(HAL_CLOUD_CONF_MAX);

    if (text == NULL) {
        return ESP_ERR_NO_MEM;
    }

    size_t length = 0;
    const esp_err_t read = hal_storage_read_file(CLOUD_CONF_PATH, text, HAL_CLOUD_CONF_MAX, &length);

    if (read != ESP_OK) {
        free(text);
        memset(s_accounts, 0, sizeof s_accounts);
        return read;
    }

    memset(s_accounts, 0, sizeof s_accounts);

    for (int32_t provider = 0; provider < HAL_CLOUD_PROVIDER_COUNT; provider++) {
        const char *start = NULL;
        const char *end = NULL;

        if (!toml_section(text, CLOUD_SECTIONS[provider], &start, &end)) {
            continue;
        }

        cloud_account_t *account = &s_accounts[provider];

        if (!toml_string(start, end, "secret_id", account->secret_id, sizeof account->secret_id)) {
            continue;
        }
        if (!toml_string(start, end, "secret_key", account->secret_key, sizeof account->secret_key)) {
            /* A SecretId without a SecretKey is a half-written file, and half a key signs nothing:
             * the entry is dropped whole rather than left to fail at the API. */
            account->secret_id[0] = '\0';
            continue;
        }

        if (!toml_string(start, end, "region", account->region, sizeof account->region)) {
            account->region[0] = '\0';
        }

        account->used = true;
    }

    free(text);

    return ESP_OK;
}

/**
 * Write the table back, both sections, whether or not either is in use.
 *
 * The text is staged and handed to `hal_storage_write_file`, which is what makes the directory
 * above it -- `AppData/CLOUD/` does not exist until the first key is saved, and a `fopen` here
 * would be the one thing in this file that has to know that.
 *
 * The quoting is `snprintf`'s and not an escaper's, because a key with a quote in it never gets
 * this far: `hal_cloud_credentials_set` refuses one at the door. A file format this file both
 * writes and reads is not the place to find out what a SecretKey is allowed to contain.
 */
static esp_err_t cloud_store(void)
{
    char text[HAL_CLOUD_CONF_MAX];
    size_t used = 0;

    for (int32_t provider = 0; provider < HAL_CLOUD_PROVIDER_COUNT; provider++) {
        const cloud_account_t *account = &s_accounts[provider];

        if (!account->used) {
            continue;
        }

        const int made = snprintf(text + used, sizeof text - used,
                                  "[%s]\nregion = \"%s\"\nsecret_id = \"%s\"\nsecret_key = \"%s\"\n\n",
                                  CLOUD_SECTIONS[provider], account->region, account->secret_id,
                                  account->secret_key);

        if (made < 0 || (size_t)made >= sizeof text - used) {
            ESP_LOGE(TAG, "the key file did not fit in %d bytes", HAL_CLOUD_CONF_MAX);
            return ESP_ERR_INVALID_SIZE;
        }

        used += (size_t)made;
    }

    const esp_err_t err = hal_storage_write_file(CLOUD_CONF_PATH, text, used);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "cannot write %s: %s", CLOUD_CONF_PATH, esp_err_to_name(err));
        return err;
    }

    ESP_LOGI(TAG, "the keys are in %s", CLOUD_CONF_PATH);

    return ESP_OK;
}

/** The table, read once. Called from every entry point that needs a key, because the filesystem is
 * not up when this file is compiled and may not be up when the first of them runs. */
static void cloud_ensure_loaded(void)
{
    if (s_loaded) {
        return;
    }

    s_loaded = true;
    (void)cloud_load();
}

// =============================================================================
// The request
// =============================================================================

/**
 * Which fault a failed connection is.
 *
 * A certificate problem and an unreachable host arrive here as the same kind of thing -- a negative
 * number -- and they are very different sentences to the person reading the page. esp-tls reports
 * itself in the 0x8000 block and passes mbedTLS's own codes through as negatives from about -0x1000
 * down, which is the whole of the distinction that can be made from here without linking the error
 * tables in. Anything else is "I could not reach it".
 */
static int32_t connection_fault(esp_err_t err)
{
    if ((err >= 0x8000 && err < 0x9000) || err <= -0x1000) {
        return HAL_CLOUD_FAULT_TLS;
    }

    return HAL_CLOUD_FAULT_UNREACHABLE;
}

/** Whether a string is the shape an instance id or a region is: letters, digits, `-` and `_`, and
 * short. Both go into a JSON body this file builds by hand, so this is what keeps a quote out of
 * it -- and neither is ever anything else, so refusing the rest costs nothing. */
static bool identifier_ok(const char *text, size_t capacity)
{
    const size_t length = strlen(text);

    if (length == 0 || length >= capacity) {
        return false;
    }

    for (const char *at = text; *at != '\0'; at++) {
        const bool ok = (*at >= 'a' && *at <= 'z') || (*at >= 'A' && *at <= 'Z') ||
                        (*at >= '0' && *at <= '9') || *at == '-' || *at == '_';

        if (!ok) {
            return false;
        }
    }

    return true;
}

/** The endpoint, and the three constants that go with it: the API family in the signature, and the
 * version in the header. They are a property of the provider and not of the call. */
static void cloud_endpoint(int32_t provider, const char **out_host, const char **out_service,
                           const char **out_version)
{
    if (provider == HAL_CLOUD_LIGHTHOUSE) {
        *out_host = "lighthouse.tencentcloudapi.com";
        *out_service = "lighthouse";
        *out_version = "2020-03-24";
    } else {
        *out_host = "cvm.tencentcloudapi.com";
        *out_service = "cvm";
        *out_version = "2017-03-12";
    }
}

/** The API's own name for what was asked. */
static const char *cloud_action_name(int32_t action)
{
    switch (action) {
    case HAL_CLOUD_ACTION_START:  return "StartInstances";
    case HAL_CLOUD_ACTION_STOP:   return "StopInstances";
    case HAL_CLOUD_ACTION_REBOOT: return "RebootInstances";
    default:                      return "DescribeInstances";
    }
}

/** What one call brought back: the body, already in `s_response`. */
typedef struct {
    char *out;
    size_t capacity;
    size_t used;
} cloud_reply_t;

/**
 * One provider's connection, kept open across the regions of a merged list.
 *
 * `esp_http_client_perform` reuses the TCP/TLS connection when the answer said keep-alive, and that
 * is the whole reason this is a session rather than a function: a scan asks the same host twenty
 * times, and twenty handshakes would be a minute of waiting where twenty requests are a few
 * seconds. The first call opens the connection; every later region is one request on it.
 */
typedef struct {
    int32_t provider;
    esp_http_client_handle_t client;
    cloud_reply_t reply;
    /** The answer was longer than `s_response`; what arrived is kept and the tail is lost. */
    bool overflowed;
} cloud_session_t;

/** The body of an answer, as it arrives. */
static esp_err_t cloud_collect(esp_http_client_event_t *event)
{
    cloud_session_t *session = (cloud_session_t *)event->user_data;

    if (event->event_id != HTTP_EVENT_ON_DATA || event->data == NULL || event->data_len <= 0) {
        return ESP_OK;
    }

    const size_t room = session->reply.capacity - 1 - session->reply.used;
    const size_t take = (size_t)event->data_len < room ? (size_t)event->data_len : room;

    memcpy(session->reply.out + session->reply.used, event->data, take);
    session->reply.used += take;

    if (take < (size_t)event->data_len) {
        session->overflowed = true;
    }

    return ESP_OK;
}

/** Opens the connection holder for `provider`. No socket is opened until the first call. */
static bool cloud_session_open(cloud_session_t *session, int32_t provider)
{
    memset(session, 0, sizeof *session);

    session->provider       = provider;
    session->reply.out      = s_response;
    session->reply.capacity = CLOUD_RESPONSE_MAX;

    const char *host = NULL;
    const char *service = NULL;
    const char *version = NULL;
    cloud_endpoint(provider, &host, &service, &version);
    (void)service;
    (void)version;

    char url[96];
    snprintf(url, sizeof url, "https://%s/", host);

    const esp_http_client_config_t config = {
        .url               = url,
        .method            = HTTP_METHOD_POST,
        .timeout_ms        = CLOUD_TIMEOUT_MS,
        .buffer_size       = CLOUD_IO_CHUNK,
        .buffer_size_tx    = CLOUD_IO_CHUNK,
        .crt_bundle_attach = esp_crt_bundle_attach,
        .keep_alive_enable = true,
        .event_handler     = cloud_collect,
        .user_data         = session,
    };

    session->client = esp_http_client_init(&config);
    if (session->client == NULL) {
        return false;
    }

    esp_http_client_set_header(session->client, "Content-Type", "application/json; charset=utf-8");

    return true;
}

static void cloud_session_close(cloud_session_t *session)
{
    if (session->client != NULL) {
        esp_http_client_cleanup(session->client);
        session->client = NULL;
    }
}

/**
 * One API call on an open session: sign it, send it, and leave the body in `s_response`.
 *
 * Returns `HAL_CLOUD_FAULT_NONE` when what came back is a whole body, and the fault otherwise -- the
 * connection, the handshake, or an answer that stopped in the middle. It draws nothing: whether a
 * body is worth drawing, and what to say when it is not, is the caller's decision, because a merged
 * list has many bodies to judge.
 */
static int32_t cloud_session_call(cloud_session_t *session, int32_t action, const char *region,
                                  const char *instance, int *out_status)
{
    *out_status = 0;

    const char *host = NULL;
    const char *service = NULL;
    const char *version = NULL;
    cloud_endpoint(session->provider, &host, &service, &version);

    /* The body. A describe says how many rows to bring back; a power action names the one machine.
     * `Limit` and `Offset` are required by Lighthouse and optional for CVM, so both get them. */
    char body[192];

    if (action == HAL_CLOUD_ACTION_LIST) {
        snprintf(body, sizeof body, "{\"Limit\":%d,\"Offset\":0}", CLOUD_MAX_INSTANCES);
    } else {
        snprintf(body, sizeof body, "{\"InstanceIds\":[\"%s\"]}", instance);
    }

    int64_t timestamp = 0;
    if (hal_system_get_epoch(&timestamp) != ESP_OK) {
        return HAL_CLOUD_FAULT_CLOCK;
    }

    char authorization[CLOUD_AUTH_MAX];
    if (!cloud_authorize(service, host, &s_accounts[session->provider], timestamp, body, authorization,
                         sizeof authorization)) {
        ESP_LOGE(TAG, "the signature could not be built");
        return HAL_CLOUD_FAULT_STORAGE;
    }

    char stamp[24];
    snprintf(stamp, sizeof stamp, "%lld", (long long)timestamp);

    session->reply.used = 0;
    session->reply.out[0] = '\0';
    session->overflowed = false;

    /* The five the API needs, and the one that carries the signature. `Host` is set by the client
     * from the URL and is what the signature signed, so it is deliberately not set here. */
    esp_http_client_set_header(session->client, "Authorization", authorization);
    esp_http_client_set_header(session->client, "X-TC-Action", cloud_action_name(action));
    esp_http_client_set_header(session->client, "X-TC-Version", version);
    esp_http_client_set_header(session->client, "X-TC-Timestamp", stamp);

    /* A kept-alive session carries the previous region in its header list: a call that names none
     * has to take it back out, or a power command would be sent to whatever region was asked for
     * last. */
    if (region[0] != '\0') {
        esp_http_client_set_header(session->client, "X-TC-Region", region);
    } else {
        esp_http_client_delete_header(session->client, "X-TC-Region");
    }

    esp_http_client_set_post_field(session->client, body, (int)strlen(body));

    ESP_LOGI(TAG, "%s %s%s%s", cloud_action_name(action), provider_name(session->provider),
             region[0] != '\0' ? " in " : "", region);

    const esp_err_t err = esp_http_client_perform(session->client);
    const int status = (err == ESP_OK) ? esp_http_client_get_status_code(session->client) : 0;

    if (err != ESP_OK) {
        ESP_LOGW(TAG, "the call failed: %s", esp_err_to_name(err));
        /* The state of a connection that failed mid-answer is not one to hand the next region: the
         * next call opens a fresh one. */
        esp_http_client_close(session->client);

        return connection_fault(err);
    }

    session->reply.out[session->reply.used] = '\0';

    if (session->reply.used == 0) {
        /* Not an empty list: no answer at all. */
        ESP_LOGW(TAG, "the API answered %d with an empty body", status);
        return HAL_CLOUD_FAULT_HTTP;
    }

    if (session->overflowed) {
        ESP_LOGW(TAG, "the answer was cut at %d bytes", CLOUD_RESPONSE_MAX - 1);
    }

    ESP_LOGD(TAG, "HTTP %d, %u bytes", status, (unsigned)session->reply.used);

    *out_status = status;

    return HAL_CLOUD_FAULT_NONE;
}

/**
 * A call, tried a second time when it was the wire that failed.
 *
 * The failures worth another try are the ones about the network -- DNS, the handshake, a body that
 * stopped early. A key that is wrong, a clock that was never set or a signature that could not be
 * built fail the same way the second time, and making somebody wait twice for the same sentence is
 * worse than saying it once.
 */
static int32_t cloud_fetch_retrying(int32_t provider, int32_t action, const char *region,
                                    const char *instance, int *out_status)
{
    cloud_session_t session;

    if (!cloud_session_open(&session, provider)) {
        return HAL_CLOUD_FAULT_STORAGE;
    }

    int32_t fault = HAL_CLOUD_FAULT_NONE;

    for (int attempt = 1; attempt <= CLOUD_TRIES; attempt++) {
        fault = cloud_session_call(&session, action, region, instance, out_status);

        if (fault == HAL_CLOUD_FAULT_NONE) {
            break;
        }

        const bool again = fault == HAL_CLOUD_FAULT_UNREACHABLE || fault == HAL_CLOUD_FAULT_TLS ||
                           fault == HAL_CLOUD_FAULT_HTTP;

        if (!again) {
            break;
        }

        if (attempt < CLOUD_TRIES) {
            ESP_LOGW(TAG, "attempt %d of %d got no answer, asking again", attempt, CLOUD_TRIES);
            vTaskDelay(pdMS_TO_TICKS(250));
        }
    }

    cloud_session_close(&session);

    return fault;
}

/** Whether an API error is about the account rather than about the region being unsold there. */
static bool refusal_is_about_the_account(const char *code)
{
    static const char *const ACCOUNT[] = {
        "AuthFailure", "SignatureFailure", "InvalidSecretId", "InvalidCredential",
        "UnauthorizedOperation", "SecretIdNotFound",
    };

    for (size_t i = 0; i < sizeof ACCOUNT / sizeof ACCOUNT[0]; i++) {
        if (strncmp(code, ACCOUNT[i], strlen(ACCOUNT[i])) == 0) {
            return true;
        }
    }

    return false;
}

/**
 * Every server, from every data centre of every provider that has a key.
 *
 * A `DescribeInstances` knows one region, so a list that is "all of them" is one call per region:
 * the key's own region was why a machine in Tokyo was invisible. Each provider's calls share one
 * connection (see [`cloud_session_t`]), and a region the account has not opened answers with an
 * `Error` object that is simply skipped -- that is an unsold data centre, not a failed list.
 *
 * `kind` and `region` on each row are what tells a button which API and which data centre to send
 * `StartInstances` to. A provider with no key on file is not a failure and is not mentioned -- that
 * is a person who owns machines at one place. One that answered nothing is named in `failed`, so a
 * page can say "轻量的这次没取到" instead of quietly showing half of what somebody owns. When nothing
 * came back at all, the first real reason is the answer, because that is the one worth reading.
 */
static void render_all_instances(void)
{
    writer_t w = { .out = s_answer, .capacity = CLOUD_ANSWER_MAX };
    int listed = 0;
    int tried = 0;
    int ok = 0;

    bool failed[HAL_CLOUD_PROVIDER_COUNT];
    memset(failed, 0, sizeof failed);

    int32_t first_fault = HAL_CLOUD_FAULT_NONE;
    char first_code[64];
    char first_message[256];
    first_code[0] = '\0';
    first_message[0] = '\0';

    /* A minute of calls is what a scan of every region costs on a bad line, and a page that waits
     * that long is worse than a page that shows the twenty rows it got. Past the budget the scan
     * stops and whatever answered is the list. */
    const int64_t budget_end = esp_timer_get_time() + (int64_t)CLOUD_SCAN_BUDGET_MS * 1000;

    w_printf(&w, "{\"ok\":1,\"provider\":\"all\",\"region\":\"\",\"instances\":[");

    for (int32_t p = 0; p < HAL_CLOUD_PROVIDER_COUNT; p++) {
        if (!s_accounts[p].used) {
            continue;
        }

        tried++;

        const char *const *regions = NULL;
        size_t region_count = 0;
        cloud_regions(p, &regions, &region_count);

        cloud_session_t session;

        if (!cloud_session_open(&session, p)) {
            failed[p] = true;
            first_fault = HAL_CLOUD_FAULT_STORAGE;
            continue;
        }

        bool answered = false;
        bool kept = false;
        int32_t provider_fault = HAL_CLOUD_FAULT_NONE;

        for (size_t r = 0; r < region_count && listed < CLOUD_MAX_INSTANCES; r++) {
            if (esp_timer_get_time() > budget_end) {
                ESP_LOGW(TAG, "the scan ran past its budget at %s", regions[r]);
                break;
            }

            int status = 0;
            const int32_t fault =
                cloud_session_call(&session, HAL_CLOUD_ACTION_LIST, regions[r], "", &status);

            if (fault != HAL_CLOUD_FAULT_NONE) {
                if (!kept) {
                    provider_fault = fault;
                }

                continue;
            }

            kept = true;

            char code[64];
            char message[256];
            code[0] = '\0';
            message[0] = '\0';

            /* Reading the body before the status is what turns "HTTP 403" into "签名错误". */
            const bool refused = body_is_refusal(s_response, code, sizeof code, message,
                                                 sizeof message);

            if (refused) {
                /* One data centre that is not sold to this account, or the account itself is the
                 * problem. The latter is worth keeping for the case where *every* region says it. */
                if (code[0] != '\0' && refusal_is_about_the_account(code) && first_code[0] == '\0') {
                    first_fault = HAL_CLOUD_FAULT_API;
                    snprintf(first_code, sizeof first_code, "%s", code);
                    snprintf(first_message, sizeof first_message, "%s", message);
                }

                continue;
            }

            if (status < 200 || status >= 300) {
                continue;
            }

            if (append_instances(&w, s_response, p, regions[r], &listed)) {
                answered = true;
            }
        }

        cloud_session_close(&session);

        if (answered) {
            ok++;
            continue;
        }

        failed[p] = true;

        if (first_code[0] == '\0') {
            first_fault = provider_fault != HAL_CLOUD_FAULT_NONE ? provider_fault
                                                                 : HAL_CLOUD_FAULT_API;
            snprintf(first_code, sizeof first_code, "MalformedAnswer");
            snprintf(first_message, sizeof first_message, "接口没有返回实例列表");
        }
    }

    w_printf(&w, "],\"count\":%d", listed);

    bool any_failed = false;
    for (int32_t p = 0; p < HAL_CLOUD_PROVIDER_COUNT; p++) {
        any_failed = any_failed || failed[p];
    }

    if (any_failed) {
        int written = 0;

        w_printf(&w, ",\"failed\":[");

        for (int32_t p = 0; p < HAL_CLOUD_PROVIDER_COUNT; p++) {
            if (!failed[p]) {
                continue;
            }

            if (written > 0) {
                w_printf(&w, ",");
            }

            w_printf(&w, "\"%s\"", provider_name(p));
            written++;
        }

        w_printf(&w, "]");
    }

    w_printf(&w, "}");

    if (tried == 0) {
        /* No key at all is not an empty list: it is a board that has not been told how to reach the
         * cloud yet, and the page has a sentence and a form for that. */
        answer_fault(HAL_CLOUD_FAULT_NO_CREDS);
        return;
    }

    if (ok == 0) {
        if (first_code[0] != '\0') {
            answer_api_error(first_code, first_message);
        } else {
            answer_fault(first_fault != HAL_CLOUD_FAULT_NONE ? first_fault : HAL_CLOUD_FAULT_HTTP);
        }

        return;
    }

    if (w.over) {
        ESP_LOGW(TAG, "the answer for %d instances did not fit in %d bytes", listed, CLOUD_ANSWER_MAX);
        answer_fault(HAL_CLOUD_FAULT_STORAGE);
        return;
    }

    settle(HAL_CLOUD_FAULT_NONE, w.used);
}

/**
 * One request, from the socket to the answer.
 *
 * Runs on its own task with a stack deep enough for the handshake, because everything here blocks:
 * the DNS lookup, the TCP connect, the TLS handshake and the read. The frame loop and the web
 * server's handler both call the board from places that cannot afford any of that.
 *
 * Every way out of this is a `return`, and none of them deletes the task: that is [`cloud_task`]'s
 * job, and it is the one line this function must not be asked to remember on its own.
 */
static void cloud_request(void)
{
    int32_t provider, action;
    char region[HAL_CLOUD_REGION_MAX];
    char instance[HAL_CLOUD_INSTANCE_MAX];

    lock();
    provider = s_provider;
    action   = s_action;
    snprintf(region, sizeof region, "%s", s_region);
    snprintf(instance, sizeof instance, "%s", s_instance);
    const bool cancelled = s_snap.cancelled;
    unlock();

    if (cancelled) {
        /* Somebody let go before the first byte went out. There is no answer to write and nobody to
         * write it for: the slot goes back to idle and the next request finds it empty. */
        lock();
        s_snap.state  = HAL_CLOUD_IDLE;
        s_snap.fault  = HAL_CLOUD_FAULT_NONE;
        s_snap.length = 0;
        s_snap.busy   = false;
        unlock();
        return;
    }

    if (action == HAL_CLOUD_ACTION_LIST && provider == HAL_CLOUD_ALL) {
        render_all_instances();
        return;
    }

    int status = 0;
    const int32_t fault = cloud_fetch_retrying(provider, action, region, instance, &status);

    if (fault != HAL_CLOUD_FAULT_NONE) {
        answer_fault(fault);
        return;
    }

    char code[64];
    char message[256];

    if (body_is_refusal(s_response, code, sizeof code, message, sizeof message)) {
        answer_api_error(code, message);
        return;
    }

    if (status < 200 || status >= 300) {
        snprintf(code, sizeof code, "HTTP%d", status);
        snprintf(message, sizeof message, "腾讯云返回了 HTTP %d，请稍后再试", status);

        answer_api_error(code, message);
        return;
    }

    if (action == HAL_CLOUD_ACTION_LIST) {
        render_instances(s_response, provider, region);
    } else {
        render_accepted(s_response, provider, action);
    }
}

/**
 * The worker task: one request, and then the task itself.
 *
 * The `vTaskDelete` is not tidiness. A task function that returns never comes back through here —
 * FreeRTOS calls `prvTaskExitError`, which trips a `configASSERT` with interrupts already disabled,
 * and the board panics and reboots. On the panel that is "the list arrives and the screen goes
 * black"; on the web page it is a browser that loses the answer it was about to be sent. So the
 * request is a plain function with a dozen exits, and this is the one place that has to finish the
 * task properly. `board_http.c`'s worker ends the same way, for the same reason.
 */
static void cloud_task(void *arg)
{
    (void)arg;

    cloud_request();

    vTaskDelete(NULL);
}

// =============================================================================
// What board_hal.h publishes
// =============================================================================

esp_err_t hal_cloud_init(void)
{
    if (s_lock == NULL) {
        s_lock = xSemaphoreCreateMutex();
        if (s_lock == NULL) {
            return ESP_ERR_NO_MEM;
        }
    }

    /* Both buffers once, out of PSRAM by preference: they are the largest things this file owns,
     * they are alive for the life of the board, and nothing needs either of them fast. A board
     * without PSRAM gets them out of the heap instead -- they are four kilobytes each and there is
     * room for that. */
    if (s_answer == NULL) {
        s_answer = (char *)heap_caps_malloc(CLOUD_ANSWER_MAX, MALLOC_CAP_SPIRAM);
        if (s_answer == NULL) {
            s_answer = (char *)malloc(CLOUD_ANSWER_MAX);
        }
    }

    if (s_response == NULL) {
        s_response = (char *)heap_caps_malloc(CLOUD_RESPONSE_MAX, MALLOC_CAP_SPIRAM);
        if (s_response == NULL) {
            s_response = (char *)malloc(CLOUD_RESPONSE_MAX);
        }
    }

    if (s_answer == NULL || s_response == NULL) {
        ESP_LOGE(TAG, "no room for the answer buffers");
        return ESP_ERR_NO_MEM;
    }

    cloud_ensure_loaded();

    return ESP_OK;
}

esp_err_t hal_cloud_start(int32_t provider, int32_t action, const char *region,
                          const char *instance)
{
    if (provider < 0 || provider > HAL_CLOUD_ALL) {
        return ESP_ERR_INVALID_ARG;
    }
    if (action < HAL_CLOUD_ACTION_LIST || action > HAL_CLOUD_ACTION_REBOOT) {
        return ESP_ERR_INVALID_ARG;
    }
    if (provider == HAL_CLOUD_ALL && action != HAL_CLOUD_ACTION_LIST) {
        /* A power action has to name one machine, and "all of them" is not one: the union is a
         * list's target and nothing else. */
        return ESP_ERR_INVALID_ARG;
    }
    if (region != NULL && region[0] != '\0' && !identifier_ok(region, HAL_CLOUD_REGION_MAX)) {
        return ESP_ERR_INVALID_ARG;
    }
    if (action != HAL_CLOUD_ACTION_LIST && !identifier_ok(instance, HAL_CLOUD_INSTANCE_MAX)) {
        return ESP_ERR_INVALID_ARG;
    }

    if (hal_cloud_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();
    if (s_snap.busy) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }

    bool have_key = false;
    const char *key_region = "";

    if (provider == HAL_CLOUD_ALL) {
        /* Any key at all makes "everything" worth asking. Which providers are actually asked is the
         * worker's decision, from the same table. */
        for (int32_t p = 0; p < HAL_CLOUD_PROVIDER_COUNT; p++) {
            have_key = have_key || s_accounts[p].used;
        }
    } else {
        have_key = s_accounts[provider].used;
        key_region = s_accounts[provider].region;
    }

    s_provider = provider;
    s_action   = action;
    s_snap.cancelled = false;
    /* The region comes from the caller when it named one and from the key when it did not: the page
     * sends what is in its field, and the panel app has no field. */
    snprintf(s_region, sizeof s_region, "%s",
             (region != NULL && region[0] != '\0') ? region
                                                   : (key_region[0] != '\0' ? key_region
                                                                            : "ap-guangzhou"));
    snprintf(s_instance, sizeof s_instance, "%s", instance != NULL ? instance : "");

    s_snap.state  = HAL_CLOUD_RUNNING;
    s_snap.fault  = HAL_CLOUD_FAULT_NONE;
    s_snap.length = 0;
    s_snap.busy   = true;
    unlock();

    /* Answered before the task starts, so the page's error arrives in the same round trip as a
     * failed request rather than three seconds later through the socket's timeout. */
    if (!have_key) {
        answer_fault(HAL_CLOUD_FAULT_NO_CREDS);
        return ESP_OK;
    }

    if (!network_is_up()) {
        answer_fault(HAL_CLOUD_FAULT_NO_NETWORK);
        return ESP_OK;
    }

    if (!clock_is_set()) {
        answer_fault(HAL_CLOUD_FAULT_CLOCK);
        return ESP_OK;
    }

    if (xTaskCreate(cloud_task, "cloud", CLOUD_TASK_STACK, NULL, 5, NULL) != pdPASS) {
        answer_fault(HAL_CLOUD_FAULT_STORAGE);
        return ESP_ERR_NO_MEM;
    }

    return ESP_OK;
}

int32_t hal_cloud_state(void)
{
    lock();
    const int32_t state = s_snap.state;
    unlock();

    return state;
}

int32_t hal_cloud_fault(void)
{
    lock();
    const int32_t fault = s_snap.fault;
    unlock();

    return fault;
}

const char *hal_cloud_answer(size_t *out_length)
{
    lock();
    /* A failure has an answer too, and it is the one worth reading: `{"error":"cloud_clock"}` is the
     * whole of what the page has to draw. Only a state that has not got there yet has nothing. */
    const bool ready = (s_snap.state == HAL_CLOUD_DONE || s_snap.state == HAL_CLOUD_FAILED) &&
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

void hal_cloud_take(void)
{
    lock();
    s_snap.state  = HAL_CLOUD_IDLE;
    s_snap.fault  = HAL_CLOUD_FAULT_NONE;
    s_snap.length = 0;
    unlock();
}

size_t hal_cloud_take_answer(char *out, size_t capacity)
{
    if (out == NULL || capacity == 0) {
        return 0;
    }

    out[0] = '\0';

    if (hal_cloud_init() != ESP_OK) {
        return 0;
    }

    lock();

    /* Copied and released under one lock: between the moment the pointer is handed out and the
     * moment it is handed back, the answer belongs to whoever asked for it -- and a second browser
     * tab asking for a list in that window would otherwise have the document rewritten underneath
     * the first one while it was being sent. */
    const bool ready = (s_snap.state == HAL_CLOUD_DONE || s_snap.state == HAL_CLOUD_FAILED) &&
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

    s_snap.state  = HAL_CLOUD_IDLE;
    s_snap.fault  = HAL_CLOUD_FAULT_NONE;
    s_snap.length = 0;

    unlock();

    return length;
}

void hal_cloud_cancel(void)
{
    lock();
    s_snap.cancelled = true;

    /* A worker is closing its socket and will write the answer nobody is waiting for; the slot is
     * not free until it does. Nothing in flight means the slot is free now. */
    if (!s_snap.busy) {
        s_snap.state  = HAL_CLOUD_IDLE;
        s_snap.fault  = HAL_CLOUD_FAULT_NONE;
        s_snap.length = 0;
    }
    unlock();
}

// =============================================================================
// The keys
// =============================================================================

esp_err_t hal_cloud_credentials_get(int32_t provider, char *out_masked, size_t capacity,
                                    char *out_region, size_t region_capacity)
{
    if (provider < 0 || provider >= HAL_CLOUD_PROVIDER_COUNT) {
        return ESP_ERR_INVALID_ARG;
    }
    if (out_masked == NULL || capacity == 0) {
        return ESP_ERR_INVALID_ARG;
    }

    if (hal_cloud_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    lock();
    const cloud_account_t *account = &s_accounts[provider];
    const bool used = account->used;
    const size_t id_length = strlen(account->secret_id);

    /* The tail is what tells two keys apart, and the head is what tells a person it is a SecretId
     * and not something else. Everything between them is the part that must not come back out. */
    if (used) {
        if (id_length > 8) {
            snprintf(out_masked, capacity, "%.4s……%.4s", account->secret_id,
                     account->secret_id + id_length - 4);
        } else {
            snprintf(out_masked, capacity, "%s", account->secret_id);
        }

        if (out_region != NULL && region_capacity > 0) {
            snprintf(out_region, region_capacity, "%s", account->region);
        }
    }
    unlock();

    return used ? ESP_OK : ESP_ERR_NOT_FOUND;
}

/**
 * Whether a key is something the file can hold and the signature can sign.
 *
 * Printable ASCII without the two characters that would end a TOML string or a JSON one. Every
 * SecretId and SecretKey Tencent issues is letters and digits, so this refuses only what somebody
 * pasted by mistake -- and it refuses it here, where the answer is a sentence, rather than at the
 * API three seconds later where the answer is `AuthFailure.SignatureFailure`.
 */
static bool key_characters_ok(const char *text, size_t capacity)
{
    const size_t length = strlen(text);

    if (length == 0 || length >= capacity) {
        return false;
    }

    for (const char *at = text; *at != '\0'; at++) {
        const unsigned char c = (unsigned char)*at;

        if (c < 0x20 || c >= 0x7F || c == '"' || c == '\\') {
            return false;
        }
    }

    return true;
}

esp_err_t hal_cloud_credentials_set(int32_t provider, const char *region, const char *secret_id,
                                    const char *secret_key)
{
    if (provider < 0 || provider >= HAL_CLOUD_PROVIDER_COUNT) {
        return ESP_ERR_INVALID_ARG;
    }
    if (!key_characters_ok(secret_id, HAL_CLOUD_SECRET_ID_MAX) ||
        !key_characters_ok(secret_key, HAL_CLOUD_SECRET_KEY_MAX)) {
        return ESP_ERR_INVALID_ARG;
    }
    if (region != NULL && region[0] != '\0' && !identifier_ok(region, HAL_CLOUD_REGION_MAX)) {
        return ESP_ERR_INVALID_ARG;
    }

    if (hal_cloud_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    cloud_account_t *account = &s_accounts[provider];

    memset(account, 0, sizeof *account);
    snprintf(account->region, sizeof account->region, "%s",
             (region != NULL && region[0] != '\0') ? region : "ap-guangzhou");
    snprintf(account->secret_id, sizeof account->secret_id, "%s", secret_id);
    snprintf(account->secret_key, sizeof account->secret_key, "%s", secret_key);
    account->used = true;

    const esp_err_t err = cloud_store();
    if (err != ESP_OK) {
        /* The new key is in memory and not on the card. Taking it back out is the honest end: the
         * caller is told the save failed, and the board goes on answering with the key it had. */
        memset(account, 0, sizeof *account);
        return err;
    }

    return ESP_OK;
}

esp_err_t hal_cloud_credentials_forget(int32_t provider)
{
    if (provider < 0 || provider >= HAL_CLOUD_PROVIDER_COUNT) {
        return ESP_ERR_INVALID_ARG;
    }

    if (hal_cloud_init() != ESP_OK) {
        return ESP_ERR_NO_MEM;
    }

    cloud_account_t *account = &s_accounts[provider];
    if (!account->used) {
        return ESP_ERR_NOT_FOUND;
    }

    cloud_account_t saved = *account;
    memset(account, 0, sizeof *account);

    const esp_err_t err = cloud_store();
    if (err != ESP_OK) {
        *account = saved;
        return err;
    }

    return ESP_OK;
}
