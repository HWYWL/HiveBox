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
 * Screenshots come from a shadow copy of the frame the panel was last given, which costs half a
 * megabyte of PSRAM. When that allocation failed there is no copy and no screenshot.
 */
bool hal_display_has_capture(void);

/**
 * @brief Copy the screen into `out` as RGB565, top row first, `BOARD_DISPLAY_WIDTH` pixels wide.
 *
 * @param out    Buffer of at least `BOARD_DISPLAY_WIDTH * BOARD_DISPLAY_HEIGHT` pixels.
 * @param pixels Size of `out` in pixels, checked against the panel geometry.
 * @return ESP_OK, ESP_ERR_INVALID_STATE when there is no shadow frame, or ESP_ERR_INVALID_SIZE.
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
 * @brief Remove `path`.
 *
 * @return ESP_OK when it is gone, including when it never existed; ESP_ERR_INVALID_ARG if `path` is
 *         NULL; ESP_FAIL if the filesystem refused.
 */
esp_err_t hal_storage_remove_file(const char *path);

/* The flash chip itself: how much of it there is, and what every region of it is for.
 *
 * hal_storage_get_internal() answers "how full is the filesystem the box can write" — on this board
 * 3 MB of a 16 MB chip. This answers the question under it: where the other 13 MB went. A page that
 * draws only the first is drawing a third of the answer.
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
