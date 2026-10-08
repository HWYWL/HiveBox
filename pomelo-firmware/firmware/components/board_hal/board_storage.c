/*
 * The board's filesystems: the built-in `internal` LittleFS partition, and the microSD slot.
 *
 * Two things are worth knowing before reading it.
 *
 * # There is no card-detect pin
 *
 * The slot is wired to SDMMC in 1-bit mode with neither card-detect nor write-protect, so "is there
 * a card?" cannot be watched — it can only be asked, and asking means trying to mount one. A probe
 * that finds nothing still spends the driver's timeouts (the card never answers, so the probe ends
 * in ESP_ERR_TIMEOUT after the SDMMC host has given up), which is why this file has a retry
 * interval of its own and why the Rust side asks it when the answer matters rather than every frame.
 *
 * # The IDF layer logs the failures, not this one
 *
 * `vfs_fat_sdmmc` prints an ERROR line every time a probe finds nothing — `sdmmc_card_init failed` —
 * and it is the only thing that knows *why*, so this file does not add a second line at that level:
 * an empty slot would then be two complaints about the ordinary case. What is logged here is the
 * transitions that mean something: a card that arrived, and a card that left.
 */

#include "board_hal.h"

#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>

#include "bsp/esp32_s3_touch_amoled_2_16.h"
#include "esp_flash.h"
#include "esp_littlefs.h"
#include "esp_log.h"
#include "esp_partition.h"
#include "esp_timer.h"
#include "esp_vfs_fat.h"
#include "sdmmc_cmd.h"

static const char *TAG = "board_storage";

/* The built-in partition: as `partitions.csv` names it and as `main.c` mounts it. */
#define STORAGE_INTERNAL_PARTITION "internal"
#define STORAGE_INTERNAL_MOUNT     "/internal"

/* How long to leave the slot alone after a probe that found nothing.
 *
 * A bound rather than a policy: the callers that exist ask on a page opening and on a finger, so
 * this is here for the caller that would loop — an open page redrawing, a future poller — where an
 * unbounded probe per call would keep the SDMMC host busy for the sake of an empty slot. */
#define STORAGE_RETRY_INTERVAL_US (1000 * 1000)

static bool    s_card_mounted = false;
static int64_t s_next_probe_us = 0;

/** Copy a C string into one of the fixed fields of the FFI struct. */
static void set_field(char *field, size_t size, const char *value)
{
    snprintf(field, size, "%s", value);
}

/** Whether the card is still answering.
 *
 * Asked of the card and not of the filesystem: FATFS keeps the free-cluster count it read at mount
 * time in RAM, so `esp_vfs_fat_info` goes on answering with a card's sizes long after the card has
 * left the slot — which is what a page that kept drawing a card no longer in it was reading back.
 * CMD13 is the command ESP-IDF's own FATFS driver asks this question with (`ff_sdmmc_card_available`,
 * whose comment is "Check if SD/MMC card is present"): no card, no answer.
 *
 * A NULL handle is a board whose last mount failed, which is also "no card". */
static bool card_answers(void)
{
    if (bsp_sdcard == NULL) {
        return false;
    }

    return sdmmc_get_status(bsp_sdcard) == ESP_OK;
}

