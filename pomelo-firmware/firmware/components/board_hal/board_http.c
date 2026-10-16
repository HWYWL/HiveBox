// =============================================================================
// HTTP client (esp_http_client + ESP-TLS)
// -----------------------------------------------------------------------------
// The Rust `HttpBackend`, served by a worker task: `hal_http_start_to` names a URL
// and returns, the task does the resolve, the handshake and the read, and the
// state kept here is what the poll reads. Non-blocking for the same reason the
// Wi-Fi scan is — a frame loop cannot wait a second and a half for a server, and
// a page that did would be a page that froze.
//
// Two things are refused before the network is touched, because both are knowable
// here and neither is worth dressing up as "connection failed": the board being
// off the network, and an `https` URL on a board whose clock has never been set.
// The second is the one that catches people — every certificate carries a
// validity window and mbedTLS judges it against the system clock, so a board
// reading 1970 rejects every certificate ever issued. `hal_time_sync()` is what
// fixes that; this file is where it shows.
//
// One request at a time, and the answer is *taken* rather than read twice:
// `hal_http_take` hands over the bytes and clears the state that said they were
// there.
//
// A body goes one of three ways. `HAL_HTTP_SINK_MEMORY` is a document: it lands in
// the one buffer this file owns and `HAL_HTTP_BODY_MAX` is the bound on it, which
// is what stops a page that asked for something unbounded from taking the heap
// with it. `HAL_HTTP_SINK_FILE` is a file — a glyph table, a font — and is
// written out as it arrives through the same buffer used as a staging area, so
// its size is the filesystem's problem rather than this board's memory. A
// download that ends any way but successfully removes what it wrote: half a glyph
// table reads as a font with some of its characters missing, and that is a
// failure which surfaces a long way from its cause.
//
// `HAL_HTTP_SINK_OTA` is the third, and the only one whose destination this file
// chooses: `esp_ota_get_next_update_partition` picks the app slot this image is
// not running from, and the body is a firmware image going into it. Three rules
// come with that and none has an analogue in the other two sinks. A slot is
// only written to by an answer that *is* a success — a `404` page landing there
// would erase an image to put a web page in its place. And a slot is only
// *bootable* once the request has finished: `esp_ota_end` is what says the bytes
// are an image this chip can start, `esp_ota_set_boot_partition` is what points
// the next boot at it, and anything that leaves before those two — a cancel, a
// read that dies mid-body, a full slot — calls `esp_ota_abort`, which leaves the
// boot partition where it was. So the worst a failed update does is fill a slot
// nobody boots from, and the next attempt erases it again.
//
// And the erasing takes its turn with the reading instead of running ahead of
// it. A slot has to be erased before it can be written, an image is five
// megabytes, and five megabytes of erase is ninety-odd block erases — several
// seconds of a progress bar that was put on the screen the moment the button
// was pressed and does not move for any of them, which is indistinguishable
// from a box that has hung. So `esp_ota_begin` is handed
// `OTA_WITH_SEQUENTIAL_WRITES`, which erases nothing at all where it stands and
// leaves each region to the `esp_ota_write` that is about to fill it: the
// bytes are on the wire while the flash is being cleared for them, and the
// first of them is shown as soon as the network delivers it. The writes are a
// whole block at a time for a reason of the same kind — `esp_flash_erase_region`
// only reaches for the block command when the region it is given is an aligned
// block, and one 64 KB block erase costs about what sixteen 4 KB sector erases
// do, so a chunk cut to the socket's size would trade the wait it just lost for
// an install several times longer than it needs to be.
// =============================================================================

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
#include "esp_ota_ops.h"
#include "esp_partition.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"

#include "board_hal.h"

static const char *TAG = "board_http";

/** How long the whole request may take: resolve, connect, handshake and body together. */
#define HAL_HTTP_TIMEOUT_MS 8000

/** The worker's stack. The deep part is the handshake — mbedTLS's own figures put that in the 4-6 KB
 * range, and its 16 KB content buffers are heap (PSRAM), not stack. */
#define HAL_HTTP_TASK_STACK 8192

