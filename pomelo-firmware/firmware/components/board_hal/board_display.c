#include "board_hal_internal.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "esp_lcd_panel_ops.h"
#include "esp_lcd_panel_io.h"
#include "esp_lcd_co5300.h"
#include "esp_attr.h"
#include "driver/gpio.h"
#include "esp_timer.h"
#include "esp_rom_sys.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/semphr.h"
#include "bsp/esp-bsp.h"
#include "bsp/display.h"
#include "bsp/esp32_s3_touch_amoled_2_16.h"

/*
#if __has_include("splash_logo.h")
#include "splash_logo.h"
#define HAS_SPLASH_LOGO 1
#else
#define HAS_SPLASH_LOGO 0
#endif
*/

static const char *TAG = "board_display";

/* QSPI clock for the panel. 80 MHz is the ESP32-S3 SPI master's ceiling and the fastest this
 * CO5300 is driven on this board; the four data lines and the clock already run at maximum drive
 * strength (see the gpio_set_drive_capability calls in board_display_init) for clean edges.
 *
 * A full frame is 480 * 480 * 2 = 460.8 KB: 15.3 ms of pure transfer at 60 MHz, 11.5 ms at 80 MHz.
 * Nothing on this board is fast enough to hide 3.8 ms per full-screen flush in anything else.
 *
 * Anything above this is out of spec for the SPI master and will not clock at all. If a particular
 * board shows artefacts (sparkle, dropped columns, a bent image), 60 MHz is the known-good value to
 * fall back to. */
#define BOARD_DISPLAY_QSPI_PCLK_HZ (80 * 1000 * 1000)

static esp_lcd_panel_handle_t s_panel_handle = NULL;
static esp_lcd_panel_io_handle_t s_io_handle = NULL;
static SemaphoreHandle_t s_trans_done_sem = NULL;

#if defined(CONFIG_BOARD_LCD_TE_GPIO) && (CONFIG_BOARD_LCD_TE_GPIO >= 0)
#define BSP_LCD_TE CONFIG_BOARD_LCD_TE_GPIO
#define BSP_LCD_TE_ENABLED 1
#else
#define BSP_LCD_TE_ENABLED 0
#endif

static SemaphoreHandle_t s_te_sem = NULL;
static volatile uint32_t s_te_count = 0;
static bool s_te_active = false;
static int64_t s_last_frame_start_us = 0;
static bool s_display_active = false;
/* Whether this image has told the bootloader it came up, so that an over-the-air update is no longer
 * on probation. A flag of its own rather than `s_display_active`: that one is also what the idle
 * sleep and the page's power control drive, so a frame arriving after a wake would find it already
 * true and the confirmation would never be sent. See `board_hal_confirm_boot()`. */
static bool s_boot_confirmed = false;
/* Panel brightness, 0-255. Remembered so that waking the display does not silently slam it back to
 * full, and so a page can read back what it set. */
static uint8_t s_brightness = 255;

#if BSP_LCD_TE_ENABLED
static IRAM_ATTR void s_te_gpio_isr_handler(void *arg)
{
    s_te_count++;
    BaseType_t high_task_awoken = pdFALSE;
    if (s_te_sem) {
        xSemaphoreGiveFromISR(s_te_sem, &high_task_awoken);
    }
    if (high_task_awoken == pdTRUE) {
        portYIELD_FROM_ISR();
    }
}
#endif

void hal_display_wait_vsync(void)
{
    if (s_te_active && s_te_sem != NULL) {
        // Drain any stale V-sync pulses from previous frames
        xSemaphoreTake(s_te_sem, 0);
        // Wait for next hardware V-Blank rising edge (timeout 25ms in case a pulse is missed)
        if (xSemaphoreTake(s_te_sem, pdMS_TO_TICKS(25)) == pdTRUE) {
            s_last_frame_start_us = esp_timer_get_time();
            return;
        }
        ESP_LOGW(TAG, "Hardware TE timeout (>25ms), fallback to software 60Hz pacing");
        s_te_active = false;
    }

    // Adaptive 60Hz frame pacing fallback (16,667us per frame)
    const int64_t frame_interval_us = 16667;
    int64_t now_us = esp_timer_get_time();

    if (s_last_frame_start_us == 0) {
        s_last_frame_start_us = now_us;
        return;
    }

    int64_t elapsed_us = now_us - s_last_frame_start_us;
    if (elapsed_us < frame_interval_us) {
        int64_t wait_us = frame_interval_us - elapsed_us;
        // Yield FreeRTOS task if we have 2ms or more remaining
        if (wait_us >= 2000) {
            vTaskDelay(pdMS_TO_TICKS((wait_us - 1000) / 1000));
        }
        // Precision microsecond spin wait for exact target boundary
        while ((esp_timer_get_time() - s_last_frame_start_us) < frame_interval_us) {
            esp_rom_delay_us(50);
        }
        s_last_frame_start_us += frame_interval_us;
    } else {
        // Frame took longer than 16.67ms or was idle
        s_last_frame_start_us = esp_timer_get_time();
    }
}

