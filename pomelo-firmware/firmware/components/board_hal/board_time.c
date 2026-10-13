// =============================================================================
// Network time (SNTP)
// -----------------------------------------------------------------------------
// The one thing that puts a real time into the board's clock, and therefore the
// one thing HTTPS was waiting for: mbedTLS judges a certificate against the
// system clock, and a board fresh out of the box reads 1970 — before every
// certificate ever issued — so every handshake fails until this has run once.
//
// Started from board_wifi.c's IP_EVENT_STA_GOT_IP handler, because an address is
// what a time server needs. It does not block and it does not wait: the SNTP
// client runs on a task of its own and retries on its own, and the time it finds
// reaches the chip through settimeofday — which board_rtc.c wraps for exactly
// this, so it is also written to the hardware RTC and survives the next power
// cycle.
//
// Nothing reads time through this file. `hal_system_get_epoch` is the reading and
// it reads the RTC; this is only what fills it.
// =============================================================================

#include <stdbool.h>

#include "esp_err.h"
#include "esp_log.h"
#include "esp_netif_sntp.h"

#include "board_hal.h"

static const char *TAG = "board_time";

/** The pool, and not one host: a single owner's outage should not leave this board without a clock. */
#define HAL_TIME_SERVER "pool.ntp.org"

/** Set once, under the event task that calls this. A second GOT_IP after a reconnect must not start a
 * second client, and must not restart the one already running — it is the same clock either way. */
static bool s_started = false;

esp_err_t hal_time_sync(void)
{
    if (s_started) {
        return ESP_OK;
    }

    /* `start = true` and `wait_for_sync = true` are the defaults this macro carries: the client is
     * started here, and the sync is not waited for. */
    const esp_sntp_config_t config = ESP_NETIF_SNTP_DEFAULT_CONFIG(HAL_TIME_SERVER);

    const esp_err_t ret = esp_netif_sntp_init(&config);
    if (ret != ESP_OK) {
        ESP_LOGW(TAG, "esp_netif_sntp_init(%s): %s", HAL_TIME_SERVER, esp_err_to_name(ret));
        return ret;
    }

    s_started = true;
    ESP_LOGI(TAG, "SNTP started (%s)", HAL_TIME_SERVER);
    return ESP_OK;
}
