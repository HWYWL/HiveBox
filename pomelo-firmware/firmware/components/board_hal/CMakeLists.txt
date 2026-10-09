# The C side of the Rust FFI boundary.
#
# `board_hal.c` and its sub-modules define the `hal_*` shims (display, touch, buttons, audio, wifi, power);
# `pomelo-hal-esp32` and `rust_main` call them, which is to say `librust_main.a` does.
#
# Hence `WHOLE_ARCHIVE`: the linker walks static archives in link order and once,
# and ESP-IDF does not put the component archives in a group (only the C++ runtime
# libs and a couple of managed components get one). Leaving this as part of `main`
# put `libmain.a` *before* `librust_main.a`, so nothing had asked for `hal_wifi_*`
# when that archive was read and its members were never pulled in — the link then
# died on `undefined reference to hal_wifi_scan_start`. Naming this component in
# `rust_main`'s `REQUIRES` does not move it later (measured: it still came first),
# so the objects are forced in instead; the definitions are then present before
# anything can ask for them, whatever order IDF chooses.
idf_component_register(
    SRCS
        "board_hal.c"
        "board_log.c"
        "board_event.c"
        "board_display.c"
        "board_touch.c"
        "board_button.c"
        "board_power.c"
        "board_audio.c"
        "board_wifi.c"
        "board_rtc.c"
        "board_storage.c"
        "board_web.c"
    INCLUDE_DIRS "."
    WHOLE_ARCHIVE
    # The management page, as bytes in the image: it has to be there before anything can ask for it,
    # and the filesystem it manages is not a place it can live (see the top of `board_web.c`).
    # `EMBED_FILES` and not `EMBED_TXTFILES`: the length served is `end - start`, exactly.
    EMBED_FILES "web/web_page.html"
    REQUIRES
        waveshare__esp32_s3_touch_amoled_2_16
        waveshare__pcf85063a
        espressif__esp_codec_dev
        espressif__esp_lcd_co5300
        esp_lcd
        esp_lcd_touch
        driver
        vfs
        esp_wifi
        esp_netif
        # The management server (`board_web.c`), and the sockets under it.
        esp_http_server
        # The network page's own tools: name resolution (`getaddrinfo`), a reachability check
        # (`esp_ping`), and a fetch the box does itself when a page hands it a URL.
        lwip
        esp_http_client
        # The app's own description (name, version, build date) for `/api/system`.
        esp_app_format
        esp_event
        nvs_flash
        esp_timer
        esp_rom
        # The card is mounted by FATFS (`esp_vfs_fat_*`), and the built-in partition's sizes come
        # from the LittleFS component that owns it.
        fatfs
        joltwallet__littlefs
    # The flash map: how big the chip is (`esp_flash_get_size`) and what it holds
    # (`esp_partition_find`, `esp_partition_get`). Private and not a `REQUIRES`: nothing outside this
    # component names either header, because `board_hal.h` describes the map in plain C types that
    # both the Rust side and the C side can read.
    PRIV_REQUIRES
        spi_flash
        esp_partition
)

# Intercept POSIX settimeofday and clock_settime so they automatically persist to hardware RTC
target_link_options(${COMPONENT_LIB} INTERFACE
    "-Wl,--wrap=settimeofday"
    "-Wl,--wrap=clock_settime"
)