/** The socket's own read buffer. The body buffer beside it is the larger one, and is `board_hal.h`'s. */
#define HAL_HTTP_READ_CHUNK 2048

/** How much of an image is read, erased and written at a time — one whole erase block, for the reason
 * in the note at the top of this file. It is staged in a buffer of its own, allocated per request and
 * only for an image, because a buffer this size kept for the asking is a quarter of the PSRAM this
 * board has left. */
#define HAL_HTTP_OTA_CHUNK 65536

/** What this board calls itself to a server. Servers log it, and a few refuse a request without one. */
#define HAL_HTTP_USER_AGENT "HiveBox/1.0"

/** How many redirects one request follows before its last 3xx is handed back as the answer. The
 * number is only a place to stop: a chain that loops would otherwise eat the whole timeout. */
#define HAL_HTTP_MAX_REDIRECTS 4

// ---- State ------------------------------------------------------------------
typedef struct {
    int32_t state;
    int32_t fault;
    int32_t status;
    /** Length of the body in `s_body`, meaningful while the state is DONE. */
    size_t length;
    /** How many bytes of the body have landed, for the bar a page draws while it waits. Written by
     * the worker as the chunks arrive and read by `hal_http_get_progress`; it is a count of bytes
     * *given to the caller*, so a redirect's own page is not in it. */
    size_t received;
    /** What the server said the body would be — `Content-Length` — or 0 when it said nothing, which
     * is every chunked answer and every server that does not bother. See `HttpProgress`. */
    size_t total;
    /** A worker task is alive. Not the same as RUNNING: a cancel clears the state while the worker is
     * still closing its socket, and the slot is not free until that is done. */
    bool busy;
    /** Asked to stop since the worker started. The worker looks between chunks. */
    bool cancelled;
} http_snapshot_t;

static http_snapshot_t s_snap = { .state = HAL_HTTP_IDLE };

static SemaphoreHandle_t s_lock = NULL;

/** The one body buffer, out of PSRAM by preference: it is the largest thing this file owns and
 * nothing needs it fast. */
static uint8_t *s_body = NULL;

/** The URL the worker is fetching. Written under the lock, read by the worker while `busy`. */
static char s_url[HAL_HTTP_URL_MAX];

/** Where the answer goes: `HAL_HTTP_SINK_*`. Written under the lock, read by the worker while `busy`. */
static int32_t s_sink = HAL_HTTP_SINK_MEMORY;

/** The file a `HAL_HTTP_SINK_FILE` request writes. Same lifetime as `s_url`, and empty otherwise. */
static char s_path[HAL_HTTP_PATH_MAX];

/** Whether a status code is one an answer's body may be *installed* from. Every 2xx is: what a slot
 * must never be written from is a page that says the image is somewhere else. */
static bool status_is_success(int32_t status)
{
    return status >= 200 && status < 300;
}

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
// Helpers
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

/** Whether the URL is one we would hand to the TLS stack. */
static bool url_is_tls(const char *url)
{
    return strncmp(url, "https://", 8) == 0;
}

/** Whether the board knows what time it is. See the note at the top of this file. */
static bool clock_is_set(void)
{
    int64_t epoch = 0;
    return hal_system_get_epoch(&epoch) == ESP_OK;
}

static bool is_cancelled(void)
{
    lock();
    const bool cancelled = s_snap.cancelled;
    unlock();
    return cancelled;
}

/**
 * The fault an `esp_http_client` failure is, in the closed list the Rust side reads.
 *
 * `esp_http_client_*` answers with codes of its own, of esp-tls's, and of mbedTLS's, and a handshake
 * has a dozen ways to fail. Naming every one of them here would be a list to keep in step with a
 * library across versions, so the split is drawn where a page needs it: the two codes that mean
 * "nothing is listening at that name" are unreachable, a timeout is a timeout, and on an `https` URL
 * everything else that can go wrong before the first byte of the body counts as the handshake. What
 * that costs, when it is wrong, is the difference between "check the network" and "check the
 * certificate"; what a table that has drifted out of step with IDF costs is a build that stops.
 */