#define CHUNK_STRIP_WIDTH_DEFAULT 64
static int s_chunk_strip_width = CHUNK_STRIP_WIDTH_DEFAULT;
static uint16_t *s_dma_chunk[2] = {NULL, NULL};

/* The last frame the panel was handed, kept so a page can be shown what the screen is showing.
 *
 * The panel is written once and never read back — the CO5300 has no path that returns its GRAM over
 * QSPI — so the only picture of the screen is the frame buffer the Rust side hands to every flush:
 * `flush_damage` in `platform.rs` sends the *whole* buffer plus the rectangle of it that changed, so
 * the damaged rectangle is already part of a complete frame and a screenshot can be read straight out
 * of the source. Keeping that pointer and its stride is the whole of what a capture needs.
 *
 * It is deliberately not copied on the way past. That copy is a full-screen PSRAM round trip sitting
 * in front of a transfer the panel is waiting for — 20.9 ms of a 44.6 ms full-screen `flush` on this
 * board, second only to the byte swap — and it was paid on every frame so that an occasional
 * `/api/screenshot` would find the picture ready. Reading the source where it lies moves that cost to
 * the request that actually wants it, and leaves the frame path carrying only what has to reach the
 * panel.
 *
 * The pointer belongs to the compositor's surface, and is re-recorded on every flush, so it follows a
 * surface that is rebuilt; it is written after the stride it belongs to, and read as a pair, so a
 * capture never pairs a new pointer with a stale stride. `NULL` until the first frame is sent, which
 * is the one state `hal_display_capture` reports. */
static const uint16_t *volatile s_last_pixels = NULL;
static volatile int32_t s_last_stride = 0;

// =============================================================================
// CO5300 AMOLED Panel Initialization Command Sequence
// =============================================================================
static const co5300_lcd_init_cmd_t s_lcd_init_cmds[] = {
    {0x11, (uint8_t[]){0x00}, 0, 120}, // Sleep out (requires 120ms standard delay per CO5300 datasheet)
    {0xFE, (uint8_t[]){0x20}, 1, 0},   // Select vendor command page 1
    {0x19, (uint8_t[]){0x10}, 1, 0},   // Vendor setting
    {0x1C, (uint8_t[]){0xA0}, 1, 0},   // Vendor setting
    {0xFE, (uint8_t[]){0x00}, 1, 0},   // Select user command page 0
    {0xC4, (uint8_t[]){0x80}, 1, 0},   // SPI mode / interface setting
    {0x3A, (uint8_t[]){0x55}, 1, 0},   // Interface pixel format: 16-bit RGB565
    {0x35, (uint8_t[]){0x00}, 1, 0},   // Tearing effect line output
    {0x53, (uint8_t[]){0x20}, 1, 0},   // Control display (dimming / brightness control enable)
    {0x51, (uint8_t[]){0x00}, 1, 0},   // Display brightness (0% initially to prevent power-on flash)
    {0x63, (uint8_t[]){0xFF}, 1, 0},   // HBM brightness setting
    {0x2A, (uint8_t[]){0x00, 0x00, 0x01, 0xDF}, 4, 0}, // Column address: 0 to 479 (0x01DF)
    {0x2B, (uint8_t[]){0x00, 0x00, 0x01, 0xDF}, 4, 0}, // Row address: 0 to 479 (0x01DF)
    {0x36, (uint8_t[]){0xA0}, 1, 0},   // Memory data access control (scan order / rotation)
    {0x28, (uint8_t[]){0x00}, 0, 0},   // Display OFF until first frame is presented
};

/* Words loaded before any of them is stored. Tuned against the `[bw]` probe in a `render-debug`
 * build, which is the only thing that can say whether a given width still has reads to hide: the
 * kernel is measured against a plain `memcpy` of the same bytes, and the gap between them is what
 * this number exists to close.
 *
 * Eight is worse than four, and measured so — 21.8 MB/s against 38.8. Xtensa has sixteen general
 * registers and eight words want most of them, so the batch spills to the stack and the extra traffic
 * costs more than the reads it was hiding. Four is where the register file runs out. */
#define SWAP_UNROLL 4

/* Swap the two 16-bit halves of each 32-bit word — two RGB565 pixels in one operation. */
static inline uint32_t bswap_pixel_pairs(uint32_t v)
{
    return ((v & 0x00FF00FFu) << 8) | ((v & 0xFF00FF00u) >> 8);
}

/**
 * @brief High-speed row copy with 32-bit SIMD RGB565 Little-to-Big Endian byte swapping.
 *
 * The swap is four ALU operations on a word whose load costs about sixteen cycles, so what this loop
 * is really waiting on is the load, and one iteration at a time it waits for every one of them: the
 * store cannot retire until the load it depends on has landed, which leaves a single read in flight.
 * Four words are therefore loaded before any of them is stored, which is what fills that gap.
 *
 * The measurement that says so is the `[bw]` line in a `render-debug` build, where the same kernel
 * over the same bytes runs at 31.8 MB/s read in address order against the 57.7 MB/s a plain `memcpy`
 * of those bytes gets — the difference being exactly this, since the flush's own walk was shown to be
 * worth only 7% by the `swap-seq` and `swap-strip` pair.
 */
