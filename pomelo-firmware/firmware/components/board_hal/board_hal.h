#ifndef BOARD_HAL_H
#define BOARD_HAL_H

#include <stdint.h>
#include <stdbool.h>
#include <time.h>
#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

/* ---------------------------------------------------------------------------
 * Display panel geometry (2.16-inch CO5300 AMOLED)
 * ------------------------------------------------------------------------- */
#define BOARD_DISPLAY_WIDTH   480
#define BOARD_DISPLAY_HEIGHT  480


/**
 * @brief Initialize AMOLED display and CST816 touch controller
 */
esp_err_t board_hal_init(void);

/**
 * @brief Tell the bootloader this image boots: cancel the rollback it is on probation for.
 *
 * Only means anything for an image that arrived over the air. `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE`
 * makes `esp_ota_set_boot_partition` leave the newly written slot in the "pending verify" state, and
 * the bootloader then boots it on the understanding that the image will confirm itself: without that
 * confirmation the next reset goes back to the slot the box came from. This is the confirmation.
 *
 * Called once, from `board_display.c`, when the first frame reaches the panel — the point at which
 * everything an update can break has run to the end: the board comes up, the fonts are where they
 * should be, the renderer draws, and the panel takes the frame. An image that faults before it gets
 * there never calls this, and is rolled back.
 *
 * A no-op for an image flashed over USB: the bootloader finds an empty `otadata` and marks such an
 * image valid itself, so this finds it already valid. Harmless with the rollback feature off, too,
 * where the state it would set is the one that never changes.
 *
 * @return
 *      - ESP_OK once the image is marked valid, or was already
 *      - ESP_ERR_NOT_SUPPORTED when the running partition is not one the OTA layer knows
 *      - what `esp_ota_mark_app_valid_cancel_rollback` gave, if it refused
 */
esp_err_t board_hal_confirm_boot(void);

/**
 * @brief Draw RGB565 bitmap to display bounding box [x1, y1) to [x2, y2).
 * Handles Big-Endian byte swap required by CO5300 and DMA chunking.
 *
 * Coordinates snap outward to even pixels (CO5300 requirement).
 */
void hal_display_draw_bitmap(int32_t x1, int32_t y1, int32_t x2, int32_t y2,
                            const uint16_t *pixels, int32_t stride);

/**
 * @brief Wait for display vertical synchronization (hardware TE interrupt or 60Hz frame pacing).
 */
void hal_display_wait_vsync(void);

/**
 * @brief Turn display panel power on or off (e.g. sleep/wake).
 */
void hal_display_set_power(bool on);

/** @brief Whether the panel is currently powered (the last state passed to hal_display_set_power). */
bool hal_display_is_powered(void);

/**
 * @brief Set panel brightness via the CO5300's 0x51 register.
 *
 * The level is remembered, so waking the display later does not force it back to full.
 * @param level 0 (off) to 255 (full).
 */
esp_err_t hal_display_set_brightness(uint8_t level);

/** @brief The brightness last set, or 255 before anything has been set. */
uint8_t hal_display_get_brightness(void);

/**
 * @brief Whether a screenshot can be taken.
 *
 * True once a frame has reached the panel: a capture is read out of the frame buffer that frame came
 * in, so before the first flush there is nothing to read.
 */
bool hal_display_has_capture(void);

/**
 * @brief Copy the screen into `out` as RGB565, top row first, `BOARD_DISPLAY_WIDTH` pixels wide.
 *
 * Reads the frame buffer the panel was last given, so the copy is paid by the caller instead of by
 * every frame. That buffer is the one the renderer draws into: called from a thread other than the
 * one flushing — which is what the HTTP handler does — a frame in progress can be read half-drawn.
 *
 * @param out    Buffer of at least `BOARD_DISPLAY_WIDTH * BOARD_DISPLAY_HEIGHT` pixels.
 * @param pixels Size of `out` in pixels, checked against the panel geometry.
 * @return ESP_OK, ESP_ERR_INVALID_STATE when no frame has been sent, or ESP_ERR_INVALID_SIZE.
 */
esp_err_t hal_display_capture(uint16_t *out, size_t pixels);

/**
 * @brief Poll touch point. Returns true if touched, false if released.
 */
bool hal_touch_get_point(int32_t *out_x, int32_t *out_y);

/**
 * @brief Touch event types for interrupt-driven event queues.
 */
typedef enum {
    HAL_TOUCH_EVENT_NONE = 0,
    HAL_TOUCH_EVENT_DOWN,
    HAL_TOUCH_EVENT_MOVE,
    HAL_TOUCH_EVENT_UP,
} hal_touch_event_type_t;

typedef struct {
    hal_touch_event_type_t type;
    int32_t x;
    int32_t y;
} hal_touch_event_t;

/**
 * @brief Wait for a touch event from the FreeRTOS event queue.
 *
 * Blocks the calling task for up to timeout_ms milliseconds (0 for non-blocking poll).
 * Returns true if an event was received, false on timeout.
 */
bool hal_touch_wait_event(hal_touch_event_t *out_event, uint32_t timeout_ms);

/**
 * @brief Feed a synthetic touch event in, as if a finger had produced it.
 *
 * Used by the web page's remote-control pad. Coordinates outside the panel are clamped.
 * @return ESP_OK, ESP_ERR_INVALID_STATE before touch is up, or ESP_ERR_TIMEOUT if the queue stayed
 *         full for 50 ms.
 */
esp_err_t hal_touch_inject(hal_touch_event_type_t type, int32_t x, int32_t y);

/**
 * @brief When the glass was last touched, in microseconds since boot, or 0 if never.
 *
 * A read-only timestamp for callers that need "has anything happened recently" without consuming an
 * event the UI is waiting on — the idle sleep is the one.
 */
int64_t hal_touch_last_activity_us(void);

/**
 * @brief Button event types for interrupt-driven event queues.
 */
typedef enum {
    HAL_BUTTON_EVENT_NONE = 0,
    HAL_BUTTON_EVENT_BOOT_PRESS,
    HAL_BUTTON_EVENT_BOOT_RELEASE,
    HAL_BUTTON_EVENT_PWR_PRESS,
    HAL_BUTTON_EVENT_PWR_RELEASE,
    HAL_BUTTON_EVENT_SIDE_PRESS,
    HAL_BUTTON_EVENT_SIDE_RELEASE,
} hal_button_event_t;

/* ---------------------------------------------------------------------------
 * Unified Hardware Event Queue
 * ------------------------------------------------------------------------- */

typedef enum {
    HAL_EVENT_NONE = 0,
    HAL_EVENT_BUTTON,
    HAL_EVENT_POWER,
    HAL_EVENT_WIFI,
} hal_event_type_t;

typedef enum {
    HAL_POWER_STATE_CHARGING_STARTED = 1,
    HAL_POWER_STATE_CHARGING_STOPPED,
    HAL_POWER_STATE_BATTERY_UPDATE,
    HAL_POWER_STATE_PEKEY_SHORT,
    HAL_POWER_STATE_PEKEY_LONG,
} hal_power_state_t;

typedef struct {
    hal_power_state_t state;
    int32_t           percent;
    int32_t           voltage_mv;
    bool              is_charging;
} hal_power_event_t;

typedef enum {
    HAL_WIFI_EVENT_CONNECTED = 1,
    HAL_WIFI_EVENT_DISCONNECTED,
    HAL_WIFI_EVENT_SCAN_DONE,
} hal_wifi_event_type_t;

typedef struct {
    hal_wifi_event_type_t type;
    int32_t               status;
} hal_wifi_event_t;

typedef struct {
    hal_event_type_t type;
    union {
        hal_button_event_t button;
        hal_power_event_t  power;
        hal_wifi_event_t   wifi;
    } data;
} hal_event_t;

esp_err_t hal_event_init(void);
bool hal_event_send(const hal_event_t *ev);
bool hal_event_send_from_isr(const hal_event_t *ev);
bool hal_event_wait(hal_event_t *out_event, uint32_t timeout_ms);

