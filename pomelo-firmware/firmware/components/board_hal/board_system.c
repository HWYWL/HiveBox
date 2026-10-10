/**
 * @file board_system.c
 * @brief The `hal_system_*` reads: what this machine is, and what it has left.
 *
 * Four questions with four different owners — the chip, the image, the timer and
 * the heap — and not one of them a peripheral this component drives. That is why
 * there is no `hal_system_init`: everything here is a read of something that is
 * already running, and the only state is on the other side of the call.
 *
 * Both of the strings come out of `esp_app_get_description`, which is the image's
 * own description of itself as the build stamped it. Reading them here rather than
 * letting a page write them down is the whole point: a version number that lives in
 * two places is a version number that is wrong in one of them, and on a board that
 * gets reflashed the one in the source is the one that lies.
 */

#include "board_hal.h"

#include <string.h>
#include <time.h>

#include "esp_app_desc.h"
#include "esp_chip_info.h"
#include "esp_heap_caps.h"
#include "esp_system.h"
#include "esp_timer.h"

#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

/* ---------------------------------------------------------------------------
 * Chip
 * ------------------------------------------------------------------------- */

/**
 * @brief The part number a chip model reports, spelled the way the chip says it.
 *
 * A switch and not `esp_chip_info`'s own enum, because the enum's *values* are the
 * vendor's business and its *names* are C identifiers. What a page draws has to be
 * the part number: `ESP32-S3` is what is written on the module and what a person
 * can search for.
 *
 * Every model this IDF can name is listed, including the ones this board will never
 * be — they are a handful of bytes in `.rodata`, and a port to another chip that got
 * `unknown` would be a bug nobody could see. The `default` is therefore for a model
 * from an IDF newer than this one, and it says so rather than guessing.
 */
static const char *chip_model_name(esp_chip_model_t model)
{
    switch (model) {
    case CHIP_ESP32:     return "ESP32";
    case CHIP_ESP32S2:   return "ESP32-S2";
    case CHIP_ESP32S3:   return "ESP32-S3";
    case CHIP_ESP32C2:   return "ESP32-C2";
    case CHIP_ESP32C3:   return "ESP32-C3";
    case CHIP_ESP32C5:   return "ESP32-C5";
    case CHIP_ESP32C6:   return "ESP32-C6";
    case CHIP_ESP32C61:  return "ESP32-C61";
    case CHIP_ESP32H2:   return "ESP32-H2";
    case CHIP_ESP32H21:  return "ESP32-H21";
    case CHIP_ESP32H4:   return "ESP32-H4";
    case CHIP_ESP32P4:   return "ESP32-P4";
    case CHIP_ESP32S31:  return "ESP32-S31";
    case CHIP_POSIX_LINUX: return "POSIX/Linux";
    default:             return "unknown";
    }
}

esp_err_t hal_system_get_chip(hal_system_chip_t *out)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    esp_chip_info_t chip;
    esp_chip_info(&chip);

    /* Zeroed first, so the copy below cannot leave the struct holding whatever was
     * on the caller's stack — that is most of what a C struct initialised field by
     * field gets wrong, and this is a struct that crosses to another language. */
    memset(out, 0, sizeof *out);

    strncpy(out->model, chip_model_name(chip.model), sizeof out->model - 1);
    out->cores = (uint8_t)chip.cores;
    out->revision = (uint16_t)chip.revision;

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * Firmware
 * ------------------------------------------------------------------------- */

esp_err_t hal_system_get_firmware(hal_system_firmware_t *out)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    const esp_app_desc_t *app = esp_app_get_description();
    if (app == NULL) {
        /* An image built outside this toolchain carries no description. Not a
         * fault to report as a read error: there is no answer to give. */
        return ESP_ERR_NOT_FOUND;
    }

    memset(out, 0, sizeof *out);

    strncpy(out->name, app->project_name, sizeof out->name - 1);
    strncpy(out->version, app->version, sizeof out->version - 1);

    /* The stamp the compiler left in this image — the same pair the IDF itself puts
     * in its boot log. Copied across as the two strings it is; see the note on the
     * struct for why the respelling happens on the Rust side. */
    strncpy(out->date, app->date, sizeof out->date - 1);
    strncpy(out->time, app->time, sizeof out->time - 1);

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * Uptime
 * ------------------------------------------------------------------------- */

int64_t hal_system_get_uptime_us(void)
{
    /* The microsecond timer starts at zero when the chip comes out of reset and
     * counts up from there, so this is the uptime and not a difference between two
     * readings of a clock that can be set. It keeps counting through light sleep on
     * this IDF by default, which is what "how long has this been up" means. */
    return esp_timer_get_time();
}

/* ---------------------------------------------------------------------------
 * Memory
 * ------------------------------------------------------------------------- */