static inline void copy_row_rgb565_be(const uint16_t *src, uint16_t *dst, int32_t width)
{
    if (((uintptr_t)src & 3u) == 0 && ((uintptr_t)dst & 3u) == 0) {
        const uint32_t *src32 = (const uint32_t *)src;
        uint32_t *dst32 = (uint32_t *)dst;
        const int32_t words = width / 2;
        int32_t i = 0;
        for (; i + SWAP_UNROLL <= words; i += SWAP_UNROLL) {
            const uint32_t a = src32[i];
            const uint32_t b = src32[i + 1];
            const uint32_t c = src32[i + 2];
            const uint32_t d = src32[i + 3];
            dst32[i] = bswap_pixel_pairs(a);
            dst32[i + 1] = bswap_pixel_pairs(b);
            dst32[i + 2] = bswap_pixel_pairs(c);
            dst32[i + 3] = bswap_pixel_pairs(d);
        }
        for (; i < words; ++i) {
            dst32[i] = bswap_pixel_pairs(src32[i]);
        }
        if (width & 1) {
            dst[width - 1] = __builtin_bswap16(src[width - 1]);
        }
    } else {
        for (int32_t i = 0; i < width; ++i) {
            dst[i] = __builtin_bswap16(src[i]);
        }
    }
}

#if BOARD_DISPLAY_PROFILE
/* One-off, in a measuring build only: is the byte swap slow because PSRAM is slow, or because of the
 * order the strip walk reads it in?
 *
 * The comment on the strip ladder above asserts the former — "bounded by PSRAM access time" — and the
 * flush probe agrees on the size of the term (18.8 ms for 460 KB, 81 ns/pixel, against the 11.5 ms the
 * QSPI link needs to carry the same bytes). What neither can say is *why*, and the two answers want
 * opposite fixes: a bandwidth floor means the swap has to go away entirely, while a pattern penalty
 * means it only has to be reordered — `swap-strip` below runs the walk the flush runs, `swap-seq`
 * runs the identical kernel over the identical bytes read in address order, and the gap between the
 * two is the part of the 18.8 ms that is the traversal rather than the memory.
 *
 * The destination is the caller's first strip buffer, reused per strip exactly as the flush reuses it,
 * so this measures the real thing and allocates nothing for it. */
static void probe_bandwidth(size_t chunk, void *sram_raw)
{
    uint8_t *sram = (uint8_t *)sram_raw;
    const size_t total = (size_t)BOARD_DISPLAY_WIDTH * BOARD_DISPLAY_HEIGHT * sizeof(uint16_t);
    uint8_t *ps_src = (uint8_t *)heap_caps_aligned_alloc(64, total, MALLOC_CAP_SPIRAM);
    uint8_t *ps_dst = (uint8_t *)heap_caps_aligned_alloc(64, total, MALLOC_CAP_SPIRAM);

    if (ps_src == NULL || ps_dst == NULL || sram == NULL || chunk == 0 || chunk > total) {
        ESP_LOGW(TAG, "[bw] skipped: PSRAM staging did not fit");
        if (ps_src) free(ps_src);
        if (ps_dst) free(ps_dst);
        return;
    }

    // Back every page before timing, so no first-touch mapping lands inside a measurement.
    memset(ps_src, 0xA5, total);
    memset(ps_dst, 0x5A, total);
    memset(sram, 0, chunk);

    int64_t t;

    // Both sides PSRAM, sequential: the ceiling the rest is read against.
    t = esp_timer_get_time();
    for (size_t off = 0; off < total; off += chunk) {
        memcpy(ps_dst + off, ps_src + off, (total - off < chunk) ? total - off : chunk);
    }
    const int64_t t_pp = esp_timer_get_time() - t;

    // PSRAM read on its own.
    t = esp_timer_get_time();
    for (size_t off = 0; off < total; off += chunk) {
        memcpy(sram, ps_src + off, (total - off < chunk) ? total - off : chunk);
    }
    const int64_t t_ps = esp_timer_get_time() - t;

    // PSRAM write on its own.
    t = esp_timer_get_time();
    for (size_t off = 0; off < total; off += chunk) {
        memcpy(ps_dst + off, sram, (total - off < chunk) ? total - off : chunk);
    }
    const int64_t t_sp = esp_timer_get_time() - t;

    // The same kernel on the same bytes, read in address order.
    t = esp_timer_get_time();
    for (size_t off = 0; off < total; off += chunk) {
        const size_t n = (total - off < chunk) ? total - off : chunk;
        copy_row_rgb565_be((const uint16_t *)(ps_src + off), (uint16_t *)sram, (int32_t)(n / 2));
    }
    const int64_t t_swap_seq = esp_timer_get_time() - t;

    // The walk the flush actually makes: 32-column strips out of a 480-wide surface, so each row is a
    // 64-byte read a 960-byte stride from the last. Right to left, as the flush scans.
    t = esp_timer_get_time();
    for (int32_t x = BOARD_DISPLAY_WIDTH; x > 0; ) {
        const int32_t cx1 = (x - 32 >= 0) ? (x - 32) : 0;
        const int32_t w = x - cx1;
        for (int32_t r = 0; r < BOARD_DISPLAY_HEIGHT; ++r) {
            const uint16_t *in = (const uint16_t *)ps_src + (size_t)r * BOARD_DISPLAY_WIDTH + (size_t)cx1;
            copy_row_rgb565_be(in, (uint16_t *)sram, w);
        }
        x = cx1;
    }
    const int64_t t_swap_strip = esp_timer_get_time() - t;

    printf("[bw] psram->psram %.1f  psram->sram %.1f  sram->psram %.1f  swap-seq %.1f  swap-strip %.1f MB/s  (%u bytes)\n",
           (double)total / (double)t_pp,
           (double)total / (double)t_ps,
           (double)total / (double)t_sp,
           (double)total / (double)t_swap_seq,
           (double)total / (double)t_swap_strip,
           (unsigned)total);

    free(ps_src);
    free(ps_dst);
}
#endif