/* ---------------------------------------------------------------------------
 * AXP2101 PMIC power management.
 * ------------------------------------------------------------------------- */

/**
 * @brief Initialize AXP2101 PMIC power management.
 */
esp_err_t hal_power_init(void);

/**
 * @brief Get battery percentage (0-100), or -1 if battery is absent / unknown.
 */
int32_t hal_power_get_battery_percent(void);

/**
 * @brief Check if battery is currently being charged.
 */
bool hal_power_is_charging(void);

/**
 * @brief Get battery voltage in millivolts (mV), or 0 if unavailable.
 */
int32_t hal_power_get_battery_voltage_mv(void);

/**
 * @brief Read the PMIC's own die temperature, in tenths of a degree Celsius.
 *
 * The die, not the cell: this board carries a two-pin battery, so there is no NTC on the TS pin and
 * no pack temperature to read. hal_power_init() turns that channel off for exactly that reason, and
 * what is left on the power path is the AXP2101's own temperature.
 *
 * @param[out] out_dc  Temperature in 0.1 °C units (314 = 31.4 °C).
 *
 * @return
 *      - ESP_OK on success
 *      - ESP_ERR_INVALID_ARG if out_dc is NULL
 *      - ESP_ERR_INVALID_STATE if the PMIC has not been initialised
 *      - ESP_ERR_INVALID_RESPONSE when the ADC has not produced a reading yet
 */
esp_err_t hal_power_get_chip_temperature_dc(int32_t *out_dc);

/**
 * @brief Restart the board: reset the SoC from the application.
 *
 * A reset and not a power cycle — the PMIC keeps the rails up, and what the chip keeps across a
 * reset (the RTC in its own memory, the reset reason) survives.
 *
 * @note Does not return.
 */
void hal_power_restart(void);

/* ---------------------------------------------------------------------------
 * Wi-Fi station (esp_wifi).
 *
 * Thread-safe and non-blocking: long operations are started and then polled
 * via the scan-state / status getters. All state is cached by the event
 * handlers in board_wifi.c, so the Rust side never blocks on the driver.
 * ------------------------------------------------------------------------- */

#define HAL_WIFI_SCAN_IDLE      0
#define HAL_WIFI_SCAN_SCANNING  1
#define HAL_WIFI_SCAN_DONE      2
#define HAL_WIFI_SCAN_ERROR    -1

#define HAL_WIFI_STATE_DISCONNECTED 0
#define HAL_WIFI_STATE_SCANNING     1
#define HAL_WIFI_STATE_CONNECTING   2
#define HAL_WIFI_STATE_CONNECTED    3

#define HAL_WIFI_SSID_MAX_LEN 33
#define HAL_WIFI_IP_MAX_LEN   16

typedef struct {
    char    ssid[HAL_WIFI_SSID_MAX_LEN];
    int8_t  rssi;
    uint8_t channel;
    bool    secure;
} hal_wifi_ap_t;

typedef struct {
    int32_t state;                              /* HAL_WIFI_STATE_* */
    char    ssid[HAL_WIFI_SSID_MAX_LEN];        /* connected SSID, "" if none */
    char    ip[HAL_WIFI_IP_MAX_LEN];
    char    netmask[HAL_WIFI_IP_MAX_LEN];
    char    gateway[HAL_WIFI_IP_MAX_LEN];
    int8_t  rssi;
} hal_wifi_status_t;

/**
 * @brief Initialize the Wi-Fi station (NVS + netif + event loop + driver).
 *        The radio stays off until hal_wifi_set_enabled(true) is called.
 */
esp_err_t hal_wifi_init(void);

/** @brief Enable or disable the radio. Enabling starts the station. */
esp_err_t hal_wifi_set_enabled(bool on);
bool      hal_wifi_is_enabled(void);

/** @brief Begin an asynchronous scan; poll hal_wifi_scan_get_state(). */
esp_err_t hal_wifi_scan_start(void);
int32_t   hal_wifi_scan_get_state(void);

/**
 * @brief Copy up to max_count discovered APs into records.
 * @param[out] out_count  Number of records actually written.
 */
esp_err_t hal_wifi_scan_get_results(hal_wifi_ap_t *records, uint16_t max_count, uint16_t *out_count);

/** @brief Connect to an AP (password "" for open networks). Non-blocking. */
esp_err_t hal_wifi_connect(const char *ssid, const char *password);
esp_err_t hal_wifi_disconnect(void);

/** @brief Snapshot the current connection status. */
esp_err_t hal_wifi_get_status(hal_wifi_status_t *out_status);

/* ---------------------------------------------------------------------------
 * Network time (SNTP).
 *
 * The board's clock is the hardware RTC's (`hal_system_get_epoch`), and this is
 * what puts a real time into it: `board_time.c` starts lwIP's SNTP client, the
 * server answers, and the sync reaches the chip through `settimeofday` — which
 * `board_rtc.c` wraps for exactly this, so the time also survives the next power
 * cycle.
 *
 * It is here, beside Wi-Fi, because an address is what it needs and joining a
 * network is the moment it becomes possible. Nothing reads time through this
 * file: `hal_system_get_epoch` is the reading, and this is only what fills it.
 * ------------------------------------------------------------------------- */

/** @brief Start the SNTP client. Non-blocking, idempotent, safe from an event handler.
 *
 * Called by `board_wifi.c` from its `IP_EVENT_STA_GOT_IP` handler. The client then runs on a task of
 * its own and retries on its own; the time it finds lands in the RTC by itself.
 *
 * @return
 *      - ESP_OK once the client is running, or was already
 *      - ESP_ERR_INVALID_STATE before the Wi-Fi stack is up
 *      - what `esp_netif_sntp_init` said, if it refused
 */
esp_err_t hal_time_sync(void);

/* ---------------------------------------------------------------------------
 * HTTP client (`esp_http_client` + ESP-TLS).
 *
 * The board as a client of somebody else's service, for the Rust `HttpBackend`:
 * a request is started and then polled, on the same shape as the Wi-Fi scan.
 * `board_http.c` owns the one worker task and the one body buffer; this is the
 * whole of what the Rust side sees of it.
 *
 * HTTPS is verified against the root bundle in the image, which means the clock
 * has to be right — see `HAL_HTTP_FAULT_CLOCK` and `hal_time_sync()` above.
 * ------------------------------------------------------------------------- */

/** Most of a body this board will hold, in bytes. Kept in step with `HttpResponse::MAX_BODY_BYTES`. */
#define HAL_HTTP_BODY_MAX 16384

/** Longest URL accepted, including its terminator. */
#define HAL_HTTP_URL_MAX  512

/** Longest file path accepted by `HAL_HTTP_SINK_FILE`, including its terminator. */
#define HAL_HTTP_PATH_MAX 512

#define HAL_HTTP_IDLE     0
#define HAL_HTTP_RUNNING  1
#define HAL_HTTP_DONE     2
#define HAL_HTTP_FAILED  -1

/** Where an answer's bytes go as they arrive, as `Sink` mirrors. */
#define HAL_HTTP_SINK_MEMORY 0
#define HAL_HTTP_SINK_FILE   1
#define HAL_HTTP_SINK_OTA    2

/** Why a request did not come back, as the closed list `HttpFault` mirrors. */
#define HAL_HTTP_FAULT_NONE        0
#define HAL_HTTP_FAULT_NO_NETWORK  1
#define HAL_HTTP_FAULT_CLOCK       2
#define HAL_HTTP_FAULT_UNREACHABLE 3
#define HAL_HTTP_FAULT_TIMEOUT     4
#define HAL_HTTP_FAULT_TLS         5
#define HAL_HTTP_FAULT_TOO_LARGE   6
#define HAL_HTTP_FAULT_STORAGE     7