static int32_t fault_of(esp_err_t err, bool tls)
{
    switch (err) {
    case ESP_ERR_HTTP_CONNECT:
    case ESP_ERR_HTTP_FETCH_HEADER:
        return HAL_HTTP_FAULT_UNREACHABLE;
    case ESP_ERR_HTTP_READ_TIMEOUT:
    case ESP_ERR_TIMEOUT:
        return HAL_HTTP_FAULT_TIMEOUT;
    default:
        break;
    }

    return tls ? HAL_HTTP_FAULT_TLS : HAL_HTTP_FAULT_UNREACHABLE;
}

/** Record how the request ended, and let go of the slot. */
static void finish(int32_t fault, int32_t status, size_t length)
{
    lock();
    s_snap.fault = fault;
    s_snap.status = status;
    s_snap.length = length;
    s_snap.state = (fault == HAL_HTTP_FAULT_NONE) ? HAL_HTTP_DONE : HAL_HTTP_FAILED;
    s_snap.busy = false;
    unlock();
}

/** Let go of the slot without recording anything: all a cancelled request has to leave behind. */
static void release(void)
{
    lock();
    s_snap.busy = false;
    unlock();
}

/**
 * Records how far the body has got, for the bar a page draws while it waits.
 *
 * Taken and given back per chunk rather than held across the download: the writer is this task and
 * the reader is the frame loop, and a lock held for a minute of downloading would be the frame loop
 * stopped for one. `total` is passed in as well as `received` because the two are written together —
 * a reader that saw a new `received` beside a stale `total` would draw a bar past its own end.
 */
static void report_progress(size_t received, size_t total)
{
    lock();
    s_snap.received = received;
    s_snap.total = total;
    unlock();
}

// =============================================================================
// The worker
// =============================================================================

