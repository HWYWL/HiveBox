//! `cargo run` — the forecast as a desktop window.
//!
//! ```text
//! cargo run                      # from this directory: iced's real `iced_winit`, and a window
//! ```
//!
//! iced builds the state with [`Weather::new`], then owns the loop. The board is the HAL's desktop
//! simulator, with its HTTP client swapped for one holding a *forecast* — [`FORECAST`] below, which is
//! the shape Open-Meteo really answers in. That swap is the whole point of the window: without it the
//! simulator's client answers `200` with an empty body, the page reads it as something that was not a
//! forecast, and the demo draws the fault line that case deserves rather than a forecast.
//!
//! The simulator only advances when somebody calls `tick`, and a desktop run has no firmware loop to do
//! it. The device's own client is driven by the network and its `tick` does nothing, so this is a
//! desktop-only concern — which is why it lives in `main` and not in the app.
//!
//! The subscription is the app's own ([`Weather::subscription`]) and it is the one thing the app cannot
//! do for itself: a window's clock is its frames, where the board's is the launcher's pump. The app asks
//! for them only while a request is in flight — see [`Weather::waiting`].

use std::sync::Arc;
use std::time::Duration;

use pomelo_hal::sim::SimHttp;
use pomelo_hal::Board;
use weather::Weather;

/// What the service would have sent.
///
/// A real answer, cut down to the fields the page reads, with `current` and `daily` as Open-Meteo
/// spells them: the columns are arrays that line up by index, and the weather is a WMO code. Keeping
/// it a *real* document rather than a convenient one is what makes the demo worth running — the reader
/// and the reading are exercised by exactly what the page will meet on the network.
const FORECAST: &str = r#"{
    "latitude": 39.875,
    "longitude": 116.375,
    "generationtime_ms": 0.107,
    "utc_offset_seconds": 28800,
    "timezone": "Asia/Shanghai",
    "timezone_abbreviation": "GMT+8",
    "elevation": 49.0,
    "current_units": {
        "time": "iso8601",
        "interval": "seconds",
        "temperature_2m": "\u00b0C",
        "relative_humidity_2m": "%",
        "weather_code": "wmo code",
        "wind_speed_10m": "km/h"
    },
    "current": {
        "time": "2026-10-12T09:30",
        "interval": 900,
        "temperature_2m": 18.4,
        "relative_humidity_2m": 46,
        "weather_code": 2,
        "wind_speed_10m": 11.5
    },
    "daily_units": {
        "time": "iso8601",
        "weather_code": "wmo code",
        "temperature_2m_max": "\u00b0C",
        "temperature_2m_min": "\u00b0C"
    },
    "daily": {
        "time": ["2026-10-12", "2026-10-13", "2026-10-14", "2026-10-15", "2026-10-16"],
        "weather_code": [2, 3, 61, 80, 0],
        "temperature_2m_max": [21.3, 19.8, 16.2, 17.5, 22.1],
        "temperature_2m_min": [9.4, 10.1, 8.8, 7.9, 9.0]
    }
}"#;

fn main() -> iced::Result {
    let board = Arc::new(Board::simulated().with_http(Box::new(SimHttp::answering(FORECAST))));

    {
        let board = Arc::clone(&board);

        std::thread::spawn(move || loop {
            board.tick();
            std::thread::sleep(Duration::from_millis(10));
        });
    }

    iced::application(
        move || Weather::new(Arc::clone(&board)),
        Weather::update,
        Weather::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(Weather::theme)
    .subscription(Weather::subscription)
    .run()
}