esp_err_t hal_system_get_memory(hal_system_memory_t *out)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    /* The default heap, and not the chip's whole address space.
     *
     * `MALLOC_CAP_DEFAULT` is what `malloc()` without a capability allocates from —
     * internal 8-bit-capable RAM — and the 8 MB of PSRAM beside it is deliberately
     * outside that pool: the heap capability allocator will only hand PSRAM to a
     * caller that names it, which is why the framebuffer and the log ring ask for it
     * explicitly. Counting PSRAM in this total would answer a different question —
     * "how much of the chip's RAM is spoken for" — and it would answer it in a way
     * that makes an exhausted internal heap look like a board with two thirds free.
     *
     * `esp_get_free_heap_size` is this same pool's free size, so the two numbers
     * are two readings of one thing and the difference between them means something. */
    out->total_bytes = (uint64_t)heap_caps_get_total_size(MALLOC_CAP_DEFAULT);
    out->free_bytes = (uint64_t)esp_get_free_heap_size();

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * Clock
 * ------------------------------------------------------------------------- */

esp_err_t hal_system_get_epoch(int64_t *out_epoch)
{
    if (out_epoch == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    /* The hardware RTC and not `time(NULL)`, even though the two are usually the
     * same instant.
     *
     * The chip is the one that holds time across a power cycle, and `board_rtc.c`
     * keeps the system clock in step with it in both directions: it restores the
     * POSIX clock from the chip at boot, and it wraps `settimeofday` so that an NTP
     * sync is written straight back to the chip. Asking the chip is therefore asking
     * the clock, and it comes with the one thing `time(NULL)` cannot say — a
     * "nothing sane is stored" answer of its own, rather than zero or the seconds
     * since boot dressed up as 1970. */
    time_t epoch = 0;
    esp_err_t ret = hal_rtc_get_epoch(&epoch);
    if (ret != ESP_OK) {
        return ret;
    }

    *out_epoch = (int64_t)epoch;

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * Tasks
 * ------------------------------------------------------------------------- */

esp_err_t hal_system_get_tasks(hal_system_tasks_t *out)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    memset(out, 0, sizeof *out);

#if CONFIG_FREERTOS_USE_TRACE_FACILITY
    /* `uxTaskGetSystemState` is the one read that fills in everything a list wants at
     * once — name, state, priority, core and stack high-water mark — and it is also the
     * only one of these reads that costs anything: the scheduler is briefly held while
     * the task list is walked. That is why a page showing this wants it once a second
     * and not once a frame. */
    TaskStatus_t statuses[HAL_SYSTEM_MAX_TASKS];
    UBaseType_t   count = uxTaskGetSystemState(statuses, HAL_SYSTEM_MAX_TASKS, NULL);
    TaskHandle_t  self  = xTaskGetCurrentTaskHandle();

    if (count > HAL_SYSTEM_MAX_TASKS) {
        count = HAL_SYSTEM_MAX_TASKS;
    }

    for (UBaseType_t i = 0; i < count; i++) {
        hal_system_task_t *task = &out->tasks[i];

        if (statuses[i].pcTaskName != NULL) {
            strncpy(task->name, statuses[i].pcTaskName, sizeof task->name - 1);
        }

        /* "Ready" and "running" are one answer here: the difference between them is a
         * scheduling decision measured in microseconds, and what a reader wants to know
         * is whether the task is making progress. */
        switch (statuses[i].eCurrentState) {
        case eBlocked:   task->state = 1; break;
        case eSuspended: task->state = 2; break;
        case eDeleted:   task->state = 3; break;
        default:         task->state = 0; break;
        }

        task->priority = (uint8_t)statuses[i].uxCurrentPriority;

        /* Which core it may run on, asked of the scheduler rather than read out of the status
         * struct: whether `TaskStatus_t` carries a core id at all depends on
         * `CONFIG_FREERTOS_VTASKLIST_INCLUDE_COREID`, which in turn depends on the
         * stats-formatting functions — a `vTaskList` this image has no use for. The cast is
         * deliberate: `tskNO_AFFINITY` is a big number, and it comes out as 0xff, which is
         * the value the Rust side reads as "the scheduler may run it on either". */
        task->core = (uint8_t)xTaskGetCoreID(statuses[i].xHandle);

        task->stack_free_bytes = (uint32_t)statuses[i].usStackHighWaterMark;
        task->current          = (statuses[i].xHandle == self) ? 1 : 0;
    }

    out->count = (uint32_t)count;
#else
    /* Built without the trace facility: there is no per-task read to make, and an empty
     * list is the honest answer. A list of made-up tasks would be worse than none. */
    out->count = 0;
#endif

    return ESP_OK;
}

/* ---------------------------------------------------------------------------
 * External RAM
 * ------------------------------------------------------------------------- */

esp_err_t hal_system_get_psram(hal_system_memory_t *out)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }

    size_t total = heap_caps_get_total_size(MALLOC_CAP_SPIRAM);
    if (total == 0) {
        return ESP_ERR_NOT_FOUND;
    }

    memset(out, 0, sizeof *out);

    out->total_bytes = (uint64_t)total;
    out->free_bytes  = (uint64_t)heap_caps_get_free_size(MALLOC_CAP_SPIRAM);

    return ESP_OK;
}