/** @brief Ask for a URL, with the answer going to `sink`. Returns at once; the answer arrives when it
 * arrives.
 *
 * A `GET`, and the URL is the whole of the request. `HAL_HTTP_SINK_MEMORY` reads the body into the one
 * buffer this file owns, where `HAL_HTTP_BODY_MAX` is the bound and an answer over it is
 * `HAL_HTTP_FAULT_TOO_LARGE`; that is the whole of a document. `HAL_HTTP_SINK_FILE` streams it into
 * `path` as it arrives, so the answer may be any size the filesystem has room for — a glyph table or a
 * font, which is a megabyte and more — and nothing is left behind if it does not finish: a cancelled
 * or failed download removes what it wrote.
 *
 * `HAL_HTTP_SINK_OTA` streams it into *flash* instead, as a firmware image: the bytes go through
 * `esp_ota_write` into the app slot `esp_ota_get_next_update_partition` picks — the one this image is
 * not running from, which is why no path is taken and why the caller cannot aim an update at the slot
 * in use. A slot is only made bootable by a request that finishes: `esp_ota_end` validates the image
 * and `esp_ota_set_boot_partition` is what points the next boot at it, and a request that fails or is
 * cancelled calls `esp_ota_abort`, which leaves the boot partition pointing where it pointed — the box
 * goes on starting from what it started from. An answer that is not a 2xx is never written at all, so
 * a `404` page cannot land in a slot. Failures here — no slot, a write that would not take, a body
 * that is not an image this chip can boot, a slot without room — are all `HAL_HTTP_FAULT_STORAGE`,
 * since all of them are "the answer could not be written where it was asked to go" and none is the
 * network's.
 *
 * Nothing is sent until the worker task runs, so a URL that is obviously unfetchable is refused here
 * rather than reported as a network fault: an argument error, a URL that does not fit, and a request
 * already in flight are all answers from this call. A board that is off the network, or an `https` URL
 * on a board whose clock has never been set, are *not* — those are the request's own answer, and they
 * are waiting in `hal_http_get_state()` before the first byte of anything is sent.
 *
 * @param url  The URL to fetch.
 * @param sink `HAL_HTTP_SINK_MEMORY`, `HAL_HTTP_SINK_FILE` to write `path`, or `HAL_HTTP_SINK_OTA` to
 *             install an image into the app slot that is not in use.
 * @param path The file to write when the sink is `HAL_HTTP_SINK_FILE`; ignored otherwise. Its
 *             directories must already exist — this opens a file, and making the path to it is the
 *             caller's.
 *
 * @return
 *      - ESP_OK when the request has been taken (including the two faults above)
 *      - ESP_ERR_INVALID_ARG for NULL, "", a URL over `HAL_HTTP_URL_MAX`, a scheme that is neither
 *        `http` nor `https`, a sink that is none of the three, or a `HAL_HTTP_SINK_FILE` request with
 *        no path or one over `HAL_HTTP_PATH_MAX`
 *      - ESP_ERR_INVALID_STATE while another request is in flight
 *      - ESP_ERR_NO_MEM when the worker task could not be created
 */
esp_err_t hal_http_start_to(const char *url, int32_t sink, const char *path);

/** @brief How the request is going: `HAL_HTTP_*`. Cheap and safe to call any time. */
int32_t hal_http_get_state(void);

/** @brief Why it failed: `HAL_HTTP_FAULT_*`. Meaningful while the state is `HAL_HTTP_FAILED`. */
int32_t hal_http_get_fault(void);

/** @brief How many bytes of the body have arrived, and how many there will be.
 *
 * For the bar a page draws while it waits: a download is a minute of somebody watching, and the only
 * thing that says the box has not hung is a number that moves. `*out_total` is what the server put in
 * `Content-Length`, and 0 when it said nothing — a chunked answer has no length until it is over —
 * so a caller with a size of its own (a manifest that already said how big the file is) uses that
 * instead and reads a 0 here as "not a percentage".
 *
 * Both numbers are of *this* request and are cleared when one starts or is cancelled. A redirect's
 * own page is not in them: what is counted is what the caller is given.
 *
 * @param[out] out_received How many bytes of the body have been handed to the caller so far.
 * @param[out] out_total    The body's declared length, or 0 when the server did not declare one.
 *
 * @return
 *      - ESP_OK while a request is running, which is the only time the numbers mean anything
 *      - ESP_ERR_INVALID_ARG for NULL out-parameters
 *      - ESP_ERR_INVALID_STATE when nothing is in flight: before a request, and again once its
 *        answer has been taken or cancelled — the numbers are leftovers then, and a page that drew
 *        them would draw a bar that had already finished
 */
esp_err_t hal_http_get_progress(size_t *out_received, size_t *out_total);

/** @brief The answer of the request that just finished, and clear it.
 *
 * Hands back a pointer *into the client's own buffer* rather than a copy, so the bytes are valid until
 * the next `hal_http_start_to` and the caller must read them before asking for anything else. A
 * `HAL_HTTP_SINK_FILE` answer is *not* in that buffer — its bytes are in the file the sink named, and
 * have been since they arrived — so `out_body` comes back NULL and only `out_length` says how much
 * there was.
 *
 * @param[out] out_status Receives the status code the server wrote.
 * @param[out] out_body   Receives a pointer to the body, not NUL-terminated as a document but with a
 *                        NUL just past its end, so a `printf` of it is not a buffer overrun. NULL for a
 *                        file sink, whose body is in the file.
 * @param[out] out_length Receives the body's length in bytes, whether it went into the buffer or into a
 *                        file.
 *
 * @return
 *      - ESP_OK
 *      - ESP_ERR_INVALID_ARG for NULL out-parameters
 *      - ESP_ERR_INVALID_STATE when there is nothing to take: the request is still running, it
 *        failed, or its answer has already been read
 */
esp_err_t hal_http_take(int32_t *out_status, const uint8_t **out_body, size_t *out_length);

/** @brief Abandon the request in flight, or drop the answer waiting to be taken.
 *
 * The state is `HAL_HTTP_IDLE` afterwards either way, and cancelling nothing is ESP_OK. A worker that
 * is mid-read notices at the next chunk rather than instantly — it is the one holding the socket — so
 * a request started immediately after a cancel can be refused for the remainder of that chunk.
 */
esp_err_t hal_http_cancel(void);

/* ---------------------------------------------------------------------------
 * Storage: the built-in `internal` LittleFS partition and the microSD slot.
 *
 * The card is mounted on demand rather than assumed. This board wires its
 * microSD slot to SDMMC in 1-bit mode with no card-detect pin, so the only way
 * to find out whether a card is in it is to try — hal_storage_refresh() is that
 * try, and it is meant to be called when the answer matters (a settings page
 * opening, a finger on "check again"), not once a frame.
 * ------------------------------------------------------------------------- */

#define HAL_STORAGE_MOUNT_POINT_MAX_LEN 32
#define HAL_STORAGE_FILESYSTEM_MAX_LEN  16

typedef struct {
    char     mount_point[HAL_STORAGE_MOUNT_POINT_MAX_LEN];
    char     filesystem[HAL_STORAGE_FILESYSTEM_MAX_LEN];
    uint64_t total_bytes;
    uint64_t free_bytes;
} hal_storage_volume_t;

/**
 * @brief Mount a card if one is in the slot, and drop one that has left.
 *
 * @return
 *      - ESP_OK on success (a card is mounted afterwards)
 *      - ESP_ERR_NOT_FOUND when the slot is empty, which is not a failure
 *      - other error codes from SDMMC or the FATFS driver
 */
esp_err_t hal_storage_refresh(void);

/**
 * @brief Read the built-in `internal` partition.
 *
 * @return ESP_OK, or the reason LittleFS gave — ESP_ERR_INVALID_STATE when the
 *         partition is not mounted (the firmware mounts it at boot).
 */
esp_err_t hal_storage_get_internal(hal_storage_volume_t *out);

/**
 * @brief Read the card in the slot.
 *
 * @return ESP_OK, or ESP_ERR_NOT_FOUND when nothing is mounted there.
 */
