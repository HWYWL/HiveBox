idf_component_register(
    SRCS "main.c"
    INCLUDE_DIRS "."
    REQUIRES
        board_hal
        rust_main
        joltwallet__littlefs
        vfs
)

target_link_options(${COMPONENT_LIB} INTERFACE "-Wl,--wrap=write" "-Wl,--wrap=esp_vfs_write" "-Wl,--wrap=_write_r")

# The `internal` partition holds what the board keeps *for its owner* — the Wi-Fi credentials and
# whatever else a person has told it — so it is deliberately **not** `FLASH_IN_PROJECT`.
#
# `FLASH_IN_PROJECT` puts `internal.bin` into `flash_args`, which means every `idf.py flash` writes a
# freshly built, empty filesystem over `0xc10000` and the board forgets its network. That is a data
# loss disguised as an ordinary build step, and it went unnoticed for exactly as long as this line was
# written down as the way to build.
#
# The image is still built, for a factory first flash:
#   idf.py build && python -m esptool ... 0xc10000 build/internal.bin
#
# A board that has never had it is not broken: `fs_init` in `main.c` mounts with
# `format_if_mount_failed`, so the partition is formatted on the first boot that needs it, and
# `welcome.txt` is written by that same function when it is missing.
littlefs_create_partition_image(internal "${CMAKE_SOURCE_DIR}/../../assets/internal")


