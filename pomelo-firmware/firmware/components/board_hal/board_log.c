/*
 * The log ring: the last hundred-odd lines `esp_log` wrote, for the page to read.
 *
 * The console already has them, and a serial cable is the way to read them — but the box is a thing
 * on a shelf, and "plug a cable into it" is not something a person does from the sofa. What this adds
 * is the other end of the same output: the same lines, kept in a ring, readable over HTTP.
 *
 * The hook is `esp_log_set_vprintf`, which sits in front of every `ESP_LOGx`. A line is formatted
 * once into a stack buffer, copied into the ring, and then handed to the console's own `vprintf`
 * untouched — so the serial output is exactly what it was, byte for byte, and this is only a second
 * reader of it.
 */
#include "board_hal.h"

#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"

#include <stdarg.h>
#include <stdio.h>
#include <string.h>

static hal_log_entry_t s_lines[HAL_LOG_LINES];

/* Where the next line goes, and the number it gets. Empty slots keep `seq == 0`, which is what
 * `hal_log_read` reads to mean "nothing has been written here yet". */
static size_t   s_next;
static uint32_t s_sequence = 1;

static SemaphoreHandle_t s_lock;
static vprintf_like_t    s_console = vprintf;

static void log_push(const char *text, int length)
{
    if (s_lock == NULL || length <= 0) {
        return;
    }

    /* The ring is written from every task that logs, and read from the HTTP task. `memcpy` under a
     * mutex is the whole of the synchronisation — the alternative, a lock-free ring, would be a
     * second copy of a data structure to get wrong for a feature that is a convenience. */
    if (xSemaphoreTake(s_lock, pdMS_TO_TICKS(20)) != pdTRUE) {
        return;
    }

    hal_log_entry_t *slot = &s_lines[s_next];

    if (length > HAL_LOG_LINE_MAX - 1) {
        length = HAL_LOG_LINE_MAX - 1;
    }

    memcpy(slot->text, text, (size_t)length);
    slot->text[length] = '\0';
    slot->uptime_us    = esp_timer_get_time();
    slot->seq          = s_sequence++;

    s_next = (s_next + 1) % HAL_LOG_LINES;

    xSemaphoreGive(s_lock);
}

/* Every `ESP_LOGx` arrives here first: keep the line, then let the console have it exactly as it was
 * written. A line longer than the ring holds is truncated in the ring and printed whole on the
 * cable — the console is the record, this is the convenience. */
static int log_vprintf(const char *format, va_list args)
{
    char    line[HAL_LOG_LINE_MAX + 16];
    va_list copy;

    va_copy(copy, args);
    int written = vsnprintf(line, sizeof line, format, copy);
    va_end(copy);

    if (written > 0) {
        int length = written < (int)sizeof line ? written : (int)sizeof line - 1;

        /* The newline is the console's; a page ends its own lines. */
        while (length > 0 && (line[length - 1] == '\n' || line[length - 1] == '\r')) {
            length--;
        }

        log_push(line, length);
    }

    return s_console != NULL ? s_console(format, args) : 0;
}

esp_err_t hal_log_init(void)
{
    if (s_lock == NULL) {
        s_lock = xSemaphoreCreateMutex();
        if (s_lock == NULL) {
            return ESP_ERR_NO_MEM;
        }
    }

    memset(s_lines, 0, sizeof s_lines);
    s_next     = 0;
    s_sequence = 1;

    s_console = esp_log_set_vprintf(log_vprintf);

    return ESP_OK;
}

size_t hal_log_read(hal_log_entry_t *out, size_t max, uint32_t *in_out_since)
{
    if (out == NULL || max == 0) {
        return 0;
    }

    uint32_t since = in_out_since != NULL ? *in_out_since : 0;
    size_t   count = 0;

    if (s_lock != NULL && xSemaphoreTake(s_lock, pdMS_TO_TICKS(20)) == pdTRUE) {
        /* Forwards from the oldest line, so what comes back is in the order it was written. */
        for (size_t i = 0; i < HAL_LOG_LINES && count < max; i++) {
            const hal_log_entry_t *line = &s_lines[(s_next + i) % HAL_LOG_LINES];

            if (line->seq != 0 && line->seq > since) {
                out[count++] = *line;
            }
        }

        xSemaphoreGive(s_lock);
    }

    if (in_out_since != NULL) {
        /* What the caller has now seen, whether or not it was sent: a page that asked for four lines
         * and got four does not want them again, and one that asked for more than there were has
         * seen everything. */
        *in_out_since = s_sequence - 1;
    }

    return count;
}