static IRAM_ATTR bool s_on_color_trans_done(esp_lcd_panel_io_handle_t panel_io,
                                            esp_lcd_panel_io_event_data_t *edata,
                                            void *user_ctx)
{
    BaseType_t high_task_awoken = pdFALSE;
    if (s_trans_done_sem) {
        xSemaphoreGiveFromISR(s_trans_done_sem, &high_task_awoken);
    }
    return high_task_awoken == pdTRUE;
}

esp_err_t board_display_init(void)
{
    if (s_panel_handle != NULL) {
        return ESP_OK;
    }

    /* Ping-pong strips of internal DMA SRAM: each is the panel's full height by the strip width, and
     * there are two so that the byte swap of the next strip overlaps the transfer of the last. At 64
     * columns that is 2 * 60 KB = 120 KB.
     *
     * Allocated first, ahead of the SPI bus and the panel controller, and it is worth knowing that
     * this buys nothing. Moving it here from the foot of this function — where it used to run, once
     * the panel was up — took the free DMA heap from 153535 bytes to 158151 and left the largest free
     * block at exactly 73728 either way. That number is a boundary of the internal heap's own regions
     * and not the residue of fragmentation, so allocating earlier does not reach past it. Two 60 KB
     * strips need 122880 contiguous bytes and the second one never lands, which is why this board runs
     * 32-column strips (15 across the panel) rather than the 64 it asks for first.
     *
     * The widths stay a ladder rather than being fixed at 32 because trying costs nothing and a board
     * with a different internal layout would take the wider rung. The fallback is not the loss it
     * looks like either: the swap is bounded by PSRAM access time and not by how many columns a call
     * receives, and measured across rectangles of every size on this board it runs at 71-84 ns per
     * pixel — the per-call fixed cost a wider strip would amortise is the smaller part of that. */
    static const int strip_candidates[] = {64, 48, 32};
    size_t chunk_bytes = 0;
    for (size_t i = 0; i < sizeof(strip_candidates) / sizeof(strip_candidates[0]); ++i) {
        const int strip = strip_candidates[i];
        const size_t bytes = (size_t)BOARD_DISPLAY_HEIGHT * (size_t)strip * sizeof(uint16_t);

        ESP_LOGI(TAG, "DMA strips: trying %d columns (%u bytes each; internal DMA free %u, largest %u).",
                 strip, (unsigned)bytes,
                 (unsigned)heap_caps_get_free_size(MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL),
                 (unsigned)heap_caps_get_largest_free_block(MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL));

        s_dma_chunk[0] = (uint16_t *)heap_caps_aligned_alloc(64, bytes, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
        s_dma_chunk[1] = (uint16_t *)heap_caps_aligned_alloc(64, bytes, MALLOC_CAP_DMA | MALLOC_CAP_INTERNAL);
        if (s_dma_chunk[0] != NULL && s_dma_chunk[1] != NULL) {
            s_chunk_strip_width = strip;
            chunk_bytes = bytes;
            break;
        }

        // Two of this width did not fit. Hand back whichever one landed and try the next step down.
        if (s_dma_chunk[0]) { free(s_dma_chunk[0]); s_dma_chunk[0] = NULL; }
        if (s_dma_chunk[1]) { free(s_dma_chunk[1]); s_dma_chunk[1] = NULL; }
    }

    if (s_dma_chunk[0] == NULL || s_dma_chunk[1] == NULL) {
        ESP_LOGE(TAG, "No internal DMA memory for a ping-pong pair; the panel cannot be driven.");
        return ESP_ERR_NO_MEM;
    }

    ESP_LOGI(TAG, "DMA ping-pong buffers initialized: %d column strips (%u bytes each).",
             s_chunk_strip_width, (unsigned)chunk_bytes);

#if BOARD_DISPLAY_PROFILE
    // Here rather than later because the strip buffer it borrows is allocated and the panel is not yet
    // drawing into it. Measuring and not acting: the numbers say which of the two fixes the swap
    // wants, and neither is safe to guess at.
    probe_bandwidth(chunk_bytes, s_dma_chunk[0]);
#endif

    ESP_LOGI(TAG, "Initializing Waveshare 480x480 AMOLED display at %dMHz QSPI (high drive strength)...",
             BOARD_DISPLAY_QSPI_PCLK_HZ / 1000000);
    const spi_bus_config_t buscfg = CO5300_PANEL_BUS_QSPI_CONFIG(BSP_LCD_PCLK,
                                                                 BSP_LCD_DATA0,
                                                                 BSP_LCD_DATA1,
                                                                 BSP_LCD_DATA2,
                                                                 BSP_LCD_DATA3,
                                                                 BSP_LCD_H_RES * BSP_LCD_V_RES * sizeof(uint16_t));
    esp_err_t ret = spi_bus_initialize(BSP_LCD_SPI_NUM, &buscfg, SPI_DMA_CH_AUTO);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "spi_bus_initialize failed: %s", esp_err_to_name(ret));
        return ret;
    }

    // Boost QSPI GPIO output drive capability to maximum (40mA, Cap 3) for clean 80MHz signal edges
    gpio_set_drive_capability(BSP_LCD_PCLK, GPIO_DRIVE_CAP_3);
    gpio_set_drive_capability(BSP_LCD_DATA0, GPIO_DRIVE_CAP_3);
    gpio_set_drive_capability(BSP_LCD_DATA1, GPIO_DRIVE_CAP_3);
    gpio_set_drive_capability(BSP_LCD_DATA2, GPIO_DRIVE_CAP_3);
    gpio_set_drive_capability(BSP_LCD_DATA3, GPIO_DRIVE_CAP_3);
    gpio_set_drive_capability(BSP_LCD_CS, GPIO_DRIVE_CAP_3);

    s_trans_done_sem = xSemaphoreCreateBinary();
    s_te_sem = xSemaphoreCreateBinary();

    // Configure TE GPIO interrupt only if explicitly mapped (e.g. via TP301 jumper wire)
