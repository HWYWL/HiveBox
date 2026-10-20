//! 服务器，作为一个可安装的包。
//!
//! 旁边那台机器：连上了没有、CPU 多少、温度多少、内存用了多少、网络上下各多少、开了多久、
//! 有几块盘。全部来自宿主的 [`host::nas_snapshot`] 和 [`host::nas_disks`] —— 这个包没有 SSH
//! 客户端，也不会碰一次会话：重活留在固件那一侧，应用只编排界面，这也是沙箱里唯一能有的
//! 分工。
//!
//! # 读数是别人的，不是这个包的
//!
//! 快照带一个 `age_secs`：这份读数是几秒前取的。它写在页面上，因为一台机器的「现在」和一
//! 块面板的「现在」不是同一件事 —— 取一次读数要一次连接，而这一页每帧画一次。画着三分钟
//! 前的 CPU 却不说，是把别人的读数当成自己的。
//!
//! 没配置（还没在设置里填过一台机器）和连不上是两件事，页面也分开说：前者要人去设置里填，
//! 后者可能是那台机器关了。权限：`NAS`。

#![no_std]

use app_sdk::{
    app, app_abi, format_into, host, icons, theme, widgets, App, Context, Event, Rect, Ui,
};

/// 这个包自己的颜色，与 `tools/build_apps.py` 里 nas-monitor 那条目的 `#705CF0` 是同一个。
const ACCENT: app_sdk::ui::Color = 0x0070_5CF0;

/// 页头右上那颗「重新读取」。
const REFRESH: Rect = Rect::new(318.0, 22.0, 144.0, 44.0);

/// 一行的高度，也是这一页排版的单位。
const ROW: f32 = 34.0;

/// 盘的清单：`device\tmodel\tsize_bytes\ttemperature_c\tbusy_percent\tread_per_sec\t
/// write_per_sec\tpurpose`，一行一块。
const DISK_LINES: usize = 8;

struct Monitor {
    status: u32,
    fault: u32,
    cpu: u32,
    has_cpu: bool,
    temperature: i32,
    memory_used_kb: u32,
    memory_total_kb: u32,
    sent: u32,
    received: u32,
    uptime: u32,
    age: u32,
    disks: usize,
    /// 盘那几行，整块留着原文：一帧只画得下三块，而解析整块比解析要画的那几块简单。
    disk_text: [u8; 1024],
    disk_len: usize,
    refresh_pressed: bool,
}

impl Monitor {
    fn state(&self) -> &'static str {
        match self.status {
            app_abi::nas::UNCONFIGURED => "还没配置",
            app_abi::nas::CONNECTING => "正在连接",
            app_abi::nas::ONLINE => "在线",
            _ => "连不上",
        }
    }

    /// 连不上时的原因，跟「还没配置」不一样：后者要人去填，前者可能是那台机器关了。
    fn fault(&self) -> &'static str {
        match self.fault {
            app_abi::fault::NETWORK => "这台盒子没有网络",
            app_abi::fault::UNREACHABLE => "找不到那台机器",
            app_abi::fault::TIMEOUT => "它没回答",
            app_abi::fault::AUTHENTICATION => "用户名或密码不对",
            app_abi::fault::UNSUPPORTED => "这个固件没有 SSH",
            _ => "",
        }
    }

    /// 第 `index` 块盘那一行（`\t` 分开的八个字段里的第 `field` 个）。
    fn disk_field(&self, index: usize, field: usize) -> &[u8] {
        let mut line = &self.disk_text[..self.disk_len];

        for _ in 0..index {
            let Some(end) = line.iter().position(|byte| *byte == b'\n') else {
                return b"";
            };

            line = &line[end + 1..];
        }

        let line = match line.iter().position(|byte| *byte == b'\n') {
            Some(end) => &line[..end],
            None => line,
        };

        let mut rest = line;

        for _ in 0..field {
            let Some(end) = rest.iter().position(|byte| *byte == b'\t') else {
                return b"";
            };

            rest = &rest[end + 1..];
        }

        match rest.iter().position(|byte| *byte == b'\t') {
            Some(end) => &rest[..end],
            None => rest,
        }
    }

    fn disk_name(&self, index: usize) -> &str {
        // 型号（第二个字段）比设备名（第一个）好认：`/dev/sda` 对不上机箱里哪一块，而型号能。
        let model = self.disk_field(index, 1);

        let name = if model.is_empty() {
            self.disk_field(index, 0)
        } else {
            model
        };

        core::str::from_utf8(name).unwrap_or("磁盘")
    }
}