static void http_task(void *arg)
{
    (void)arg;

    const bool tls = url_is_tls(s_url);

    /* What the request came to, filled in by whichever way out it takes. `record` is false for a
     * cancel: the page has already cleared the state and there is nothing to add to it. */
    int32_t fault = HAL_HTTP_FAULT_NONE;
    int32_t status = 0;
    size_t used = 0;
    bool record = true;

    /* The file a `HAL_HTTP_SINK_FILE` request writes, declared up here and not at the point it is
     * opened: the ways out above that point jump to `out`, which closes it, and a pointer that was
     * never assigned would be closed and deleted there. Zero is "nothing open", which is what every
     * path that did not get as far as opening one leaves. */
    FILE *file = NULL;

    /* The same for the write an `HAL_HTTP_SINK_OTA` request makes: the slot it was aimed at and the
     * handle `esp_ota_write` takes, both up here because `out` has to drop a write that did not
     * finish. Zero is again "nothing began", and a slot that was not chosen is NULL. */
    esp_ota_handle_t ota = 0;
    const esp_partition_t *slot = NULL;

    /* And again for the block an image is staged in — see `HAL_HTTP_OTA_CHUNK`. It is allocated late,
     * once the request is known to be an image, and `out` frees it on every path, including the ones
     * that jump there before the allocation ever happened; NULL is what those leave and what `free`
     * takes. `ota_held` is how much of the block the reads have filled and the writes have not taken
     * yet, and it is up here for the same reason. */
    uint8_t *ota_chunk = NULL;
    size_t ota_held = 0;

    /* The certificate bundle in the image, and the reason HTTPS needs a clock: mbedTLS checks the
     * window on every certificate it is shown, so this only works once `hal_time_sync()` has run. */
    const esp_http_client_config_t cfg = {
        .url = s_url,
        .method = HTTP_METHOD_GET,
        .timeout_ms = HAL_HTTP_TIMEOUT_MS,
        .buffer_size = HAL_HTTP_READ_CHUNK,
        .buffer_size_tx = 512,
        .user_agent = HAL_HTTP_USER_AGENT,
        .crt_bundle_attach = esp_crt_bundle_attach,
        .keep_alive_enable = false,
    };

    esp_http_client_handle_t client = esp_http_client_init(&cfg);
    if (client == NULL) {
        ESP_LOGW(TAG, "esp_http_client_init failed");
        fault = HAL_HTTP_FAULT_UNREACHABLE;
        goto out;
    }

    /* A 3xx is followed here, because the library only follows one on the path this is not. The
     * config's auto-redirect reads as if that were handled, but `disable_auto_redirect` is consulted
     * inside `esp_http_client_perform()` — and that is the one call this request cannot use, since it
     * hands the whole body back at once, while this one writes a file as the bytes arrive and looks
     * for a cancel between chunks. Driven by hand, nothing looks at the status, so a redirect's own
     * page is what a caller reads: Gitee answers a raw path with a 302 to `raw.giteeusercontent.com`,
     * and its redirect page would be parsed as a manifest. `Location` is kept while the headers are
     * parsed, and `esp_http_client_set_redirection` turns it into the next URL — before the next
     * `open`, which walks `esp_http_client_prepare` and frees it. */
    for (int hop = 0; ; hop++) {
        esp_err_t err = esp_http_client_open(client, 0);
        if (err != ESP_OK) {
            ESP_LOGW(TAG, "open %s: %s", s_url, esp_err_to_name(err));
            /* No cleanup here: `out` closes and frees the client once. Cleaning up as well would free
             * it twice, and the second walk of the transport list calls whatever the freed handle now
             * holds — the reboot a failed HTTPS open used to end in. */
            fault = fault_of(err, tls);
            goto out;
        }

        /* Headers first, so the status code is known before the body is read: a 404 is an answer with
         * a body in it and not a fault, and a page gets to see both. */
        (void)esp_http_client_fetch_headers(client);
        status = (int32_t)esp_http_client_get_status_code(client);

        if (status < 300 || status >= 400) {
            /* Not a redirect: this is the answer, and what is read below is its body. */
            break;
        }
        if (hop >= HAL_HTTP_MAX_REDIRECTS) {
            ESP_LOGW(TAG, "over %d redirects: %s", HAL_HTTP_MAX_REDIRECTS, s_url);
            break;
        }
        if (esp_http_client_set_redirection(client) != ESP_OK) {
            /* No `Location` to go to, or one that leaves https. Either way the 3xx is all there is,
             * and handing it up beats answering with something invented. */
            ESP_LOGW(TAG, "%s -> %d, nowhere to follow", s_url, (int)status);
            break;
        }

        ESP_LOGI(TAG, "%s -> %d, following", s_url, (int)status);
        /* `close` puts the state back to INIT and drops the socket; the next `open` connects again —
         * to another host, which is what this one is. Nothing read the redirect's body: it is not the
         * answer, and `used` counts only what the caller is given. */
        esp_http_client_close(client);
    }

    /* What the server said the body would be, read once the final answer's headers are in hand. A
     * negative answer is "it did not say" — a chunked body, or a server that left the header out — and
     * 0 is what the progress reader gets then, which is its own way of saying "no percentage here". */
    const int64_t declared = esp_http_client_get_content_length(client);
    const size_t total = (declared > 0) ? (size_t)declared : 0;

    /* Where the bytes go. A file is opened before the first chunk, so that a path that cannot be
     * written is the request's own answer rather than a mystery at the end of a long download — and
     * the body of a `404` lands in it just as the body of a success does, because the status is the
     * answer's to carry and not this file's to judge. */
    const bool to_file = (s_sink == HAL_HTTP_SINK_FILE);
    const bool to_ota = (s_sink == HAL_HTTP_SINK_OTA);
    /* A file stages each chunk through the head of the body buffer: it is the read buffer as much as
     * it is the body buffer, and a file has no reason to keep the bytes twice. A slot keeps them in a
     * staging block of its own — see `HAL_HTTP_OTA_CHUNK` — and either way the answer is not bounded
     * by the body buffer, which is the question `staged` is asked below. */
    const bool staged = to_file || to_ota;

    /* A slot is the one place where the status *is* this file's to judge, and the reason is what
     * judging wrongly costs: a `404` page written into a slot erases an image to put a web page
     * where one used to be. So a body that is not a success is not read at all here — the status is
     * the whole of the answer, and the caller reads it as the fault it is. */
    if (to_ota && !status_is_success(status)) {
        ESP_LOGW(TAG, "%s -> %d, nothing written to a slot", s_url, (int)status);
        goto out;
    }

    if (to_file) {
        file = fopen(s_path, "wb");
        if (file == NULL) {
            ESP_LOGW(TAG, "cannot write %s", s_path);
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }
    }

    if (to_ota) {
        /* Which slot is not this file's to be told: the one the running image is not in, which
         * `esp_ota_ops` knows and a caller with a path in hand cannot say. */
        slot = esp_ota_get_next_update_partition(NULL);
        if (slot == NULL) {
            ESP_LOGW(TAG, "no app slot to install into");
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }

        /* A slot too small for the image is the one thing `OTA_WITH_SEQUENTIAL_WRITES` cannot answer
         * for us — nothing is erased or written where that call stands, so there is no earlier
         * moment at which it could have looked at a length — and it is worth answering here rather
         * than ninety block erases later, because "the repository publishes an image meant for a
         * board with twice this flash" is something a person can act on and a write that ran off the
         * end of a partition is not. A server that declared nothing cannot be asked at all; that
         * image is refused by the write that reaches the end of the slot, which is the same failure
         * one slow download later. */
        if (total > slot->size) {
            ESP_LOGW(TAG, "%s is %u bytes, %s holds %u", s_url, (unsigned)total, slot->label,
                     (unsigned)slot->size);
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }

        ota_chunk = (uint8_t *)heap_caps_malloc(HAL_HTTP_OTA_CHUNK, MALLOC_CAP_SPIRAM);
        if (ota_chunk == NULL) {
            /* The internal heap is the fallback and a worse fit for this than it is for the body
             * buffer: this allocation is made in the middle of the largest thing this board does. */
            ota_chunk = (uint8_t *)malloc(HAL_HTTP_OTA_CHUNK);
        }
        if (ota_chunk == NULL) {
            ESP_LOGW(TAG, "no room to stage %d bytes of an image", HAL_HTTP_OTA_CHUNK);
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }

        /* Nothing is erased by this call and nothing is written where it stands — see the note at the
         * top of this file. What it buys is the handle: the slot, and the rule that a region is
         * erased once, by the write that is about to fill it. */
        const esp_err_t began = esp_ota_begin(slot, OTA_WITH_SEQUENTIAL_WRITES, &ota);
        if (began != ESP_OK) {
            ESP_LOGW(TAG, "esp_ota_begin(%s): %s", slot->label, esp_err_to_name(began));
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }
    }

    /* Nothing has arrived yet, and the total is now known: the first reading a bar gets, and the one
     * that turns "downloading" into "downloading one of *this* many bytes". */
    report_progress(0, total);

    for (;;) {
        if (is_cancelled()) {
            ESP_LOGI(TAG, "cancelled: %s", s_url);
            record = false;
            goto out;
        }

        /* `used` is the answer's length either way, but only the buffered one is bounded by it: a file
         * and a slot have somewhere else to put the rest. */
        if (!staged && used >= HAL_HTTP_BODY_MAX - 1) {
            ESP_LOGW(TAG, "answer over %d bytes: %s", HAL_HTTP_BODY_MAX, s_url);
            fault = HAL_HTTP_FAULT_TOO_LARGE;
            goto out;
        }

        /* A staged chunk goes into the head of the same buffer, and only a document is accumulated
         * past it — see `staged` above. An image is the exception, and the erase is why: the block
         * the write below clears is the block this read fills, so the read is the size of a block
         * rather than the size of the socket's buffer, and the two are never cut to fit each other. */
        uint8_t *const into = to_ota ? (ota_chunk + ota_held) : (staged ? s_body : s_body + used);
        const int room = to_ota ? (int)(HAL_HTTP_OTA_CHUNK - ota_held)
                                : (int)(HAL_HTTP_BODY_MAX - 1 - (staged ? 0 : used));

        const int read = esp_http_client_read(client, (char *)into, room);
        if (read < 0) {
            ESP_LOGW(TAG, "read %s failed (%d)", s_url, read);
            fault = fault_of(ESP_FAIL, tls);
            goto out;
        }
        if (read == 0) {
            /* Nothing more, which is either the end of the body or a socket that went away in the
             * middle of one. The second must not be read as a short answer: half a JSON document
             * parses as nothing at all, and a page that drew it would be drawing its own guess. */
            if (esp_http_client_is_complete_data_received(client)) {
                break;
            }
            ESP_LOGW(TAG, "connection ended mid-body: %s", s_url);
            fault = HAL_HTTP_FAULT_UNREACHABLE;
            goto out;
        }

        if (to_file && fwrite(into, 1, (size_t)read, file) != (size_t)read) {
            ESP_LOGW(TAG, "write %s failed", s_path);
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }

        /* A block into the slot, once there is a whole one. A flash write that will not take is the
         * board's fault and not the network's — see `HAL_HTTP_FAULT_STORAGE` — and the way out drops
         * the write whole. */
        if (to_ota) {
            ota_held += (size_t)read;

            if (ota_held == HAL_HTTP_OTA_CHUNK) {
                if (esp_ota_write(ota, ota_chunk, ota_held) != ESP_OK) {
                    ESP_LOGW(TAG, "esp_ota_write into %s failed", slot->label);
                    fault = HAL_HTTP_FAULT_STORAGE;
                    goto out;
                }

                ota_held = 0;
            }
        }

        used += (size_t)read;

        /* Per chunk and not per frame: the frame loop reads this while the worker writes it, and a
         * reading taken here is one a bar can move on. */
        report_progress(used, total);
    }

    /* What is left of an image when the body ends part-way into a block: the tail of every image ever
     * published, since none of them is a multiple of the erase size. It is written here, as the one
     * write that is not a whole block, and its last sectors are erased one at a time by the same call
     * — a sector-erase tail, rather than the ninety sector-erase blocks that reading a chunk at a
     * time would have cost. */
    if (to_ota && ota_held > 0) {
        if (esp_ota_write(ota, ota_chunk, ota_held) != ESP_OK) {
            ESP_LOGW(TAG, "esp_ota_write into %s failed", slot->label);
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }

        ota_held = 0;
    }

    if (to_file) {
        /* Let go of the file before the way out, so that `out` does not read a good close as a reason
         * to delete a file that is whole. A close that *fails* is the opposite case — the bytes may
         * never have reached the card — so that one is removed here rather than left to be read. */
        FILE *const written = file;
        file = NULL;
        if (fclose(written) != 0) {
            ESP_LOGW(TAG, "close %s failed", s_path);
            remove(s_path);
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }
        ESP_LOGI(TAG, "%s -> %d, %u bytes to %s", s_url, (int)status, (unsigned)used, s_path);
    } else if (to_ota) {
        /* The image is whole, and whole is what makes a slot an *update*. `esp_ota_end` is what says
         * these bytes are an image this chip can start — a release built for another chip, or a body
         * that is a web page after all, is refused here rather than pointed at by the next boot — and
         * `esp_ota_set_boot_partition` is what points it there. Until both have said yes the slot
         * holds bytes and nothing more, and every way out of this function drops them. */
        const esp_ota_handle_t handle = ota;
        ota = 0;

        const esp_err_t ended = esp_ota_end(handle);
        if (ended != ESP_OK) {
            ESP_LOGW(TAG, "%s: not an image this board can start: %s", slot->label,
                     esp_err_to_name(ended));
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }

        const esp_err_t pointed = esp_ota_set_boot_partition(slot);
        if (pointed != ESP_OK) {
            ESP_LOGW(TAG, "esp_ota_set_boot_partition(%s): %s", slot->label,
                     esp_err_to_name(pointed));
            fault = HAL_HTTP_FAULT_STORAGE;
            goto out;
        }

        ESP_LOGI(TAG, "%s -> %d, %u bytes into %s, which the next boot starts from", s_url,
                 (int)status, (unsigned)used, slot->label);
    } else {
        /* A NUL just past the end, so that a `printf` of a body is not a buffer overrun. The Rust side
         * carries the length and does not depend on it. */
        s_body[used] = '\0';
        ESP_LOGI(TAG, "%s -> %d, %u bytes", s_url, (int)status, (unsigned)used);
    }

out:
    if (file != NULL) {
        /* Cancelled, or failed part way: half a glyph table reads as a font with some of its characters
         * missing, which is a failure that surfaces a long way from here. */
        fclose(file);
        remove(s_path);
    }

    if (ota != 0) {
        /* A slot with half an image in it, dropped rather than left in a state somebody might boot
         * from: the boot partition was never moved, so this is the box going on with what it has. The
         * next attempt erases the slot again on its way in. */
        esp_ota_abort(ota);
    }

    /* The staging block lives for the request and not for the sink, so that a board that never
     * installs anything never pays for one. */
    free(ota_chunk);

    if (client != NULL) {
        esp_http_client_close(client);
        esp_http_client_cleanup(client);
    }

    if (record) {
        finish(fault, status, used);
    } else {
        release();
    }

    vTaskDelete(NULL);
}