esp_err_t hal_storage_refresh(void)
{
    if (s_card_mounted) {
        if (card_answers()) {
            return ESP_OK;
        }

        ESP_LOGI(TAG, "the card left the slot; unmounting %s", BSP_SD_MOUNT_POINT);

        /* The BSP mounts through its own `bsp_sdcard_mount` but does not declare an unmount in its
         * header — the function exists in its source and calls exactly this — so the public call
         * underneath it is used here rather than a hand-written declaration of another component's
         * signature, which is a copy that can drift.
         *
         * `bsp_sdcard` is the head of the BSP's own `esp_vfs_fat_sdmmc_mount` and is declared in its
         * header. IDF frees the card as it unmounts, so it is cleared here rather than left pointing
         * into freed heap for whatever reads the global next. */
        if (bsp_sdcard != NULL) {
            esp_err_t ret = esp_vfs_fat_sdcard_unmount(BSP_SD_MOUNT_POINT, bsp_sdcard);
            if (ret != ESP_OK) {
                ESP_LOGW(TAG, "unmount warning: %s", esp_err_to_name(ret));
            }

            bsp_sdcard = NULL;
        }

        s_card_mounted = false;

        /* One probe per departure: a slot that has just given up its card has nothing to find for a
         * moment, and the caller is told so without paying for a second mount attempt. */
        s_next_probe_us = esp_timer_get_time() + STORAGE_RETRY_INTERVAL_US;
        return ESP_ERR_NOT_FOUND;
    }

    int64_t now = esp_timer_get_time();
    if (now < s_next_probe_us) {
        return ESP_ERR_NOT_FOUND;
    }

    esp_err_t ret = bsp_sdcard_mount();
    if (ret != ESP_OK) {
        s_next_probe_us = now + STORAGE_RETRY_INTERVAL_US;
        return ESP_ERR_NOT_FOUND;
    }

    s_card_mounted = true;
    s_next_probe_us = 0;

    /* The mount succeeded, and the question the *next* probe will ask is whether the card is still
     * there — so it is asked once here, where the answer is known to be yes. A card that mounts but
     * will not answer is one the next refresh would drop, and a line now is worth more than a card
     * that quietly leaves the page later. */
    if (card_answers()) {
        ESP_LOGI(TAG, "mounted the card at %s", BSP_SD_MOUNT_POINT);
    } else {
        ESP_LOGW(TAG, "the card at %s mounted but does not answer", BSP_SD_MOUNT_POINT);
    }

    return ESP_OK;
}

esp_err_t hal_storage_get_internal(hal_storage_volume_t *out)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    size_t total = 0;
    size_t used = 0;

    esp_err_t ret = esp_littlefs_info(STORAGE_INTERNAL_PARTITION, &total, &used);
    if (ret != ESP_OK) {
        return ret;
    }

    set_field(out->mount_point, sizeof(out->mount_point), STORAGE_INTERNAL_MOUNT);
    set_field(out->filesystem, sizeof(out->filesystem), "LittleFS");
    out->total_bytes = total;
    /* LittleFS counts what is written; a filesystem reports what is left. */
    out->free_bytes = (used > total) ? 0 : (uint64_t)total - used;

    return ESP_OK;
}

esp_err_t hal_storage_get_card(hal_storage_volume_t *out)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    /* The flag and not the filesystem: mounting and unmounting is what `refresh` does, and a getter
     * that unmounted would be doing the refresh's job behind the caller's back. An empty slot is the
     * ordinary answer here, not a fault — but a card that has been pulled *is* an empty slot, so the
     * card itself is asked, and the sizes below are only read once it has answered. */
    if (!s_card_mounted || !card_answers()) {
        return ESP_ERR_NOT_FOUND;
    }

    uint64_t total = 0;
    uint64_t free_bytes = 0;

    esp_err_t ret = esp_vfs_fat_info(BSP_SD_MOUNT_POINT, &total, &free_bytes);
    if (ret != ESP_OK) {
        /* Gone between a refresh and this call. All this can say is that it is not there now. */
        ESP_LOGW(TAG, "card info failed: %s", esp_err_to_name(ret));
        return ESP_ERR_NOT_FOUND;
    }

    set_field(out->mount_point, sizeof(out->mount_point), BSP_SD_MOUNT_POINT);
    set_field(out->filesystem, sizeof(out->filesystem), "FATFS");
    out->total_bytes = total;
    out->free_bytes = free_bytes;

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * Files, through `stdio`.
 *
 * # Why this exists at all
 *
 * The Rust half has `std::fs`, and on every other platform it is what a file is written with. Here it
 * does not work — and it fails in the flattering direction, with a *read* that succeeds and a write
 * that does not, so the code that used it looked correct and quietly kept nothing.
 *
 * What the board actually does, measured rather than supposed:
 *
 *   metadata, read_dir, create_dir, create_dir_all, remove_dir_all   all fine
 *   open an existing file for reading                                fine
 *   open anything for writing, every flag combination, O_CREAT or not
 *                                                                    ENOENT, always
 *
 * `main.c` has been writing `/internal/welcome.txt` with `fopen` since the partition was mounted, so
 * the filesystem is not the part that cannot write: it is `std::fs`'s way in. Rather than guess at
 * which syscall the Rust runtime picks, the write goes through the same `stdio` call the rest of the
 * firmware already uses, and the Rust side keeps `std::fs` for the parts of it that work.
 *
 * # The signatures are the point
 *
 * `hal_storage_read_file` fills a caller-owned buffer, because the caller is the one that knows how
 * much text it is willing to hold: a file bigger than the buffer is an error and not a truncation, and
 * the Rust side reads that as "this is not a credentials file" instead of as a shorter one.
 * ------------------------------------------------------------------------- */