#if BSP_LCD_TE_ENABLED
    gpio_config_t te_gpio_cfg = {
        .pin_bit_mask = BIT64(BSP_LCD_TE),
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE,
        .pull_down_en = GPIO_PULLDOWN_ENABLE,
        .intr_type = GPIO_INTR_POSEDGE,
    };
    gpio_config(&te_gpio_cfg);
    esp_err_t isr_err = gpio_install_isr_service(0);
    if (isr_err != ESP_OK && isr_err != ESP_ERR_INVALID_STATE) {
        ESP_LOGW(TAG, "gpio_install_isr_service: %s", esp_err_to_name(isr_err));
    }
    gpio_isr_handler_add(BSP_LCD_TE, s_te_gpio_isr_handler, NULL);
#endif

    esp_lcd_panel_io_spi_config_t io_config = CO5300_PANEL_IO_QSPI_CONFIG(BSP_LCD_CS, s_on_color_trans_done, NULL);
    io_config.pclk_hz = BOARD_DISPLAY_QSPI_PCLK_HZ;
    io_config.trans_queue_depth = 10;

    co5300_vendor_config_t vendor_config = {
        .init_cmds = s_lcd_init_cmds,
        .init_cmds_size = sizeof(s_lcd_init_cmds) / sizeof(s_lcd_init_cmds[0]),
        .flags = {
            .use_qspi_interface = 1,
        },
    };
    ret = esp_lcd_new_panel_io_spi((esp_lcd_spi_bus_handle_t)BSP_LCD_SPI_NUM, &io_config, &s_io_handle);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_lcd_new_panel_io_spi failed: %s", esp_err_to_name(ret));
        return ret;
    }

    const esp_lcd_panel_dev_config_t panel_config = {
        .reset_gpio_num = BSP_LCD_RST,
        .rgb_ele_order = LCD_RGB_ELEMENT_ORDER_RGB,
        .bits_per_pixel = BSP_LCD_BITS_PER_PIXEL,
        .vendor_config = &vendor_config,
    };
    ret = esp_lcd_new_panel_co5300(s_io_handle, &panel_config, &s_panel_handle);
    if (ret != ESP_OK) {
        ESP_LOGE(TAG, "esp_lcd_new_panel_co5300 failed: %s", esp_err_to_name(ret));
        return ret;
    }

    esp_lcd_panel_reset(s_panel_handle);
    esp_lcd_panel_init(s_panel_handle);
    // Keep display blanked/off until first valid frame is written to prevent power-on green flash
    esp_lcd_panel_disp_on_off(s_panel_handle, false);
    ESP_LOGI(TAG, "AMOLED panel controller initialized (standby mode, blanked).");

    // Probe hardware TE signal only if mapped
#if BSP_LCD_TE_ENABLED
    vTaskDelay(pdMS_TO_TICKS(50));
    if (s_te_count >= 2) {
        s_te_active = true;
        ESP_LOGI(TAG, "Hardware TE V-Sync interrupt active on GPIO%d (received %lu pulses).", (int)BSP_LCD_TE, (unsigned long)s_te_count);
    } else {
        s_te_active = false;
        ESP_LOGI(TAG, "Hardware TE not detected on GPIO%d. Using adaptive 60Hz frame pacing.", (int)BSP_LCD_TE);
    }