esp_err_t hal_storage_get_card(hal_storage_volume_t *out);

/* Files on the built-in partition, through `stdio`.
 *
 * These exist because the Rust half's `std::fs` cannot create a file on this board — it opens an
 * existing one for reading happily and fails every write with ENOENT — which made the Wi-Fi
 * credentials look saved and never be. See the note above them in `board_storage.c` for what was
 * measured. They are `fopen`/`fwrite`, the same calls `main.c` has always written its files with. */

/**
 * @brief Read a whole file as text, NUL-terminated.
 *
 * @param[in]  path      Path under a mount point, e.g. "/internal/AppData/WIFI/wifi.conf".
 * @param[out] out       Buffer of at least `capacity` bytes to fill.
 * @param[in]  capacity  Size of `out`. One byte is held back for the terminator.
 * @param[out] out_len   Bytes read, not counting the terminator.
 *
 * @return
 *      - ESP_OK on success
 *      - ESP_ERR_INVALID_ARG if any pointer is NULL or `capacity` is 0
 *      - ESP_ERR_NOT_FOUND when there is no such file — the ordinary "nothing has been saved yet"
 *      - ESP_ERR_INVALID_SIZE when the file does not fit, in which case `out` is not touched
 *      - ESP_FAIL for anything else the filesystem said
 */
esp_err_t hal_storage_read_file(const char *path, char *out, size_t capacity, size_t *out_len);

/**
 * @brief Write `len` bytes to `path`, truncating it, creating the directories above it.
 *
 * @return
 *      - ESP_OK on success, with every byte written and the file closed cleanly
 *      - ESP_ERR_INVALID_ARG if `path` or `data` is NULL
 *      - ESP_ERR_INVALID_SIZE if the path is longer than this layer will build
 *      - ESP_FAIL on a short write, a failed close, or any filesystem error
 */
esp_err_t hal_storage_write_file(const char *path, const char *data, size_t len);

/**
 * @brief Append `len` bytes to `path`, creating it and the directories above it when they are not there.
 *
 * The same write as `hal_storage_write_file` except that whatever is already in the file stays where
 * it is. Used by the page's NAS form, whose file is a *list* of machines and whose one entry belongs
 * after the entries already in it.
 *
 * @return
 *      - ESP_OK on success, with every byte appended and the file closed cleanly
 *      - ESP_ERR_INVALID_ARG if `path` or `data` is NULL
 *      - ESP_ERR_INVALID_SIZE if the path is longer than this layer will build
 *      - ESP_FAIL on a short write, a failed close, or any filesystem error
 */
esp_err_t hal_storage_append_file(const char *path, const char *data, size_t len);

/**
 * @brief Remove `path`.
 *
 * @return ESP_OK when it is gone, including when it never existed; ESP_ERR_INVALID_ARG if `path` is
 *         NULL; ESP_FAIL if the filesystem refused.
 */
esp_err_t hal_storage_remove_file(const char *path);

/**
 * @brief Move `from` to `to`, replacing `to` when there is something there.
 *
 * Within one filesystem this is `rename`, and the bytes are not read. Between the built-in partition
 * and the card it is a copy and a delete — two drivers with no inode between them — and the copy is
 * removed if it did not finish, so the destination's name never holds half a file.
 *
 * @return
 *      - ESP_OK on success, with the file at `to` and nothing at `from`
 *      - ESP_ERR_INVALID_ARG if either path is NULL
 *      - ESP_ERR_NOT_FOUND when there is nothing at `from`
 *      - ESP_ERR_NO_MEM when the copy between volumes could not be buffered
 *      - ESP_FAIL on a failed copy, a failed close, or an original that could not be removed
 */
esp_err_t hal_storage_rename(const char *from, const char *to);

/* The flash chip itself: how much of it there is, and what every region of it is for.
 *
 * hal_storage_get_internal() answers "how full is the filesystem the box can write" — on this board
 * 3.88 MB of a 16 MB chip. This answers the question under it: where the other 12.1 MB went. A page
 * that draws only the first is drawing a quarter of the answer.
 *
 * The regions tile the whole chip: the bootloader and the partition table below the first partition,
 * and the stretch past the last one, are reported rather than left out, so a bar drawn from this adds
 * up to the flash. */

#define HAL_FLASH_MAX_REGIONS   16
#define HAL_FLASH_LABEL_MAX_LEN 16

typedef enum {
    HAL_FLASH_REGION_SYSTEM = 0,  /* the bootloader, the partition table, NVS, PHY: reserved */
    HAL_FLASH_REGION_FIRMWARE,    /* an app partition: read-only while the box is running */
    HAL_FLASH_REGION_DATA,        /* a partition with a filesystem on it: writable */
    HAL_FLASH_REGION_UNALLOCATED, /* no partition claims it */
} hal_flash_region_kind_t;

typedef struct {
    char     label[HAL_FLASH_LABEL_MAX_LEN];
    uint8_t  kind; /* hal_flash_region_kind_t */
    uint8_t  _reserved[3];
    uint32_t size;
} hal_flash_region_t;

typedef struct {
    uint32_t           total_bytes;
    uint32_t           count;
    hal_flash_region_t regions[HAL_FLASH_MAX_REGIONS];
} hal_flash_layout_t;

/**
 * @brief Read the flash chip's layout: its total size, and every region of it in address order.
 *
 * @return
 *      - ESP_OK on success
 *      - ESP_ERR_INVALID_ARG if out is NULL
 *      - ESP_ERR_NOT_FOUND when the chip has no partition table to read
 *      - other error codes from the flash driver
 */
esp_err_t hal_storage_get_flash(hal_flash_layout_t *out);

/* ---------------------------------------------------------------------------
 * System information (`board_system.c`).
 *
 * What the machine *is*, as opposed to what it is doing with a device: the chip's
 * own account of itself, the image's description of itself, and the two numbers
 * that move while it runs. None of it is a peripheral — there is nothing to bring
 * up and nothing to poll — so every call here is a read that returns.
 *
 * The buffer sizes below are a promise in two languages: the arrays here and the
 * ones in `pomelo-hal-esp32/src/system.rs` have to agree. Every one of them is
 * filled by copying at most `size - 1` bytes into a zeroed struct, so a name that
 * stops fitting is a name that gets cut short, not a buffer that overruns — and
 * the sizes are generous because of it.
 * ------------------------------------------------------------------------- */

#define HAL_SYSTEM_CHIP_MODEL_MAX_LEN    16
#define HAL_SYSTEM_FIRMWARE_NAME_MAX_LEN 24
/* Exactly `esp_app_desc_t`'s own `version` field, so that what crosses the FFI is
 * what the image has and not a second opinion about it. The same 32 bytes are why the
 * spelling that goes *into* the image has to fit them: see the firmware's
 * `CMakeLists.txt`, which sets `PROJECT_VER` to a spelling that does. */
#define HAL_SYSTEM_VERSION_MAX_LEN       32
/* Exactly `esp_app_desc_t`'s own two fields, so the build stamp crosses the FFI
 * in the spelling the compiler left it in rather than a copy this header made up. */
#define HAL_SYSTEM_BUILD_MAX_LEN         16

typedef struct {
    char     model[HAL_SYSTEM_CHIP_MODEL_MAX_LEN];
    uint8_t  cores;
    uint16_t revision;
} hal_system_chip_t;

typedef struct {
    char name[HAL_SYSTEM_FIRMWARE_NAME_MAX_LEN];
    char version[HAL_SYSTEM_VERSION_MAX_LEN];
    /* When this image was built, in the compiler's spelling: `date` is `"Oct 10 2026"`
     * and `time` is `"14:32:05"`. Two strings and not one instant, because that is
     * what a C image has room for — the reader on the Rust side is what turns the
     * pair into the one spelling the interface writes instants in, and it is the
     * reader's business because it is the only side that knows what a locale is. */
    char date[HAL_SYSTEM_BUILD_MAX_LEN];
    char time[HAL_SYSTEM_BUILD_MAX_LEN];
} hal_system_firmware_t;