// =============================================================================
// Public API
// =============================================================================

esp_err_t hal_http_start_to(const char *url, int32_t sink, const char *path)
{
    if (url == NULL || url[0] == '\0') {
        return ESP_ERR_INVALID_ARG;
    }
    const size_t length = strlen(url);
    if (length >= HAL_HTTP_URL_MAX) {
        return ESP_ERR_INVALID_ARG;
    }
    if (strncmp(url, "http://", 7) != 0 && !url_is_tls(url)) {
        return ESP_ERR_INVALID_ARG;
    }

    /* The path is checked here rather than by `fopen` on the worker for the same reason the URL is: a
     * page that built an empty one has a bug, and "the card is full" is the wrong thing to tell it. */
    size_t path_length = 0;
    if (sink == HAL_HTTP_SINK_FILE) {
        if (path == NULL || path[0] == '\0') {
            return ESP_ERR_INVALID_ARG;
        }
        path_length = strlen(path);
        if (path_length >= HAL_HTTP_PATH_MAX) {
            return ESP_ERR_INVALID_ARG;
        }
    } else if (sink != HAL_HTTP_SINK_MEMORY && sink != HAL_HTTP_SINK_OTA) {
        /* `HAL_HTTP_SINK_OTA` takes no path: which slot an update goes into is this file's to pick a
         * moment from now, and a caller that passed one has the wrong idea about the sink. */
        return ESP_ERR_INVALID_ARG;
    }

    if (s_lock == NULL) {
        s_lock = xSemaphoreCreateMutex();
        if (s_lock == NULL) {
            return ESP_ERR_NO_MEM;
        }
    }
    if (s_body == NULL) {
        /* PSRAM by preference, internal heap as the fallback: 16 KB is a lot to take out of the pool
         * the Wi-Fi buffers live in, but a board without PSRAM should still be able to fetch a page. */
        s_body = (uint8_t *)heap_caps_malloc(HAL_HTTP_BODY_MAX, MALLOC_CAP_SPIRAM);
        if (s_body == NULL) {
            s_body = (uint8_t *)malloc(HAL_HTTP_BODY_MAX);
        }
        if (s_body == NULL) {
            ESP_LOGE(TAG, "no room for a %d-byte body buffer", HAL_HTTP_BODY_MAX);
            return ESP_ERR_NO_MEM;
        }
    }

    lock();
    if (s_snap.busy) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }

    memcpy(s_url, url, length + 1);
    s_sink = sink;
    if (sink == HAL_HTTP_SINK_FILE) {
        memcpy(s_path, path, path_length + 1);
    } else {
        s_path[0] = '\0';
    }

    /* The two refusals that need no network, answered before anything is sent. */
    int32_t fault = HAL_HTTP_FAULT_NONE;
    if (!network_is_up()) {
        fault = HAL_HTTP_FAULT_NO_NETWORK;
    } else if (url_is_tls(url) && !clock_is_set()) {
        fault = HAL_HTTP_FAULT_CLOCK;
    }

    if (fault != HAL_HTTP_FAULT_NONE) {
        s_snap.fault = fault;
        s_snap.status = 0;
        s_snap.length = 0;
        /* Nothing was asked for over the wire, so there is no progress to report: zeros rather than
         * the last request's counters, which a page would otherwise draw as its own bar. */
        s_snap.received = 0;
        s_snap.total = 0;
        s_snap.state = HAL_HTTP_FAILED;
        unlock();
        ESP_LOGW(TAG, "%s: %s", s_url,
                 (fault == HAL_HTTP_FAULT_NO_NETWORK) ? "not on a network" : "clock has never been set");
        return ESP_OK;
    }

    s_snap.state = HAL_HTTP_RUNNING;
    s_snap.fault = HAL_HTTP_FAULT_NONE;
    s_snap.status = 0;
    s_snap.length = 0;
    /* Before the worker starts, so that a page reading progress on the very next frame is told zero of
     * *this* request rather than the total of the last one. */
    s_snap.received = 0;
    s_snap.total = 0;
    s_snap.cancelled = false;
    s_snap.busy = true;
    unlock();

    if (xTaskCreatePinnedToCore(http_task, "board_http", HAL_HTTP_TASK_STACK, NULL, 4, NULL, 0) != pdPASS) {
        lock();
        s_snap.busy = false;
        s_snap.state = HAL_HTTP_IDLE;
        unlock();
        ESP_LOGE(TAG, "could not start the HTTP worker");
        return ESP_ERR_NO_MEM;
    }

    return ESP_OK;
}