#else
    s_te_active = false;
    ESP_LOGI(TAG, "Hardware TE line not routed to GPIO (test point TP301 on PCB). Using adaptive 60Hz frame pacing.");
#endif
    s_last_frame_start_us = esp_timer_get_time();

    // Zero-clear entire GRAM to pure black (0x0000) so uninitialized memory is never visible
    memset(s_dma_chunk[0], 0, chunk_bytes);
    for (int32_t x = 0; x < BOARD_DISPLAY_WIDTH; x += s_chunk_strip_width) {
        esp_lcd_panel_draw_bitmap(s_panel_handle, x, 0, x + s_chunk_strip_width, BOARD_DISPLAY_HEIGHT, s_dma_chunk[0]);
        xSemaphoreTake(s_trans_done_sem, portMAX_DELAY);
    }

/*
#if HAS_SPLASH_LOGO
    // Draw centered "Pomelo UI" splash logo
    int32_t splash_x1 = (BOARD_DISPLAY_WIDTH - SPLASH_LOGO_WIDTH) / 2;
    int32_t splash_y1 = (BOARD_DISPLAY_HEIGHT - SPLASH_LOGO_HEIGHT) / 2;
    int32_t splash_x2 = splash_x1 + SPLASH_LOGO_WIDTH;
    int32_t splash_y2 = splash_y1 + SPLASH_LOGO_HEIGHT;

    // CO5300 QSPI requires 2-pixel alignment
    splash_x1 = (splash_x1 >> 1) << 1;
    splash_y1 = (splash_y1 >> 1) << 1;
    splash_x2 = ((splash_x2 + 1) >> 1) << 1;
    splash_y2 = ((splash_y2 + 1) >> 1) << 1;

    size_t splash_bytes = sizeof(s_splash_logo);
    if (splash_bytes <= chunk_bytes) {
        memcpy(s_dma_chunk[0], s_splash_logo, splash_bytes);
        esp_lcd_panel_draw_bitmap(s_panel_handle, splash_x1, splash_y1, splash_x2, splash_y2, s_dma_chunk[0]);
        xSemaphoreTake(s_trans_done_sem, portMAX_DELAY);
    }

    // Turn on display and full brightness seamlessly now that splash logo is in GRAM
    esp_lcd_panel_disp_on_off(s_panel_handle, true);
    uint32_t lcd_cmd = 0x51;
    lcd_cmd &= 0xff;
    lcd_cmd <<= 8;
    lcd_cmd |= 0x02 << 24;
    uint8_t param = 255;
    esp_lcd_panel_io_tx_param(s_io_handle, lcd_cmd, &param, 1);
    s_display_active = true;
    ESP_LOGI(TAG, "AMOLED display output enabled with Pomelo UI splash screen.");
#endif
*/

    return ESP_OK;
}

/* The `[send]` line: one flush split into the passes it is actually made of.
 *
 * The Rust side prints a `[flush]` line for the whole of `flush_damage`, which is the v-blank wait
 * and the transfer together. That line is what made the transfer visible at all, and what it says is
 * that a full-screen frame spends ~44 ms in it against the 11.5 ms the QSPI link needs to carry the
 * 460 KB — so two thirds of the transfer is not the transfer. What it cannot say is *which* of the
 * passes inside this function owns that third, and the answer this probe first produced was the byte
 * swap: `copy_row_rgb565_be` moving the rectangle through PSRAM, which is most of the 44 ms, ahead of
 * a transfer that is then left with almost nothing to wait for.
 *
 * So this splits that gap into the passes it can — the byte swap into the DMA strips, the wait for a
 * strip to drain, and whatever is left over, which is `esp_lcd_panel_draw_bitmap`'s own work on the
 * calling thread; a total that is much larger than the sum is a finding in itself.
 *
 * `shadow` is kept as a field so that this log can be read against the ones taken before the shadow
 * frame was removed, and it prints 0.00 in every build now. The pass it timed was a full-screen PSRAM
 * round trip in front of every flush — 20.92 ms of the 44.62 ms line above — paid on every frame so
 * that an occasional `/api/screenshot` would find a picture ready. A capture now reads the frame
 * buffer the flush was handed (see `s_last_pixels`), so that cost sits on the request instead of the
 * frame. Read `swap` next: it is the same kind of PSRAM pass and the largest term that remains.
 *
 * The probe is off unless the build asks for it: see the `BOARD_DISPLAY_PROFILE` in
 * `board_hal/CMakeLists.txt`, which is `render-debug` on the Rust side too. It is measurement
 * scaffolding and not a feature. */
#ifndef BOARD_DISPLAY_PROFILE
#define BOARD_DISPLAY_PROFILE 0
#endif