/** Every directory on the way to `path`, created if it is not there yet. */
static esp_err_t make_dirs(const char *path)
{
    /* A stack copy of the path, because the loop below has to end the string at each separator to
     * hand `mkdir` one component at a time, and the caller's own string is not this function's to
     * write in. 256 characters is more than any path this board builds — `/internal/AppData/WIFI/
     * wifi.conf` is 33 — and a longer one is refused rather than silently cut short. */
    char scratch[256];
    size_t len = strlen(path);

    if (len >= sizeof(scratch)) {
        return ESP_ERR_INVALID_SIZE;
    }

    memcpy(scratch, path, len + 1);

    /* From the second character, so that the leading `/` of the mount point — which always exists and
     * cannot be made — is not mistaken for a component. */
    for (char *slash = scratch + 1; (slash = strchr(slash, '/')) != NULL; slash++) {
        *slash = '\0';

        if (mkdir(scratch, 0777) != 0 && errno != EEXIST) {
            ESP_LOGE(TAG, "cannot make %s: errno %d", scratch, errno);
            return ESP_FAIL;
        }

        *slash = '/';
    }

    return ESP_OK;
}

esp_err_t hal_storage_read_file(const char *path, char *out, size_t capacity, size_t *out_len)
{
    if (path == NULL || out == NULL || out_len == NULL || capacity == 0) {
        return ESP_ERR_INVALID_ARG;
    }

    FILE *file = fopen(path, "rb");

    if (file == NULL) {
        /* A file that is not there is the ordinary answer, not a fault: it is what a board that has
         * never been asked to remember anything says. */
        return (errno == ENOENT) ? ESP_ERR_NOT_FOUND : ESP_FAIL;
    }

    /* One byte held back for the terminator, so the text handed back is a C string either way. */
    size_t read = fread(out, 1, capacity - 1, file);
    bool  over = !feof(file);

    fclose(file);

    if (over) {
        ESP_LOGW(TAG, "%s is larger than the %u-byte buffer it was read into",
                 path, (unsigned)capacity);
        return ESP_ERR_INVALID_SIZE;
    }

    out[read] = '\0';
    *out_len = read;

    return ESP_OK;
}

