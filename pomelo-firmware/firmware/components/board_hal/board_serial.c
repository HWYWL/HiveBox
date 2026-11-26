/*
 * 串口回声：从 USB 串口收到什么，就回一行「收到数据 ===> 什么」。
 *
 * # 为什么是 USB 串口，不是 UART
 *
 * 这块板子的控制台就挂在芯片自带的 USB Serial/JTAG 上（`CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG`），
 * 插上数据线、开个串口工具就是它，不用另找引脚。要把回声改到某一路 UART 上，只需要把下面三处
 * `usb_serial_jtag_*` 换成 `uart_*`（并决定用哪两个 GPIO），其余逻辑不用动。
 *
 * # 为什么要有这么个东西
 *
 * 这块板子的屏幕出过问题，而「应用到底跑没跑起来」在屏幕黑着的时候无从判断。一行回声是最便宜的
 * 判据：能回，就说明固件在跑、串口通、任务调度正常；不回，问题就在这三者之前。
 *
 * # 和日志的关系
 *
 * 日志（`board_log.c`）是同一根线上的输出，回声也是输出 —— 两者会交替出现，这没问题。回声走
 * 驱动自己的写函数，不经过 `printf`，免得和 IDF 控制台抢同一个缓冲区。
 */
#include "board_hal.h"

#include "driver/usb_serial_jtag.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include <stdio.h>
#include <string.h>

/** 一次最多处理多少字节。超过这个长度的输入分成几次回，不会被丢掉。 */
#define HAL_SERIAL_CHUNK 128

/** 回声任务的栈。就是读、拼一行、写回去，四千字节绰绰有余。 */
#define HAL_SERIAL_STACK 4096

static const char *TAG = "board_serial";

static TaskHandle_t s_task;

static void serial_task(void *argument)
{
    (void)argument;

    char received[HAL_SERIAL_CHUNK + 1];
    char answer[HAL_SERIAL_CHUNK + 32];

    for (;;) {
        const int length = usb_serial_jtag_read_bytes(received, HAL_SERIAL_CHUNK, pdMS_TO_TICKS(100));

        if (length <= 0) {
            continue;
        }

        received[length] = '\0';

        /* 行尾的换行由我们自己补：串口工具按一次回车，不该回出两行。 */
        int trimmed = length;

        while (trimmed > 0 && (received[trimmed - 1] == '\n' || received[trimmed - 1] == '\r')) {
            received[--trimmed] = '\0';
        }

        const int written = snprintf(answer, sizeof answer, "收到数据 ===> %s\n", received);

        if (written > 0) {
            /* 写不进去（主机没在读）就丢掉这一次：回声是给人看的，不该把任务卡在这儿。 */
            (void)usb_serial_jtag_write_bytes(answer, (size_t)written, pdMS_TO_TICKS(100));
        }
    }
}

esp_err_t hal_serial_init(void)
{
    if (s_task != NULL) {
        return ESP_OK;
    }

    usb_serial_jtag_driver_config_t config = {
        .tx_buffer_size = 512,
        .rx_buffer_size = 1024,
    };

    const esp_err_t err = usb_serial_jtag_driver_install(&config);

    /* 控制台可能已经把驱动装过了 —— 那不是错误，读写的接口照样在。 */
    if (err != ESP_OK && err != ESP_ERR_INVALID_STATE) {
        ESP_LOGW(TAG, "usb serial driver: %s", esp_err_to_name(err));
        return err;
    }

    if (xTaskCreate(serial_task, "serial_echo", HAL_SERIAL_STACK, NULL, 3, &s_task) != pdPASS) {
        s_task = NULL;
        return ESP_ERR_NO_MEM;
    }

    ESP_LOGI(TAG, "串口回声已就绪：发什么，回「收到数据 ===> 什么」");

    return ESP_OK;
}