void hal_display_draw_bitmap(int32_t x1, int32_t y1, int32_t x2, int32_t y2, const uint16_t *pixels,
                              int32_t stride)
{
    if (!s_panel_handle || !pixels || !s_dma_chunk[0] || !s_dma_chunk[1]) {
        return;
    }
    if (x1 < 0 || y1 < 0 || x2 > BOARD_DISPLAY_WIDTH || y2 > BOARD_DISPLAY_HEIGHT || x1 >= x2 || y1 >= y2) {
        return;
    }
    if (stride < x2) {
        return;
    }

    // Hardware constraint: CO5300 QSPI controller requires 2-pixel aligned coordinates
    x1 = (x1 >> 1) << 1;
    y1 = (y1 >> 1) << 1;
    x2 = ((x2 + 1) >> 1) << 1;
    y2 = ((y2 + 1) >> 1) << 1;
    if (x2 > BOARD_DISPLAY_WIDTH) x2 = BOARD_DISPLAY_WIDTH;
    if (y2 > BOARD_DISPLAY_HEIGHT) y2 = BOARD_DISPLAY_HEIGHT;

    const int32_t width = x2 - x1;
    const int32_t height = y2 - y1;
    if (width <= 0 || height <= 0) {
        return;
    }

#if BOARD_DISPLAY_PROFILE
    // Started after the early returns so the total covers exactly the work below, and read against
    // the same microsecond clock `hal_display_wait_vsync` paces frames with. Three probes per call:
    // this is a handful of readings against tens of milliseconds, and the numbers it produces are the
    // ones that decide what to change.
    const int64_t probe_start_us = esp_timer_get_time();
    int64_t shadow_us = 0;
    int64_t swap_us = 0;
    int64_t dma_us = 0;
    int32_t strips = 0;
#endif

    // Only auto-synchronize if no frame pacing timestamp was recorded at all
    if (s_last_frame_start_us == 0) {
        hal_display_wait_vsync();
    }

    int buf_idx = 0;
    bool dma_pending = false;

    // Scan direction matching physical Gate driver scan: Right to Left (x2 -> x1).
    // Slicing vertically along X makes DMA progression parallel to the physical OLED scan,
    // breaking the 45-degree diagonal tearing plane into a parallel beam chasing model.
    for (int32_t x = x2; x > x1; ) {
        int32_t cur_x1 = (x - s_chunk_strip_width >= x1) ? (x - s_chunk_strip_width) : x1;
        int32_t cur_x2 = x;
        int32_t strip_w = cur_x2 - cur_x1;

        // Byte-swap into the ping-pong chunk buffer
#if BOARD_DISPLAY_PROFILE
        const int64_t probe_t0 = esp_timer_get_time();
#endif
        for (int32_t r = 0; r < height; ++r) {
            const uint16_t *in = pixels + (size_t)(y1 + r) * (size_t)stride + (size_t)cur_x1;
            uint16_t *out = s_dma_chunk[buf_idx] + (size_t)r * (size_t)strip_w;
            copy_row_rgb565_be(in, out, strip_w);
        }
#if BOARD_DISPLAY_PROFILE
        swap_us += esp_timer_get_time() - probe_t0;
        strips++;
#endif

        // Wait for previous DMA transaction to complete before queuing new one
        if (dma_pending) {
#if BOARD_DISPLAY_PROFILE
            const int64_t probe_w0 = esp_timer_get_time();
#endif
            xSemaphoreTake(s_trans_done_sem, portMAX_DELAY);
#if BOARD_DISPLAY_PROFILE
            dma_us += esp_timer_get_time() - probe_w0;
#endif
            dma_pending = false;
        }

        // Send over QSPI via DMA
        esp_lcd_panel_draw_bitmap(s_panel_handle, cur_x1, y1, cur_x2, y2, s_dma_chunk[buf_idx]);
        dma_pending = true;

        buf_idx = 1 - buf_idx;
        x = cur_x1;
    }

    // Wait for the final chunk's DMA transfer to finish
    if (dma_pending) {
#if BOARD_DISPLAY_PROFILE
        const int64_t probe_w0 = esp_timer_get_time();
#endif
        xSemaphoreTake(s_trans_done_sem, portMAX_DELAY);
#if BOARD_DISPLAY_PROFILE
        dma_us += esp_timer_get_time() - probe_w0;
#endif
    }

#if BOARD_DISPLAY_PROFILE
    {
        const int64_t send_us = esp_timer_get_time() - probe_start_us;
        int64_t other_us = send_us - (shadow_us + swap_us + dma_us);

        // Never printed below zero: the three probes are separate readings of a microsecond clock, so
        // their sum can land a few microseconds past the total that contains them, and a negative
        // remainder would read as one of the measurements having gone backwards.
        if (other_us < 0) {
            other_us = 0;
        }

        const double px = (double)width * (double)height;

        /* One line per call, so one per region the Rust side planned — and on almost every frame that
         * is one region, which is what makes these four terms the `send` of that frame's `[flush]`
         * line, less the loop over regions around them.
         *
         * `dma` is time *blocked*, not time transferred: the swap of the next strip is done before the
         * previous strip's transfer is waited on, so the two overlap by design and only what the
         * overlap could not hide shows up here. Adding the three together is therefore sound rather
         * than double-counting.
         *
         * `MB/s` counts the pixels this call carried against its whole total, so it is a floor on the
         * wire and not the wire: a full-screen call is 11.5 ms of QSPI at the link's 80 MHz. */
        printf("[send] shadow %.2f  swap %.2f  dma %.2f  other %.2f  total %.2fms  px %d  strips %d  %.1f MB/s\n",
               (double)shadow_us / 1000.0,
               (double)swap_us / 1000.0,
               (double)dma_us / 1000.0,
               (double)other_us / 1000.0,
               (double)send_us / 1000.0,
               (int)(width * height),
               (int)strips,
               send_us > 0 ? px * 2.0 / ((double)send_us / 1000000.0) / 1000000.0 : 0.0);

        // Flushed here and not left to the console's own buffering, so that the line lands before the
        // `[flush]` line the Rust side prints for the same frame a moment later: it is `printf`'s
        // buffer against a plain write to stdout on the other side of the FFI, and reordering the two
        // would make this line look like it belonged to the frame after the one it explains.
        fflush(stdout);
    }
#endif

    // What the panel was just given, for a capture to read later. Recorded once the last strip has
    // drained rather than before the loop, so a screenshot sees a frame the panel has already taken
    // and not one still being assembled. Stride first and pointer second: a capture that runs between
    // the two stores takes the pointer of the previous frame with the width of this one, and the width
    // is the panel's in every frame the compositor can produce.
    s_last_stride = stride;
    s_last_pixels = pixels;

    // Turn on display and full brightness seamlessly once the first valid frame is written
    if (!s_display_active) {
        esp_lcd_panel_disp_on_off(s_panel_handle, true);
        uint32_t lcd_cmd = ((uint32_t)0x51 << 8) | ((uint32_t)0x02 << 24);
        esp_lcd_panel_io_tx_param(s_io_handle, lcd_cmd, &s_brightness, 1);
        s_display_active = true;
        ESP_LOGI(TAG, "AMOLED display output enabled on first frame.");
    }

    // A frame is on the glass, so the box is up: this is where an image that arrived over the air
    // stops being on probation. Everything an update can break has run by now — the board comes up,
    // the fonts behind the text are where they should be, the renderer draws, and the panel takes the
    // frame — and an image that died anywhere earlier never reaches this line, which is exactly what
    // makes it a rollback. Once per boot; see `board_hal_confirm_boot()`.
    if (!s_boot_confirmed) {
        s_boot_confirmed = true;
        board_hal_confirm_boot();
    }
}