impl App for Monitor {
    fn new(_: &Context) -> Self {
        Self {
            status: app_abi::nas::UNCONFIGURED,
            fault: app_abi::fault::NONE,
            cpu: 0,
            has_cpu: false,
            temperature: i32::MIN,
            memory_used_kb: 0,
            memory_total_kb: 0,
            sent: 0,
            received: 0,
            uptime: 0,
            age: 0,
            disks: 0,
            disk_text: [0; 1024],
            disk_len: 0,
            refresh_pressed: false,
        }
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.refresh_pressed = context.input.pressing(REFRESH);

        if context.tapped(REFRESH) {
            host::nas_refresh();
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        if let Some(snapshot) = host::nas_snapshot() {
            self.status = snapshot.status;
            self.fault = snapshot.fault;
            self.has_cpu = snapshot.has_cpu != 0;
            self.cpu = snapshot.cpu_x100;
            self.temperature = snapshot.temperature_c_x100;
            self.memory_used_kb = snapshot.memory_used_kb;
            self.memory_total_kb = snapshot.memory_total_kb;
            self.sent = snapshot.net_sent_bps;
            self.received = snapshot.net_received_bps;
            self.uptime = snapshot.uptime_secs;
            self.age = snapshot.age_secs;
            self.disks = (snapshot.disks as usize).min(DISK_LINES);
        }

        if self.status == app_abi::nas::ONLINE {
            if let Some(len) = host::nas_disks(&mut self.disk_text) {
                self.disk_len = len.min(self.disk_text.len());
            }
        }

        // 页头：副标题就是这一台的状态 —— 内置版把它放在抬头右边，而抬头本来就该回答「它在
        // 线吗」。
        widgets::page_header(
            ui,
            Rect::new(18.0, 18.0, 288.0, 44.0),
            icons::DNS,
            ACCENT,
            "服务器",
            self.state(),
        );

        widgets::pill(
            ui,
            REFRESH,
            "重新读取",
            icons::REFRESH,
            ACCENT,
            self.refresh_pressed,
        );

        if self.status != app_abi::nas::ONLINE {
            widgets::empty(
                ui,
                Rect::new(90.0, 200.0, 340.0, 90.0),
                icons::DNS,
                if self.status == app_abi::nas::UNCONFIGURED {
                    "在设置里填一台机器"
                } else {
                    self.fault()
                },
                theme::MUTED,
            );

            return;
        }

        // CPU 和内存是两根 meter：一眼看比例，再看数字。
        let mut y = 66.0;

        if self.has_cpu {
            let mut storage = [0u8; 16];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let percent = format_into!(buffer, "{}.{}%", self.cpu / 100, (self.cpu / 10) % 10);

            widgets::key_value(ui, Rect::new(24.0, y, 432.0, 24.0), "CPU", percent, 20.0);
            widgets::progress(
                ui,
                Rect::new(24.0, y + 26.0, 432.0, 10.0),
                self.cpu as f32 / 10_000.0,
                ACCENT,
            );

            y += 48.0;
        }

        if self.memory_total_kb > 0 {
            let used = self.memory_used_kb;
            let total = self.memory_total_kb;

            let mut storage = [0u8; 24];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let memory = format_into!(
                buffer,
                "{} / {} MB",
                used / 1024,
                total / 1024
            );

            widgets::key_value(ui, Rect::new(24.0, y, 432.0, 24.0), "内存", memory, 20.0);
            widgets::progress(
                ui,
                Rect::new(24.0, y + 26.0, 432.0, 10.0),
                used as f32 / total as f32,
                ACCENT,
            );

            y += 48.0;
        }

        // 每行一个自己的缓冲区：格式化借走缓冲区，而上一行的字还在用它。
        let row = |index: usize| Rect::new(18.0, y + index as f32 * ROW, 444.0, 30.0);

        // 温度有可能读不到：一台说不上来自己多少度的机器不是一台坏了的机器，所以这一行宁可
        // 写「读不到」也不写 0 度。
        let mut storage = [0u8; 16];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let temperature = self.temperature;
        let value = if temperature == i32::MIN {
            "读不到"
        } else {
            format_into!(buffer, "{}°C", temperature / 100)
        };

        widgets::row(
            ui,
            row(0),
            icons::THERMOSTAT,
            ACCENT,
            "温度",
            "",
            value,
            false,
            theme::SURFACE,
            None,
        );

        let mut storage = [0u8; 24];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let received = self.received;
        let value = format_into!(buffer, "{received} B/s");

        widgets::row(
            ui,
            row(1),
            icons::FILE_DOWNLOAD,
            ACCENT,
            "网络下行",
            "",
            value,
            false,
            theme::SURFACE,
            None,
        );

        let mut storage = [0u8; 24];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let sent = self.sent;
        let value = format_into!(buffer, "{sent} B/s");

        widgets::row(
            ui,
            row(2),
            icons::ARROW_UPWARD,
            ACCENT,
            "网络上行",
            "",
            value,
            false,
            theme::SURFACE,
            None,
        );

        let mut storage = [0u8; 24];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let hours = self.uptime / 3600;
        let minutes = (self.uptime / 60) % 60;
        let value = format_into!(buffer, "{hours} 小时 {minutes} 分");

        widgets::row(
            ui,
            row(3),
            icons::SCHEDULE,
            ACCENT,
            "开机",
            "",
            value,
            false,
            theme::SURFACE,
            None,
        );

        y += 4.0 * ROW + 8.0;

        // 盘：一块一行，型号加温度和忙碌度。
        widgets::divider(ui, Rect::new(24.0, y, 432.0, 1.0));
        y += 12.0;

        let shown = self.disks.min(3);

        for index in 0..shown {
            let mut storage = [0u8; 64];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);

            let temperature = core::str::from_utf8(self.disk_field(index, 3)).unwrap_or("—");
            let busy = core::str::from_utf8(self.disk_field(index, 4)).unwrap_or("—");
            let name = self.disk_name(index);

            let line = format_into!(buffer, "{name}  {temperature}°  {busy}%");

            ui.text(24.0, y + index as f32 * 26.0, line, 16.0, theme::TEXT);
        }

        if shown == 0 {
            ui.text(24.0, y, "没有磁盘信息", 16.0, theme::MUTED);
        }

        // 读数有多旧：这一页每帧画一次，而一次连接要好几秒，所以「现在」要写明是谁的现在。
        let mut storage = [0u8; 32];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let age = self.age;
        let note = format_into!(buffer, "{age} 秒前取的读数");

        ui.text(24.0, 472.0, note, 13.0, theme::MUTED);

    }
}

app!(Monitor);