esp_err_t hal_storage_write_file(const char *path, const char *data, size_t len)
{
    if (path == NULL || data == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    esp_err_t made = make_dirs(path);
    if (made != ESP_OK) {
        return made;
    }

    FILE *file = fopen(path, "wb");

    if (file == NULL) {
        ESP_LOGE(TAG, "cannot open %s for writing: errno %d", path, errno);
        return ESP_FAIL;
    }

    size_t written = fwrite(data, 1, len, file);
    int    closed = fclose(file);

    /* Both checked, and both reported together: a short write is a disk that filled up, and a failed
     * close is data that never reached the flash — either one means the file is not what was asked
     * for, and the caller has to hear about it rather than find out at the next boot. */
    if (written != len || closed != 0) {
        ESP_LOGE(TAG, "%s: wrote %u of %u bytes, close returned %d",
                 path, (unsigned)written, (unsigned)len, closed);
        return ESP_FAIL;
    }

    return ESP_OK;
}

esp_err_t hal_storage_remove_file(const char *path)
{
    if (path == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    if (remove(path) == 0 || errno == ENOENT) {
        /* Including the case where there was nothing to remove: the caller asked for the file to be
         * gone, and it is. */
        return ESP_OK;
    }

    ESP_LOGE(TAG, "cannot remove %s: errno %d", path, errno);
    return ESP_FAIL;
}

/* ---------------------------------------------------------------------------
 * The flash chip: what every region of it is for.
 *
 * The sizes above answer "how full is what the box can write". This answers the question under it,
 * and the two numbers are not close: the built-in LittleFS partition is 3 MB of a 16 MB chip, and
 * the rest is the firmware and the reservations around it — none of which is a filesystem, which is
 * why none of it can come from `esp_littlefs_info`.
 *
 * The chip is described by tiling, not by listing partitions: the bootloader and the partition table
 * sit below the first partition and the trailing space sits past the last, neither has a row in the
 * table, and a map that dropped them would not add up to the flash it claims to describe.
 * ------------------------------------------------------------------------- */

/** What a partition is for, in the four kinds the UI draws.
 *
 * An app partition is the firmware. A data partition with a filesystem on it is writable. Everything
 * else — NVS, the PHY calibration blob, and whatever a future table puts there — is system: real,
 * owned by something other than the user, and not to be shown as free space. */
static uint8_t classify(const esp_partition_t *part)
{
    if (part->type == ESP_PARTITION_TYPE_APP) {
        return HAL_FLASH_REGION_FIRMWARE;
    }

    switch (part->subtype) {
    case ESP_PARTITION_SUBTYPE_DATA_SPIFFS:
    case ESP_PARTITION_SUBTYPE_DATA_LITTLEFS:
    case ESP_PARTITION_SUBTYPE_DATA_FAT:
        return HAL_FLASH_REGION_DATA;
    default:
        return HAL_FLASH_REGION_SYSTEM;
    }
}

/** Append a region. False once the layout is full, which a table this small will not fill. */
static bool add_region(hal_flash_layout_t *out, const char *label, uint8_t kind, uint32_t size)
{
    if (size == 0 || out->count >= HAL_FLASH_MAX_REGIONS) {
        return false;
    }

    hal_flash_region_t *region = &out->regions[out->count++];
    set_field(region->label, sizeof(region->label), label);
    region->kind = kind;
    region->size = size;

    return true;
}

esp_err_t hal_storage_get_flash(hal_flash_layout_t *out)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    memset(out, 0, sizeof(*out));

    uint32_t chip = 0;
    esp_err_t ret = esp_flash_get_size(NULL, &chip);
    if (ret != ESP_OK) {
        ESP_LOGW(TAG, "the flash size could not be read: %s", esp_err_to_name(ret));
        return ret;
    }

    out->total_bytes = chip;

    /* The partitions, collected and then put in address order.
     *
     * `esp_partition_find` walks the table in the order it was written, which is not promised to be
     * sorted; everything below reads the list as a run of addresses, so the order is established
     * here rather than assumed. An insertion sort over at most HAL_FLASH_MAX_REGIONS entries is
     * cheaper than a map with a region out of place. */
    const esp_partition_t *parts[HAL_FLASH_MAX_REGIONS];
    size_t count = 0;

    esp_partition_iterator_t it =
        esp_partition_find(ESP_PARTITION_TYPE_ANY, ESP_PARTITION_SUBTYPE_ANY, NULL);

    while (it != NULL && count < HAL_FLASH_MAX_REGIONS) {
        const esp_partition_t *part = esp_partition_get(it);

        size_t at = count;
        while (at > 0 && parts[at - 1]->address > part->address) {
            parts[at] = parts[at - 1];
            at--;
        }
        parts[at] = part;
        count++;

        it = esp_partition_next(it);
    }

    /* A loop that filled up still owns its iterator; the one that ran out has released its own.
     * Releasing NULL is explicitly allowed, so there is one call and not two paths. */
    esp_partition_iterator_release(it);

    if (count == 0) {
        /* No table at all. Everything below would be one region called "unallocated" covering the
         * whole chip, which is a drawing of a board that was never flashed — say so instead. */
        ESP_LOGW(TAG, "no partition table to read; the flash map is not drawn");
        return ESP_ERR_NOT_FOUND;
    }

    /* Below the first partition: the second-stage bootloader and the partition table itself. */
    uint32_t cursor = 0;

    if (parts[0]->address > 0) {
        add_region(out, "bootloader", HAL_FLASH_REGION_SYSTEM, parts[0]->address);
        cursor = parts[0]->address;
    }

    for (size_t i = 0; i < count; i++) {
        const esp_partition_t *part = parts[i];

        if (part->address > cursor) {
            add_region(out, "unallocated", HAL_FLASH_REGION_UNALLOCATED, part->address - cursor);
        }

        add_region(out, part->label, classify(part), part->size);
        cursor = part->address + part->size;
    }

    /* Past the last partition. The table does not have to reach the end of the chip, and on this
     * board it does not: the flash is 16 MB and the last partition ends 960 KB before it. */
    if (chip > cursor) {
        add_region(out, "unallocated", HAL_FLASH_REGION_UNALLOCATED, chip - cursor);
    }

    return ESP_OK;
}