void hal_display_set_power(bool on)
{
    if (s_panel_handle) {
        ESP_LOGI(TAG, "Setting AMOLED display power: %s", on ? "ON" : "OFF");
        esp_lcd_panel_disp_on_off(s_panel_handle, on);
        s_display_active = on;
    }
}

esp_err_t hal_display_set_brightness(uint8_t level)
{
    if (!s_panel_handle || !s_io_handle) {
        return ESP_ERR_INVALID_STATE;
    }
    s_brightness = level;
    // 0x51 is the CO5300's brightness register. The command word carries the `0x02` QSPI opcode in
    // its top byte, exactly as the power-on path above sends it.
    uint32_t lcd_cmd = ((uint32_t)0x51 << 8) | ((uint32_t)0x02 << 24);
    return esp_lcd_panel_io_tx_param(s_io_handle, lcd_cmd, &s_brightness, 1);
}

uint8_t hal_display_get_brightness(void)
{
    return s_brightness;
}

bool hal_display_is_powered(void)
{
    return s_display_active;
}

bool hal_display_has_capture(void)
{
    // A frame has to have reached the panel before there is anything to read: until the first flush
    // there is no frame buffer to point at, which is the one way a capture can be unavailable now.
    return s_last_pixels != NULL;
}

esp_err_t hal_display_capture(uint16_t *out, size_t pixels)
{
    if (out == NULL) {
        return ESP_ERR_INVALID_ARG;
    }
    if (pixels < (size_t)BOARD_DISPLAY_WIDTH * BOARD_DISPLAY_HEIGHT) {
        return ESP_ERR_INVALID_SIZE;
    }

    // Read as the pair the flush wrote them in, so the pointer is never paired with a stale stride.
    const uint16_t *src = s_last_pixels;
    const int32_t stride = s_last_stride;
    if (src == NULL || stride < BOARD_DISPLAY_WIDTH) {
        return ESP_ERR_INVALID_STATE;
    }

    /* Row by row, because the frame buffer is a stride wide and `out` is not. This is the whole cost
     * of a capture now: one pass over the screen, on the request that wanted the picture, instead of
     * one on every frame in case one might. */
    for (int32_t row = 0; row < BOARD_DISPLAY_HEIGHT; ++row) {
        memcpy(out + (size_t)row * BOARD_DISPLAY_WIDTH,
               src + (size_t)row * (size_t)stride,
               (size_t)BOARD_DISPLAY_WIDTH * sizeof(uint16_t));
    }

    return ESP_OK;
}