typedef struct {
    uint64_t total_bytes;
    uint64_t free_bytes;
} hal_system_memory_t;

/**
 * @brief Read the chip: the part number it reports, its cores and its revision.
 *
 * @return
 *      - ESP_OK on success
 *      - ESP_ERR_INVALID_ARG if out is NULL
 */
esp_err_t hal_system_get_chip(hal_system_chip_t *out);

/**
 * @brief Read the running image's own description: its name, its version, and when it
 *        was built.
 *
 * These are the strings the build stamped into the image, so a page showing them
 * is showing what is actually running rather than a literal someone typed. The
 * stamp is read the same way and for the same reason: a page saying when the
 * firmware was built is saying when *this* firmware was built, and it is wrong
 * only for as long as it takes to flash another one.
 *
 * @return
 *      - ESP_OK on success
 *      - ESP_ERR_INVALID_ARG if out is NULL
 *      - ESP_ERR_NOT_FOUND when the image carries no description at all
 */
esp_err_t hal_system_get_firmware(hal_system_firmware_t *out);

/**
 * @brief Microseconds since the board booted.
 *
 * `int64_t`, the same type `esp_timer_get_time` reports, so the value crosses the
 * FFI without a cast to disagree about.
 */
int64_t hal_system_get_uptime_us(void);

/**
 * @brief Read the heap: every byte of it, and what is left.
 *
 * The *default* heap — what `malloc()` without a capability hands out — and not
 * the 8 MB of PSRAM beside it. PSRAM is only ever given to a caller that asks for
 * it by name, so folding it into this total would describe a pool the kernel's own
 * allocations never draw on, and would report a board that is nearly out of memory
 * as one with two thirds free. See the note in `board_system.c`.
 *
 * Fragmented space counts as free: the allocator can hand it out.
 *
 * @return
 *      - ESP_OK on success
 *      - ESP_ERR_INVALID_ARG if out is NULL
 */
esp_err_t hal_system_get_memory(hal_system_memory_t *out);

#define HAL_SYSTEM_TASK_NAME_MAX_LEN 16
/* Enough for every task this image has been seen running, with room to spare. The array
 * crosses the FFI, so it wants to be a size both sides agree on rather than a number that
 * grows when a driver starts one more service task — tasks past this many are not listed,
 * which is what `count` is for. */
#define HAL_SYSTEM_MAX_TASKS 32

typedef struct {
    char     name[HAL_SYSTEM_TASK_NAME_MAX_LEN];
    /* Maps to the Rust `TaskState`: 0 running or ready, 1 blocked, 2 suspended, 3 deleted. */
    uint8_t  state;
    uint8_t  priority;
    /* 0xff for a task the scheduler may run on either core — and for an image built
     * without `CONFIG_FREERTOS_VTASKLIST_INCLUDE_COREID`, which is the config that
     * reports one at all. */
    uint8_t  core;
    uint8_t  current;
    /* The least stack this task has ever had left, in bytes: the number worth looking at,
     * because the day it reaches zero the task faults. */
    uint32_t stack_free_bytes;
    /* How much of the scheduler's run time counter this task has used, in microseconds.
     *
     * Zero is what an image built without `CONFIG_FREERTOS_GENERATE_RUN_TIME_STATS`
     * reports for every task, and it is also the honest reading for a task that has not
     * run yet — the two are one value here on purpose, because neither is a task with a
     * share of the time to report. `hal_system_tasks_t::total_run_time_us` is what tells
     * the two apart for a whole reading at once: a total above zero makes every task's
     * zero a task that has not run, and a total of zero means there is no count behind
     * any of the rows. */
    uint64_t run_time_us;
} hal_system_task_t;

typedef struct {
    uint32_t          count;
    hal_system_task_t tasks[HAL_SYSTEM_MAX_TASKS];
    /* Every task's `run_time_us` added up: the whole of the counter since the scheduler
     * started, and the denominator a share of it is taken against. See
     * `hal_system_task_t::run_time_us`.
     *
     * A number beside the list rather than something the other side adds up, and the
     * difference is not book-keeping: the tasks past `HAL_SYSTEM_MAX_TASKS` are not in
     * the array, so a total summed from `count` of them is short by whatever they used,
     * and every percentage taken against it is wrong by that much. This is the
     * scheduler's own total. */
    uint64_t total_run_time_us;
} hal_system_tasks_t;

/**
 * @brief Read the tasks the scheduler is running.
 *
 * What an `htop`-style list is made of. The *apps* this interface hosts are not tasks —
 * they are structs the launcher owns — while these are what the scheduler switches
 * between, with stacks that can run out and priorities that decide who goes next.
 *
 * Expensive compared to the other reads here: the scheduler is briefly held still while
 * the task list is walked, so this is the read a page wants once a second rather than
 * every frame.
 *
 * @return
 *      - ESP_OK on success, with `count` tasks filled in
 *      - ESP_ERR_INVALID_ARG if out is NULL
 *
 * An image built without `CONFIG_FREERTOS_USE_TRACE_FACILITY` has no per-task read to
 * make and answers with `count == 0`: an empty list rather than an invented one.
 *
 * One built without `CONFIG_FREERTOS_GENERATE_RUN_TIME_STATS` answers with the list all
 * the same and leaves every `run_time_us` and `total_run_time_us` at zero: the columns
 * that need the counter are the ones this read cannot answer, and they are worth an
 * empty reading rather than no reading of the other four.
 */
esp_err_t hal_system_get_tasks(hal_system_tasks_t *out);

/**
 * @brief Read the external RAM (PSRAM), when the board has any.
 *
 * The pool the framebuffer and the log ring come out of, reported separately from the
 * default heap and never summed with it — see the note on `hal_system_get_memory`, which
 * says the same thing from the other side.
 *
 * @return
 *      - ESP_OK on success
 *      - ESP_ERR_INVALID_ARG if out is NULL
 *      - ESP_ERR_NOT_FOUND when the board has no external RAM
 */
esp_err_t hal_system_get_psram(hal_system_memory_t *out);

/**
 * @brief Read the board's clock as a Unix epoch.
 *
 * The hardware RTC's time, which the firmware restores into the POSIX system clock
 * at boot and writes back whenever the system clock is set (`board_rtc.c` wraps
 * `settimeofday` for exactly that), so the two are one clock and not two.
 *
 * @param out_epoch Receives the chip's time, which is UTC.
 *
 * @return
 *      - ESP_OK on success
 *      - ESP_ERR_INVALID_ARG if out is NULL
 *      - ESP_ERR_INVALID_STATE when the board has no time yet — the chip came up
 *        holding nothing sane and nothing has set it since
 */
esp_err_t hal_system_get_epoch(int64_t *out_epoch);

/* ---------------------------------------------------------------------------
 * Web management server (`esp_http_server`).
 *
 * Started and stopped by the app of the same name and by nothing else: the box carries no server until
 * a finger asks for one. `board_web.c` holds the page and the routes; this is the whole of what the
 * Rust side sees of it.
 *
 * There is no authentication. The server is bound to every interface and the page it serves can read
 * and write the filesystem and change the network — see the note at the top of `board_web.c`, which is
 * the reason it is off by default rather than the reason it is missing a password.
 * ------------------------------------------------------------------------- */

/** @brief Bring the server up on `port`, registering the page and its API.
 *
 * Idempotent: already up on the same port is ESP_OK and nothing is restarted. A different port stops
 * the old server and starts the new one.
 *
 * @return ESP_OK, ESP_ERR_INVALID_ARG for port 0, or what `httpd_start` / `httpd_register_uri_handler`
 *         said.
 */
esp_err_t hal_web_start(uint16_t port);

/** @brief Take the server down, closing every socket it owns. Down already is ESP_OK. */
esp_err_t hal_web_stop(void);

/** @brief Whether the server is up. Safe to call from any task. */
bool hal_web_is_running(void);

