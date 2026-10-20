//! 系统读数，作为一个可安装的包。
//!
//! 内置那一版是 `htop`：一张任务表，每行一个调度器上的任务，列是状态、占用率、优先级、
//! 空闲栈和核心。哪些列能搬过来由 ABI 说了算 —— 宿主给应用的是 [`host::system`] 那样一份
//! 读数（电量、内存、外部内存、存储、开机时长、网络）和固件版本，**没有**逐个任务的列表，
//! 那件事还不在 ABI 里。所以这一页是同一件事的另一半：这一块板子现在有多少内存、多少存储、
//! 跑了多久，画成 meter 加数字。
//!
//!  Meter 是 `htop` 的形状，沿用内置版：先看比例，再看数字。三根 meter 分别是内部堆、外部
//! 内存（PSRAM，面板用的那份）和第一个挂载的卷；读不到的那根画一根空的并写「读不到」，
//! 因为一台说不上来的机器不是一台坏了的机器。
//!
//! 全部是宿主的读数（`SYSTEM` 权限），一帧取一次。

#![no_std]

use app_sdk::{app, app_abi, format_into, host, theme, widgets, App, Context, Event, Rect, Ui};

/// 一根 meter：它的名字、用了多少、总共多少。
struct Meter<'a> {
    label: &'a str,
    used: u32,
    total: u32,
}

impl Meter<'_> {
    /// 画一根：名字、比例条、右边的数字。总数为 0 的是「这一项读不到」。
    fn draw(&self, ui: &mut Ui, rect: Rect) {
        let mut storage = [0u8; 32];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);

        if self.total == 0 {
            let value = format_into!(buffer, "{}", "读不到");
            widgets::key_value(ui, rect, self.label, value, 18.0);
            return;
        }

        let fraction = self.used as f32 / self.total as f32;

        widgets::key_value(
            ui,
            Rect::new(rect.x, rect.y, rect.width, 20.0),
            self.label,
            "",
            18.0,
        );

        widgets::progress(
            ui,
            Rect::new(rect.x, rect.y + 24.0, rect.width, 10.0),
            fraction,
            if fraction >= 0.9 { theme::DANGER } else { theme::ACCENT },
        );

        buffer.reset();

        // 兆字节而不是字节：这一页是扫一眼的地方，而 "3.4 / 3.9 MB" 比
        // "3563520 / 4063232" 更像一眼能拿走的东西。
        let value = format_into!(
            buffer,
            "{}.{} / {}.{} MB",
            self.used / (1024 * 1024),
            (self.used / 1024 / 102) % 10,
            self.total / (1024 * 1024),
            (self.total / 1024 / 102) % 10
        );

        ui.text_in(
            Rect::new(rect.x, rect.y + 40.0, rect.width, 20.0),
            value,
            16.0,
            theme::MUTED,
            app_abi::Align::Right,
        );
    }
}

struct TaskViewer {
    heap_free: u32,
    heap_total: u32,
    psram_free: u32,
    psram_total: u32,
    storage_free: u32,
    storage_total: u32,
    uptime: u32,
    battery: u32,
    wifi_state: u32,
    wifi_bars: u32,
    /// 固件版本，从宿主拷在自己的缓冲区里。
    version: [u8; 32],
    version_len: usize,
}

impl TaskViewer {
    fn version(&self) -> &str {
        core::str::from_utf8(&self.version[..self.version_len]).unwrap_or("—")
    }

    fn meters(&self) -> [Meter<'_>; 3] {
        [
            Meter {
                label: "内存",
                used: self.heap_total.saturating_sub(self.heap_free),
                total: self.heap_total,
            },
            Meter {
                label: "外部内存",
                used: self.psram_total.saturating_sub(self.psram_free),
                total: self.psram_total,
            },
            Meter {
                label: "存储",
                used: self.storage_total.saturating_sub(self.storage_free),
                total: self.storage_total,
            },
        ]
    }

    /// 网络那一行的字：状态比格数重要，格数是给已经连上的人看的。
    fn network(&self) -> &'static str {
        match self.wifi_state {
            app_abi::wifi::CONNECTED => "已连接",
            app_abi::wifi::CONNECTING => "正在连接",
            app_abi::wifi::SCANNING => "正在扫描",
            _ => "未连接",
        }
    }
}

impl App for TaskViewer {
    fn new(_: &Context) -> Self {
        Self {
            heap_free: 0,
            heap_total: 0,
            psram_free: 0,
            psram_total: 0,
            storage_free: 0,
            storage_total: 0,
            uptime: 0,
            battery: 0,
            wifi_state: 0,
            wifi_bars: 0,
            version: [0; 32],
            version_len: 0,
        }
    }

    fn update(&mut self, _event: Event, _context: &mut Context) {}

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        if let Some(readings) = host::system() {
            self.heap_free = readings.heap_free;
            self.heap_total = readings.heap_total;
            self.psram_free = readings.psram_free;
            self.psram_total = readings.psram_total;
            self.storage_free = readings.storage_free;
            self.storage_total = readings.storage_total;
            self.uptime = readings.uptime_secs;
            self.battery = readings.battery_percent;
            self.wifi_state = readings.wifi_state;
            self.wifi_bars = readings.wifi_bars;
        }

        if self.version_len == 0 {
            if let Some(len) = host::firmware_version(&mut self.version) {
                self.version_len = len.min(self.version.len());
            }
        }

        ui.text(24.0, 28.0, "任务详情", 22.0, theme::MUTED);

        let mut y = 66.0;

        for meter in self.meters() {
            meter.draw(ui, Rect::new(24.0, y, 432.0, 60.0));
            y += 76.0;
        }

        widgets::divider(ui, Rect::new(24.0, y - 4.0, 432.0, 1.0));

        // 每个数字一个自己的缓冲区：格式化借走缓冲区，而上一行的字还在用它 —— 借不出第二次。
        let row = |index: usize| Rect::new(24.0, y + 12.0 + index as f32 * 34.0, 432.0, 24.0);

        widgets::key_value(ui, row(0), "固件", self.version(), 20.0);
        widgets::key_value(ui, row(1), "网络", self.network(), 20.0);

        let mut storage = [0u8; 16];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let battery = format_into!(buffer, "{}%", self.battery);

        widgets::key_value(ui, row(2), "电量", battery, 20.0);

        let mut storage = [0u8; 24];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let hours = self.uptime / 3600;
        let minutes = (self.uptime / 60) % 60;
        let uptime = format_into!(buffer, "{hours} 小时 {minutes} 分");

        widgets::key_value(ui, row(3), "开机", uptime, 20.0);

        // 最后一行说明这一页能看到什么、看不到什么：ABI 还没把任务表交给应用，与其让人在
        // 这里找一张不存在的表，不如把话说在页面上。
        ui.text(
            24.0,
            468.0,
            "逐任务的列表还在固件那一侧，未进入 ABI",
            14.0,
            theme::MUTED,
        );
    }
}

app!(TaskViewer);
