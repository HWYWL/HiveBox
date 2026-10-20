//! A clock, as a package.
//!
//! Time comes from the host (`hb_now_ms`) and the frame is drawn from it, so there is no timer in
//! here and nothing to drift: an application only ever draws the moment the host says it is. The
//! second pilot, and the one that shows a package can be *live* without owning the clock.

#![no_std]

use app_sdk::{app, format_into, icons, theme, widgets, App, Context, Event, Rect, Ui};

/// 这个包自己的颜色，与 `tools/build_apps.py` 里 clock 那条目的 `#D63384` 是同一个。
const ACCENT: app_sdk::ui::Color = 0x00D6_3384;

struct Clock;

impl Clock {
    /// Milliseconds since midnight, from the host's boot clock.
    fn time_of_day(now_ms: i64) -> (u32, u32, u32, u32) {
        let seconds = (now_ms.max(0) / 1000) as u32;
        let hour = (seconds / 3600) % 24;
        let minute = (seconds / 60) % 60;
        let second = seconds % 60;
        let millis = (now_ms.max(0) % 1000) as u32;

        (hour, minute, second, millis)
    }
}

impl App for Clock {
    fn new(_: &Context) -> Self {
        Self
    }

    fn update(&mut self, _event: Event, _context: &mut Context) {}

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        let now = app_sdk::host::now_ms();
        let (hour, minute, second, millis) = Self::time_of_day(now);

        let mut storage = [0u8; 16];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let time = format_into!(buffer, "{hour:02}:{minute:02}");

        // 页头：这一页的图标与名字，跟别的应用是同一个开头。
        widgets::page_header(
            ui,
            Rect::new(18.0, 24.0, 420.0, 44.0),
            icons::SCHEDULE,
            ACCENT,
            "时钟",
            "时间来自宿主时钟",
        );

        ui.text(36.0, 120.0, time, 92.0, theme::TEXT);

        let mut storage = [0u8; 8];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let seconds = format_into!(buffer, "{second:02}");

        ui.text(356.0, 172.0, seconds, 36.0, theme::MUTED);

        // The second, as a bar: the one thing on this page that moves, and the reason a clock is a
        // live application rather than a picture of one.
        let fraction = (second as f32 + millis as f32 / 1000.0) / 60.0;
        widgets::progress(ui, Rect::new(36.0, 250.0, 408.0, 10.0), fraction, ACCENT);

        ui.text(36.0, 286.0, "开机以来", 18.0, theme::MUTED);

        let uptime = now / 1000;
        let hours = uptime / 3600;
        let minutes = (uptime / 60) % 60;

        let mut storage = [0u8; 24];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let uptime = format_into!(buffer, "{hours} 小时 {minutes} 分");

        ui.text(36.0, 312.0, uptime, 26.0, theme::TEXT);

        ui.text(36.0, 372.0, "开机时长跟着宿主走，断电才归零", 18.0, theme::MUTED);
    }
}

app!(Clock);