/** @brief The port it is up on, or 0 when it is down. */
uint16_t hal_web_get_port(void);

/** @brief Name the app the box is showing, for the page's status panel.
 *
 * The page is served by C and the foreground is decided by Rust, so this is the one thing the server
 * cannot look up: the launcher says what it drew. Called on every switch; the name is copied, so
 * `name` does not have to outlive the call. Passing NULL or "" leaves it as the firmware's name.
 */
void hal_web_set_app_name(const char *name);

/* ---------------------------------------------------------------------------
 * Cloud servers (Tencent Cloud).
 *
 * The board's second outbound client, and deliberately not the same shape as the
 * first: a cloud call is a POST with a JSON body signed by a SecretKey, so this
 * is its own entry point rather than another argument on `hal_http_*`.
 * `board_cloud.c` has the whole of why.
 *
 * Two providers, four actions between them:
 *
 *   HAL_CLOUD_CVM         腾讯云 · 云服务器          (ins-…)
 *   HAL_CLOUD_LIGHTHOUSE  腾讯云 · 轻量应用服务器    (lhins-…)
 *   HAL_CLOUD_ALL         两家的并集，只用于 LIST —— 一次问你存了密钥的每一家
 *
 *   HAL_CLOUD_ACTION_LIST    what the key owns
 *   HAL_CLOUD_ACTION_START  power on
 *   HAL_CLOUD_ACTION_STOP   power off
 *   HAL_CLOUD_ACTION_REBOOT
 *
 * Asynchronous, in the shape of `hal_http_*`: `hal_cloud_start` returns before
 * anything has been sent, the state is polled, and the answer is one JSON
 * document read with `hal_cloud_answer` and released with `hal_cloud_take`.
 * The document is the same one the page's cloud routes hand to a browser, so
 * both surfaces read the same words:
 *
 *   {"ok":1,"provider":"cvm","region":"ap-guangzhou","count":1,
 *    "instances":[{"id":"ins-…","name":"…","state":"RUNNING","ip":"1.2.3.4","kind":"cvm"}]}
 *   {"ok":1,"provider":"all","region":"","count":2,
 *    "instances":[{"id":"lhins-…",…,"kind":"lighthouse"},{"id":"ins-…",…,"kind":"cvm"}],
 *    "failed":["lighthouse"]}                        -- 这一家的这次没取到
 *   {"ok":1,"provider":"cvm","action":"stop","request_id":"…"}
 *   {"error":"cloud_clock"}                          -- one of the faults below
 *   {"error":"cloud_api","code":"…","message":"…"}   -- the API refused
 *
 * `kind` is on every row, including the single-provider answers: a merged list
 * is a list of machines from two APIs, and a button has to know which API to
 * send `StartInstances` to. `HAL_CLOUD_ALL` is not a key's owner — the key table
 * is still two entries wide (`HAL_CLOUD_PROVIDER_COUNT`).
 *
 * The keys are plaintext in `/internal/AppData/CLOUD/cloud.conf`, like the Wi-Fi
 * password and the NAS logins. A SecretKey is worse to leak than either of those
 * — it can stop or destroy everything the account owns — so give this board a
 * CAM sub-account whose policy names only the instances it may touch.
 * ------------------------------------------------------------------------- */

/** Both providers, and the count, which is what the key table is sized by. */
#define HAL_CLOUD_CVM             0
#define HAL_CLOUD_LIGHTHOUSE      1
#define HAL_CLOUD_PROVIDER_COUNT  2

/**
 * 「全部服务器」：一次请求把存了密钥的每一家都问一遍，答案合并成一张列表。
 *
 * 只对 `HAL_CLOUD_ACTION_LIST` 有意义 —— 开关机总得指名一台机器，所以带
 * `HAL_CLOUD_ALL` 的电源动作会被 `hal_cloud_start` 拒掉。它只是列表的目标，不是任何
 * 一份密钥的主人，所以 `HAL_CLOUD_PROVIDER_COUNT` 仍然是 2，密钥表不因它变宽。
 */
#define HAL_CLOUD_ALL             2

#define HAL_CLOUD_ACTION_LIST     0
#define HAL_CLOUD_ACTION_START    1
#define HAL_CLOUD_ACTION_STOP     2
#define HAL_CLOUD_ACTION_REBOOT   3

/** What `hal_cloud_state` answers with. */
#define HAL_CLOUD_IDLE     0
#define HAL_CLOUD_RUNNING  1
#define HAL_CLOUD_DONE     2
#define HAL_CLOUD_FAILED   (-1)

/**
 * Why the request that is not DONE failed.
 *
 * A closed list rather than an `esp_err_t`, because every one of these is a
 * sentence on a page and the page is what has to be able to say it: the answer
 * JSON carries the name (`cloud_creds`, `cloud_clock`, …) and the page has the
 * wording. Deliberately absent is "the signature was wrong" — that is an API
 * refusal and arrives with a code of its own through `HAL_CLOUD_FAULT_API`.
 */
#define HAL_CLOUD_FAULT_NONE         0
/** A provider, an action or an instance id that is not one. */
#define HAL_CLOUD_FAULT_ARGS         1
/** No key on file for this provider. */
#define HAL_CLOUD_FAULT_NO_CREDS     2
/** Not on a network. */
#define HAL_CLOUD_FAULT_NO_NETWORK   3
/** The clock has not been set, so the signature would be expired before it was sent. */
#define HAL_CLOUD_FAULT_CLOCK        4
/** DNS, connect or read failed. */
#define HAL_CLOUD_FAULT_UNREACHABLE  5
/** The handshake or the certificate failed. */
#define HAL_CLOUD_FAULT_TLS          6
/** A status that was not 2xx and did not carry an `Error` object. */
#define HAL_CLOUD_FAULT_HTTP         7
/** The API answered with `Error`; the code and message are in the answer. */
#define HAL_CLOUD_FAULT_API          8
/** The answer did not fit, or the key file could not be read or written. */
#define HAL_CLOUD_FAULT_STORAGE      9

/** How long a SecretId or a SecretKey may be. Both are 36 and 32 characters as Tencent issues them. */
#define HAL_CLOUD_SECRET_ID_MAX   64
#define HAL_CLOUD_SECRET_KEY_MAX  64
/** `ap-guangzhou` and its siblings. */
#define HAL_CLOUD_REGION_MAX      32
/** `ins-xxxxxxxx` or `lhins-xxxxxxxx`. */
#define HAL_CLOUD_INSTANCE_MAX    48
/** The most of an instance name this layer will carry, in bytes of UTF-8. */
#define HAL_CLOUD_NAME_MAX        96
/** The most the key file may be. Two sections of three short lines, and room to spare. */
#define HAL_CLOUD_CONF_MAX        512

/** @brief Allocate what this needs and read the keys. Idempotent; called by everything below.
 *
 * Called at the first request rather than at boot, because the filesystem the keys are on is not
 * up when the board boots and there is no reason to have this wait for it.
 *
 * @return ESP_OK, or ESP_ERR_NO_MEM when the answer buffers could not be allocated.
 */
esp_err_t hal_cloud_init(void);

/** @brief Begin a request. Non-blocking.
 *
 * @param provider `HAL_CLOUD_CVM`, `HAL_CLOUD_LIGHTHOUSE`, or `HAL_CLOUD_ALL` for a list of every
 *                 provider that has a key (a power action naming `HAL_CLOUD_ALL` is invalid).
 * @param action   One of the `HAL_CLOUD_ACTION_*` actions.
 * @param region   `ap-guangzhou` and its siblings, or NULL/"" to use the one saved with the key.
 *                 Ignored by `HAL_CLOUD_ALL`, which asks each provider in the region its key was
 *                 saved with.
 * @param instance The instance id, and ignored by `HAL_CLOUD_ACTION_LIST`.
 *
 * Requesting one while another is in flight is ESP_ERR_INVALID_STATE and changes nothing. A request
 * that cannot be sent — no key, no network, no clock — is ESP_OK with the state failed, and the
 * reason in the answer: it is a request the caller asked for and the page has to draw.
 *
 * @return ESP_OK, ESP_ERR_INVALID_ARG, ESP_ERR_INVALID_STATE, or ESP_ERR_NO_MEM.
 */