int32_t hal_http_get_state(void)
{
    lock();
    const int32_t state = s_snap.state;
    unlock();
    return state;
}

int32_t hal_http_get_fault(void)
{
    lock();
    const int32_t fault = s_snap.fault;
    unlock();
    return fault;
}

esp_err_t hal_http_take(int32_t *out_status, const uint8_t **out_body, size_t *out_length)
{
    if (out_status == NULL || out_body == NULL || out_length == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    lock();
    if (s_snap.state != HAL_HTTP_DONE) {
        unlock();
        return ESP_ERR_INVALID_STATE;
    }

    *out_status = s_snap.status;
    /* A file's bytes went to the file as they arrived and a slot's went into flash, and neither is in
     * this buffer at all: the length still says how many there were, which is what a caller checking a
     * download against a manifest's size wants, and a NULL pointer is the honest answer for "there is
     * no buffer".
     *
     * `s_sink` and not a field of the snapshot: the sink is a property of the *request*, and it is
     * read here rather than copied into the snapshot because a request that had already been taken
     * would have cleared one and left the other saying where the bytes went. */
    *out_body = (s_sink == HAL_HTTP_SINK_MEMORY) ? s_body : NULL;
    *out_length = s_snap.length;

    /* Taken: the answer belongs to the caller now and the client is free for the next request. The
     * bytes stay where they are — see the header — because the caller copies them out of this call. */
    s_snap.state = HAL_HTTP_IDLE;
    s_snap.length = 0;
    unlock();

    return ESP_OK;
}

esp_err_t hal_http_cancel(void)
{
    lock();
    if (s_snap.busy) {
        /* The worker owns the socket and notices between chunks: closing it from here would be two
         * tasks on one handle. */
        s_snap.cancelled = true;
    }
    s_snap.state = HAL_HTTP_IDLE;
    s_snap.fault = HAL_HTTP_FAULT_NONE;
    s_snap.status = 0;
    s_snap.length = 0;
    s_snap.received = 0;
    s_snap.total = 0;
    unlock();

    return ESP_OK;
}

esp_err_t hal_http_get_progress(size_t *out_received, size_t *out_total)
{
    if (out_received == NULL || out_total == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    lock();
    const bool in_flight = (s_snap.state == HAL_HTTP_RUNNING);
    *out_received = s_snap.received;
    *out_total = s_snap.total;
    unlock();

    /* Only a *running* request has a progress worth drawing, and `INVALID_STATE` is that said out
     * loud: before one there are no counters, and once its answer has been taken the numbers are the
     * last request's leftovers. A page reading them as its own would draw a bar that had already
     * finished before it started. */
    return in_flight ? ESP_OK : ESP_ERR_INVALID_STATE;
}
