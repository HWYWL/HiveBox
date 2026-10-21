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
        "board_time.c"
        "board_storage.c"
        "board_system.c"
        "board_http.c"
        # The cloud client (`board_cloud.c`): the second thing that speaks to the
        # internet, POST-shaped and signed, and the reason it is not `board_http.c` is
        # written at the top of it.
        "board_cloud.c"
        # WorkBuddy's account status (`board_workbuddy.c`): the third, POST-shaped with a
        # bearer token instead of a signature. Same shape as the cloud client; the reason
        # it is a file of its own is at the top of it.
        "board_workbuddy.c"
        "board_web.c"
        "board_socket.c"
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
        # The other app slot, and the calls that make one bootable: `esp_ota_get_next_update_partition`,
        # `esp_ota_begin/write/end` and `esp_ota_set_boot_partition` in `board_http.c`, and the one that
        # takes a new image off probation, `esp_ota_mark_app_valid_cancel_rollback`, in `board_hal.c`.
        # Private like the two above — `board_hal.h` describes the second sink in plain C, and no other
        # component names `esp_ota_ops.h`.
        app_update
        # The root bundle HTTP client's TLS is verified against (`esp_crt_bundle_attach`, in
        # `board_http.c`). Private for the same reason as the two above: no other component names it,
        # and the roots themselves are a build setting rather than an API.
        mbedtls
)

# Intercept POSIX settimeofday and clock_settime so they automatically persist to hardware RTC
target_link_options(${COMPONENT_LIB} INTERFACE
    "-Wl,--wrap=settimeofday"
    "-Wl,--wrap=clock_settime"
)

# The `[send]` line in `board_display.c` — the one that splits a flush into the byte swap, the DMA
# wait and whatever is left — is compiled only when the flag that turns on the Rust half's `[phase]`
# and `[flush]` lines is given, so that a single
#
#   idf.py -DPOMELO_RUST_FEATURES=render-debug build flash monitor
#
# turns on the whole measurement and a plain build carries none of it. There is one switch because
# there is one measurement: a `[send]` line without the `[flush]` line it belongs to would be a number
# with nothing to weigh it against, and a firmware that pays for probes nobody collects is the thing
# this avoids.
#
# Read here and not declared here: `POMELO_RUST_FEATURES` is `rust_main`'s cache variable, and a cache
# entry made by `-D` exists before any component is processed, which is the only reason this component
# — configured first, since `rust_main` requires it — can see it at all. Undefined means the flag was
# not given, and that is a build without the probes rather than an error.
#
# Matched as a substring and not with `IN_LIST`: the same string is handed to cargo as one
# `--features` argument, where commas separate features, so `render-debug,bench` would be a single
# list element here.
if(DEFINED POMELO_RUST_FEATURES AND POMELO_RUST_FEATURES MATCHES "(^|,)render-debug($|,)")
    target_compile_definitions(${COMPONENT_LIB} PRIVATE BOARD_DISPLAY_PROFILE=1)
endif()