esp_err_t hal_cloud_start(int32_t provider, int32_t action, const char *region,
                          const char *instance);

/** @brief Where the request is: `HAL_CLOUD_IDLE`, `RUNNING`, `DONE` or `FAILED`. */
int32_t hal_cloud_state(void);

/** @brief Why it failed, as one of `HAL_CLOUD_FAULT_*`. Meaningful while the state is `FAILED`. */
int32_t hal_cloud_fault(void);

/** @brief The answer, or NULL while there is not one yet.
 *
 * There is an answer for a failure as well as for a success — `{"error":"cloud_clock"}` is the whole
 * of what a page has to draw — so this comes back for `DONE` *and* for `FAILED`, and only a request
 * still in flight has nothing.
 *
 * Valid until `hal_cloud_take`, and not for the life of the board: the next request writes over it.
 * `out_length` may be NULL, and receives the length the document was written to — the buffer behind
 * this pointer is longer than the document in it.
 *
 * Not for two threads at once. A caller that handed the pointer to somebody else and then went on
 * holding it across a second request wants `hal_cloud_take_answer`, which is this and the release in
 * one step.
 */
const char *hal_cloud_answer(size_t *out_length);

/** @brief Release the answer and the slot, so the next request may start. */
void hal_cloud_take(void);

/** @brief The answer copied into the caller's buffer and the slot released, in one step.
 *
 * The web server's way in: a handler sends the document from a buffer it owns, which is what keeps a
 * second browser tab's request from rewriting the answer while the first one is being sent. The same
 * `{"error":…}` document as `hal_cloud_answer`, truncated to fit if it has to be.
 *
 * @param out       Where to write it. NUL-terminated on return.
 * @param capacity  The room there is.
 * @return The number of bytes written, or 0 when there was no answer.
 */
size_t hal_cloud_take_answer(char *out, size_t capacity);

/** @brief Let go of a request in flight.
 *
 * A request that has not left the board is dropped and the slot goes back to idle. One already on
 * the wire finishes — there is no way to un-ask the API — and its answer is discarded instead of
 * arriving at whoever asked next.
 */
void hal_cloud_cancel(void);

/** @brief Whether this provider has a key, with a masked copy of its SecretId.
 *
 * The mask is the first four characters and the last four (`AKID……wxyz`): enough to tell which key
 * is on file, and deliberately not enough to use. The SecretKey is never handed out by anything in
 * this header — a page that has saved one cannot read it back.
 *
 * @return ESP_OK, ESP_ERR_NOT_FOUND when this provider has no key, or ESP_ERR_INVALID_ARG.
 */
esp_err_t hal_cloud_credentials_get(int32_t provider, char *out_masked, size_t capacity,
                                    char *out_region, size_t region_capacity);

/** @brief Save a key for one provider, leaving the other's alone. Written to flash.
 *
 * The two strings must be printable ASCII without quotes or backslashes: what Tencent issues always
 * is, and refusing the rest here is what keeps a pasted newline out of the file this layer both
 * writes and reads.
 *
 * @return ESP_OK, ESP_ERR_INVALID_ARG for a key that is empty or not that shape,
 *         ESP_ERR_INVALID_SIZE for one that is too long, or what the filesystem said.
 */
esp_err_t hal_cloud_credentials_set(int32_t provider, const char *region, const char *secret_id,
                                    const char *secret_key);

/** @brief Forget one provider's key. ESP_ERR_NOT_FOUND when there was none. */
esp_err_t hal_cloud_credentials_forget(int32_t provider);

/* ---------------------------------------------------------------------------
 * WorkBuddy account status
 *
 * One token, two questions: what the daily check-in activity looks like, and to
 * check in. The answer is a short line-based document (see `board_workbuddy.c`),
 * taken rather than read twice -- the same start-then-poll shape as the cloud
 * client beside it.
 *
 * The token is a bearer token from the WorkBuddy desktop client (the auth file's
 * `auth.accessToken`). It is nearly two kilobytes, so it arrives through the web
 * page or a file on the card rather than through a keyboard on the panel.
 * ------------------------------------------------------------------------- */

#define HAL_WORKBUDDY_ACTION_STATUS    0
#define HAL_WORKBUDDY_ACTION_CHECKIN   1

#define HAL_WORKBUDDY_IDLE     0
#define HAL_WORKBUDDY_RUNNING  1
#define HAL_WORKBUDDY_DONE     2
#define HAL_WORKBUDDY_FAILED   (-1)

#define HAL_WORKBUDDY_FAULT_NONE         0
#define HAL_WORKBUDDY_FAULT_ARGS         1   /** 不是这两个动作之一 */
#define HAL_WORKBUDDY_FAULT_NO_CREDS     2   /** 还没有 token */
#define HAL_WORKBUDDY_FAULT_NO_NETWORK   3
#define HAL_WORKBUDDY_FAULT_UNREACHABLE  4   /** DNS/连接/读失败 */
#define HAL_WORKBUDDY_FAULT_TLS          5
#define HAL_WORKBUDDY_FAULT_HTTP         6   /** 非 2xx 且不是 401/403 */
#define HAL_WORKBUDDY_FAULT_AUTH         7   /** 401/403：token 过期或不对 */
#define HAL_WORKBUDDY_FAULT_API          8   /** 接口拒绝，message 在答案里 */
#define HAL_WORKBUDDY_FAULT_STORAGE      9   /** 答案放不下 / token 文件读写失败 */

/** A JWT from WorkBuddy, and the one beside it that renews it. Both are under 2 KB. */
#define HAL_WORKBUDDY_TOKEN_MAX    2048
#define HAL_WORKBUDDY_REFRESH_MAX  2048
/** `www.workbuddy.cn` and whatever it is called next. */
#define HAL_WORKBUDDY_HOST_MAX     64
/** An activity's name, in bytes of UTF-8. */
#define HAL_WORKBUDDY_ACTIVITY_MAX 96
/** The token file: three lines, two of them long. */
#define HAL_WORKBUDDY_CONF_MAX     4608

/** @brief Allocate what this needs and read the token. Idempotent; called by everything below.
 *
 * Called at the first request rather than at boot: the filesystem the token is on is not
 * up when the board boots and there is no reason to have this wait for it.
 *
 * @return ESP_OK, or ESP_ERR_NO_MEM when the answer buffers could not be allocated.
 */
esp_err_t hal_workbuddy_init(void);

/** @brief Begin a request. Non-blocking.
 *
 * @param action `HAL_WORKBUDDY_ACTION_STATUS` or `HAL_WORKBUDDY_ACTION_CHECKIN`.
 *
 * Requesting one while another is in flight is ESP_ERR_INVALID_STATE and changes nothing. A
 * request that cannot be sent -- no token, no network -- is ESP_OK with the state failed, and
 * the reason in the answer: it is a request the caller asked for and the page has to draw.
 *
 * @return ESP_OK, ESP_ERR_INVALID_ARG, ESP_ERR_INVALID_STATE, or ESP_ERR_NO_MEM.
 */
esp_err_t hal_workbuddy_start(int32_t action);

/** @brief Where the request is: `HAL_WORKBUDDY_IDLE`, `RUNNING`, `DONE` or `FAILED`. */
int32_t hal_workbuddy_state(void);

/** @brief Why it failed, as one of `HAL_WORKBUDDY_FAULT_*`. Meaningful while `FAILED`. */
int32_t hal_workbuddy_fault(void);

