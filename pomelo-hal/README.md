# pomelo-hal

The board-level hardware **interface** for [Pomelo OS](https://github.com/pomelos-on-sale/pomelo-os),
plus the desktop simulator that implements it.

Here "HAL" means *board-level services* — the peripherals this board has: power, Wi-Fi, audio, mic,
IMU, storage, and the management server that puts the box on the network. Not the register-level HAL
that [`esp-hal`](https://github.com/esp-rs/esp-hal) means; nothing in this crate talks to a register.

```text
vendor/pomelo-apps/*                the iced applications
        ↑   Arc<Board>, injected by the composition root
pomelo-hal                          traits · types · Board facade · simulator   ← this crate
        ↑   implemented by
pomelo-hal-esp32                    the ESP32-S3 backends (in the firmware repo)
        ↓   extern "C"
firmware/components/board_hal/      the C drivers
```

## What is not here

**No hardware.** This crate declares no `extern "C"` function and contains no `target_os` switch.
Which backends a board has is an argument rather than a `#[cfg]`:

```rust
Board::from_backends(power, wifi, audio, mic, imu, input, storage, web, system)  // the composition root decides
Board::simulated()                                                             // the desktop simulator
```

That is deliberate, and it is the whole point of the split. The implementation lives next to the C
it declares, in the firmware repository ([`firmware/pomelo-hal-esp32`]), where a change to a C
signature and the Rust declaration that mirrors it are one directory apart. Two consequences:

* this crate can be read, built and tested with no hardware and no toolchain but a host one;
* an implementation for another board is a new crate, not a new `cfg` in this one.

The composition root is the only place that names an implementation. On the device that is
`rust_main`, which builds `pomelo_hal_esp32::board()` and hands the `Arc<Board>` to the launcher;
a desktop test builds `Arc::new(Board::simulated())` and hands it to the same launcher.

## The traits

One file per hardware domain, one trait each — `PowerBackend`, `WifiBackend`, `AudioBackend`,
`MicBackend`, `ImuBackend`, `StorageBackend`, `WebBackend`, `SystemBackend`. All of them are
`Send + Sync` (they live behind a mutex inside `Board`) and object-safe (the `Board` fields are
`Box<dyn …>`). Long-running work follows a *start + poll* rule: the caller kicks it off and then
polls a status method that never blocks.

`WebBackend` follows that rule too, and it is the one trait with no data in it: what the server serves
is the C side's page, so all that crosses here is a switch and a port.

`SystemBackend` is the one trait with nothing to bring up. It answers what the chip and the image say
about *themselves* — the part number, the version, the heap, the uptime and the clock — so there is no
`init`, no `tick`, and no device behind it. It is also the source of the one thing a readout cannot
read: `SystemEvent::Tick`, a once-a-second pulse from the firmware's event pump, which is what a page
showing a running time waits for instead of asking for frames.

`Board::from_backends` takes the nine backends in the order `power, wifi, audio, mic, imu, input,
storage, web, system`; boxes rather than generics, so a caller can mix concrete backends and a test can
pass fakes.
`Board::init` and `Board::tick` are the two lifecycle calls: initialise once at boot, tick once per
frame.

`StorageBackend` is the odd one out in two ways. It has no `tick`, because a filesystem has nothing
to advance; and it is asked *when the answer matters* rather than polled, because mounting the
microSD slot is the only way to find out whether a card is in it — the reference board wires no
card-detect pin — and a probe that finds nothing costs the SDMMC driver's timeouts.

## The simulator

`sim/` mirrors `traits/` file for file and is compiled on every platform that is not the device
itself (`cfg(not(target_os = "espidf"))`). It provides deterministic-enough fake data — a battery
that charges and discharges, a Wi-Fi scan that completes over a few `tick`s, WAV playback that
tracks position, a microphone that generates a tone, an IMU that reports gravity plus noise, a
built-in volume and a 32 GB card in the slot — so the whole UI can be developed and unit-tested with
no hardware attached.

The card is the one piece of fake hardware a test moves by hand: `SimStorage::eject` and
`SimStorage::insert` are the slot, and `SimStorage::without_card` builds a board that never had one.

Desktop audio output is real, and optional: the `simulator` feature turns on
[`rodio`](https://github.com/RustAudio/rodio). Without it the audio backend is silent and only
tracks position, so a CI machine with no sound card still runs every test.

## How it is consumed

Inside `pomelo-os` this repository is checked out at `vendor/pomelo-hal`, and the crates that need a
board depend on it by path. From outside, it is an ordinary git dependency:

```toml
pomelo-hal = { git = "ssh://git@github.com/pomelos-on-sale/pomelo-hal.git" }
```

It has **no sibling repositories** — `rodio` is its only dependency and it is optional — so a bare
checkout builds and its tests run:

```text
cargo test
```

That is not true of `iced-pomelo-gfx` or `iced-pomelo-winit`, whose working dependency graph is an
application's.

## What is not implemented yet

The interface is settled; the device side is a skeleton. Each `Esp*` backend's constructor exists so
`board()` can assemble, and every data-producing call returns `HalError::NotSupported` until its
subsystem lands:

| Backend | Phase | C side |
| --- | --- | --- |
| `PowerBackend` | P1 | `hal_power_*` — done, wraps the AXP2101 |
| `AudioBackend` | P2 | `host_audio_*` — done, legacy names (the `hal_*` rename is pending) |
| `WifiBackend` | P3 | `hal_wifi_*` — done |
| `StorageBackend` | P4 | `hal_storage_*` — done, the `internal` partition and the microSD slot |
| `WebBackend` | P4 | `hal_web_*` — done, `esp_http_server` with the page in the image |
| `MicBackend` / `ImuBackend` | P5 | `hal_mic_*` / `hal_imu_*` — to write (ES7210 / QMI8658) |

The display and the touch controller are still deliberately not abstracted: those are the UI engine's
contracts (`pomelo-iced-host`, `iced-pomelo-gfx`), a different concern from the board's services.
The full architecture and the phased plan are in
`issues-and-todo/zh/260926-03-hardware-interfaces-architecture.md` of the `pomelo-os` repository.

## Licence

GPL-3.0-only. See `LICENSE`.