/** @brief The answer, or NULL while there is not one yet.
 *
 * Line-based and private to this tree:
 *
 *     ok=1
 *     checked_in=1
 *     streak=3
 *     daily=10
 *     activity_total=42
 *     activity=签到有礼
 *     access_days=27
 *     refresh_days=29
 *     credit_remaining=
 *     credit_total=
 *
 * A failure is `error=workbuddy_auth` (or one of its siblings), with `message=` when the
 * API gave one. `credit_remaining` and `credit_total` are empty until the balance endpoint
 * is known: an empty field is a dash, and a zero would be a number somebody believes.
 *
 * Valid until `hal_workbuddy_take`, and not for the life of the board.
 */
const char *hal_workbuddy_answer(size_t *out_length);

/** @brief Release the answer and the slot, so the next request may start. */
void hal_workbuddy_take(void);

/** @brief The answer copied into the caller's buffer and the slot released, in one step. */
size_t hal_workbuddy_take_answer(char *out, size_t capacity);

/** @brief Let go of a request in flight. */
void hal_workbuddy_cancel(void);

/** @brief Whether a token is saved, with a mask of its last four characters and the host.
 *
 * @return ESP_OK, ESP_ERR_NOT_FOUND when there is no token, or ESP_ERR_INVALID_ARG.
 */
esp_err_t hal_workbuddy_credentials_get(char *out_masked, size_t capacity, char *out_host,
                                        size_t host_capacity);

/** @brief Save a token, written to flash.
 *
 * The token must be printable ASCII without quotes, backslashes or spaces: a JWT is. Refusing
 * the rest here is what keeps a pasted newline out of the file this layer both writes and reads.
 * `refresh` may be NULL/"" and `host` may be NULL/"" for the default.
 *
 * @return ESP_OK, ESP_ERR_INVALID_ARG, ESP_ERR_INVALID_STATE while a request is in flight,
 *         or what the filesystem said.
 */
esp_err_t hal_workbuddy_credentials_set(const char *token, const char *refresh, const char *host);

/** @brief Forget the token. ESP_ERR_NOT_FOUND when there was none. */
esp_err_t hal_workbuddy_credentials_forget(void);

/* ---------------------------------------------------------------------------
 * Signing in without typing anything
 *
 * The App's own sign-in, driven from here: ask for a `state`, hand the `authUrl` back
 * so somebody can open it, and poll until the account answers with a token. A bearer
 * token is two kilobytes -- this is the only way one arrives without a keyboard.
 * ------------------------------------------------------------------------- */

#define HAL_WORKBUDDY_LOGIN_IDLE    0
#define HAL_WORKBUDDY_LOGIN_WAITING 1   /** 链接已经给出，等人在浏览器里授权 */
#define HAL_WORKBUDDY_LOGIN_DONE    2   /** 令牌拿到了，也写进文件了 */
#define HAL_WORKBUDDY_LOGIN_FAILED  (-1)

/** @brief Ask the account for a sign-in link.
 *
 * Blocking, one round trip. The `authUrl` is then read with `hal_workbuddy_login_url` and
 * shown; the account waits for a person, which is why this is not the same call as the poll.
 *
 * @return ESP_OK, ESP_ERR_INVALID_STATE (no network, or a request in flight),
 *         ESP_ERR_NOT_FOUND (no platform answered), or ESP_ERR_NO_MEM.
 */
esp_err_t hal_workbuddy_login_start(void);

/** @brief Ask once whether the person has authorised yet. Blocking, one round trip.
 *
 * @return ESP_OK when the token arrived and was saved, ESP_ERR_NOT_FINISHED while the
 *         account is still waiting, ESP_ERR_INVALID_STATE when no sign-in is in flight,
 *         ESP_ERR_TIMEOUT when the wire did not answer (ask again), or
 *         ESP_ERR_INVALID_RESPONSE when the account refused -- the reason is in
 *         `hal_workbuddy_login_message`.
 */
esp_err_t hal_workbuddy_login_poll(void);

/** @brief `HAL_WORKBUDDY_LOGIN_*`. */
int32_t hal_workbuddy_login_state(void);

/** @brief The sign-in link, copied out. Returns its length. */
size_t hal_workbuddy_login_url(char *out, size_t capacity);

/** @brief The account's own refusal, when there was one. Returns its length. */
size_t hal_workbuddy_login_message(char *out, size_t capacity);

/** @brief Abandon a sign-in that is waiting. */
void hal_workbuddy_login_cancel(void);

/* ---------------------------------------------------------------------------
 * Log ring.
 *
 * The last `HAL_LOG_LINES` lines every `ESP_LOGx` wrote, kept in a ring so the page can show them.
 * `hal_log_init` installs the hook in front of the console; the console keeps its output unchanged.
 * ------------------------------------------------------------------------- */

#define HAL_LOG_LINES    128
#define HAL_LOG_LINE_MAX 160

typedef struct {
    /** The number the line was given when it was written, ascending from 1. Empty slots are 0. */
    uint32_t seq;
    /** When it was written, in microseconds since boot. */
    int64_t uptime_us;
    /** The line, without its newline, and truncated to fit. */
    char text[HAL_LOG_LINE_MAX];
} hal_log_entry_t;

/** @brief Install the log hook. Call once, early, before anything has anything to say. */
esp_err_t hal_log_init(void);

/** @brief Read the lines written since `*in_out_since`.
 *
 * `out` takes up to `max` entries in the order they were written. On return `*in_out_since` is the
 * number of the newest line in the ring — what the caller has now seen — so a page can poll with the
 * value it was given and get only what is new. A caller that has fallen further behind than the ring
 * is long simply misses the lines that were overwritten.
 */
size_t hal_log_read(hal_log_entry_t *out, size_t max, uint32_t *in_out_since);

/* ---------------------------------------------------------------------------
 * ES8311 audio codec and I2S speaker interface (Audio Sink).
 * ------------------------------------------------------------------------- */

/**
 * @brief Initialize ES8311 audio codec and I2S speaker interface
 */
esp_err_t hal_audio_init(void);

/**
 * @brief Open audio codec with specified sample rate, channels, and bits per sample
 */
esp_err_t hal_audio_open(uint32_t sample_rate, uint8_t channels, uint8_t bits_per_sample);

/**
 * @brief Write PCM audio samples to speaker codec (I2S DMA)
 */
esp_err_t hal_audio_write(const void *data, uint32_t len);

/**
 * @brief Drain I2S DMA pipeline and ensure pending samples are played
 */
esp_err_t hal_audio_drain(void);

/**
 * @brief Set speaker output volume (0 - 100)
 */
esp_err_t hal_audio_set_volume(uint8_t volume);

/**
 * @brief Close audio codec
 */
esp_err_t hal_audio_close(void);

/**
 * @brief Play a short test tone and return once it has finished.
 *
 * Blocks for the duration of the tone (capped at 5 s) and reconfigures the codec to 16 kHz mono, so
 * anything else streaming through it is interrupted — which is the point of a "beep to find the
 * box" button.
 *
 * @param freq_hz Tone pitch, at least 1 Hz and no higher than half the 16 kHz rate.
 * @param ms      Duration in milliseconds.
 * @param volume  0-100, or >100 to leave the current volume alone.
 */
esp_err_t hal_audio_tone(uint32_t freq_hz, uint32_t ms, uint8_t volume);

/* ---------------------------------------------------------------------------
 * PCF85063A Real-Time Clock (RTC).
 * ------------------------------------------------------------------------- */

/**
 * @brief Initialize PCF85063A RTC on the shared I2C bus and restore POSIX system clock if valid.
 */
esp_err_t hal_rtc_init(void);

/**
 * @brief Read the hardware RTC as a Unix epoch.
 *
 * @param out_epoch Receives the chip's time, which is UTC.
 * @return ESP_OK, ESP_ERR_INVALID_STATE when the chip holds no sane time (pre-2024), or a read error.
 */
esp_err_t hal_rtc_get_epoch(time_t *out_epoch);

/**
 * @brief Set the system clock and the hardware RTC together.
 *
 * @param epoch Unix time, at least 2024-01-01.
 */
esp_err_t hal_rtc_set_epoch(time_t epoch);


#ifdef __cplusplus
}
#endif

#endif // BOARD_HAL_H
