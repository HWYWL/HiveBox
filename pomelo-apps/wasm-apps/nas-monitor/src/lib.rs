//! 服务器，作为一个可安装的包。
//!
//! 三页，跟内置版一样：**列表**（这台盒子的配置里有哪几台机器）、**详情**（选中的那一台的六个
//! 数字）、**终端**（在那一台上开一个 shell）。列表页右上角是「添加」，详情页右上角是「终端」——
//! 它们原来在哪儿，现在还 在哪儿。
//!
//! # 读数是别人的，不是这个包的
//!
//! 快照带一个 `age_secs`：这份读数是几秒前取的。它写在页面上，因为一台机器的「现在」和一块
//! 面板的「现在」不是同一件事 —— 取一次读数要一次连接，而这一页每帧画一次。
//!
//! # 有哪几台机器，是板子说的
//!
//! 快照只有一份（当前盯着的那台），而「列表里有几行」来自 [`host::nas_devices`] —— 板子读过
//! 的那份 `/internal/AppData/NAS/nas.conf`。密码不过边界：这一页只拿到 `host` 与 `user`。
//!
//! 没配置（还有在设置里填过一台机器）和连不上是两件事，页面也分开说：前者要人去设置里填，后者
//! 可能是那台机器关了。权限：`NAS`。
#![no_std]

use app_sdk::{
    app, app_abi, format_into, host, icons, theme, widgets, App, Context, Event, Rect, Ui,
    ui::Color,
};

/// 这个包自己的颜色，与 `tools/build_apps.py` 里 nas-monitor 那条目的 `#705CF0` 是同一个。
const ACCENT: Color = 0x0070_5CF0;

/// 列表页右上角的「添加」、详情页右上角的「终端」、以及详情页那颗「重读」。
const ADD: Rect = Rect::new(318.0, 22.0, 144.0, 44.0);
const TERMINAL_PILL: Rect = Rect::new(300.0, 18.0, 162.0, 44.0);
const REFRESH: Rect = Rect::new(318.0, 70.0, 144.0, 36.0);
const BACK: Rect = Rect::new(18.0, 18.0, 48.0, 44.0);
const HANG_UP: Rect = Rect::new(318.0, 18.0, 144.0, 44.0);

/// 一台机器一行，一行 72：两行字加一个状态点。
const ROW: f32 = 80.0;

/// 列表一屏最多几台。第四台之后要翻页，而一台盒子被交代过的机器很少有那么多。
const ROWS: usize = 4;

/// 盘的清单：`device\tmodel\tsize_bytes\ttemperature_c\tbusy_percent\tread_per_sec\t
/// write_per_sec\tpurpose`，一行一块。
const DISK_LINES: usize = 8;

/// 终端里保留多少字节的输出：一块面板一屏画十几行，而 8 KB 够往回翻好几屏。
const SHELL: usize = 8192;

/// 终端里那四个快捷命令。键盘不在这一页上：一个人在这里要看的是「它还活着吗」，而四条命令
/// 就是那个问题的答案。
const COMMANDS: [&str; 4] = ["ls", "df -h", "uptime", "free -h"];

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    List,
    Detail,
    Terminal,
}

struct Monitor {
    screen: Screen,
    /// 列表里选中的是第几台。
    chosen: usize,
    /// 板子被交代过几台机器，以及它们那几行原文。
    devices: usize,
    device_text: [u8; 512],
    device_len: usize,

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
    disk_text: [u8; 1024],
    disk_len: usize,

    /// 终端那一段：会话说过的话，以及这一页是不是已经开过一次会话。
    shell: [u8; SHELL],
    shell_len: usize,
    opened: bool,

    /// 「添加」按下去之后那一句说明：往哪儿添，比一个什么也不做的按钮诚实。
    hint: bool,

    pressed: Option<Rect>,
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
            _ => "连不上",
        }
    }

    /// 第 `index` 台机器的地址。
    fn device_host(&self, index: usize) -> &str {
        core::str::from_utf8(field(&self.device_text[..self.device_len], index, 0)).unwrap_or("")
    }

    /// 第 `index` 台机器的账号：列表的第二行就是它。
    fn device_user(&self, index: usize) -> &str {
        core::str::from_utf8(field(&self.device_text[..self.device_len], index, 1)).unwrap_or("")
    }

    /// 第 `index` 块盘那一行（`\t` 分开的八个字段里的第 `field` 个）。
    fn disk_field(&self, index: usize, slot: usize) -> &[u8] {
        field(&self.disk_text[..self.disk_len], index, slot)
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

    /// 所有盘的某个字段之和。存储总量与读写速率都是这么来的 —— 六宫格里的一格说的是「这几块
    /// 盘加起来」，而不是第一块。
    fn disk_sum(&self, field: usize) -> u64 {
        let mut total = 0u64;

        for index in 0..self.disks {
            total += decimal(self.disk_field(index, field));
        }

        total
    }

    /// 一帧的开头问一次：板子才是权威，而这个包只把它说的话画出来。
    fn poll(&mut self) {
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

        if let Some(len) = host::nas_devices(&mut self.device_text) {
            self.device_len = len.min(self.device_text.len());
            self.devices = lines(&self.device_text[..self.device_len]);
        }

        if self.status == app_abi::nas::ONLINE {
            if let Some(len) = host::nas_disks(&mut self.disk_text) {
                self.disk_len = len.min(self.disk_text.len());
            }
        }

        if self.screen == Screen::Terminal {
            let mut chunk = [0u8; 256];

            if let Some(len) = host::nas_shell_take(&mut chunk) {
                for byte in &chunk[..len.min(chunk.len())] {
                    // 满了就丢掉最前面一半：终端里的人要的是最近那几屏。
                    if self.shell_len == self.shell.len() {
                        let half = self.shell.len() / 2;

                        self.shell.copy_within(half.., 0);
                        self.shell_len -= half;
                    }

                    self.shell[self.shell_len] = *byte;
                    self.shell_len += 1;
                }
            }
        }
    }

    /// 在选中的那台机器上开一个会话。60×24 是它回来时自愿报的尺寸，而这一页按 60 列画。
    fn open_shell(&mut self) {
        let host_name = self.device_host(self.chosen);
        let mut name = [0u8; 64];
        let len = host_name.len().min(name.len());
        name[..len].copy_from_slice(&host_name.as_bytes()[..len]);

        self.shell_len = 0;
        self.opened = true;

        let host_name = core::str::from_utf8(&name[..len]).unwrap_or("");

        host::nas_shell_open(host_name, 60, 24);
    }

    /// 会话现在什么状态。
    fn shell_state(&self) -> &'static str {
        match host::nas_shell_status() {
            app_abi::nas_shell::OPENING => "正在连接",
            app_abi::nas_shell::READY => "已连接",
            app_abi::nas_shell::LOST => "已断开",
            _ => "没有会话",
        }
    }
}

impl App for Monitor {
    fn new(_: &Context) -> Self {
        Self {
            screen: Screen::List,
            chosen: 0,
            devices: 0,
            device_text: [0; 512],
            device_len: 0,
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
            shell: [0; SHELL],
            shell_len: 0,
            opened: false,
            hint: false,
            pressed: None,
        }
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.pressed = None;

        match self.screen {
            Screen::List => {
                if context.input.pressing(ADD) {
                    self.pressed = Some(ADD);
                }

                if context.tapped(ADD) {
                    self.hint = true;
                }

                let shown = self.devices.min(ROWS);

                for index in 0..shown {
                    let rect = device_rect(index);

                    if context.input.pressing(rect) {
                        self.pressed = Some(rect);
                    }

                    if context.tapped(rect) {
                        self.chosen = index;
                        self.screen = Screen::Detail;
                        host::nas_refresh();
                    }
                }
            }
            Screen::Detail => {
                if context.input.pressing(BACK) {
                    self.pressed = Some(BACK);
                }

                if context.tapped(BACK) {
                    self.screen = Screen::List;
                }

                if context.input.pressing(REFRESH) {
                    self.pressed = Some(REFRESH);
                }

                if context.tapped(REFRESH) {
                    host::nas_refresh();
                }

                if context.input.pressing(TERMINAL_PILL) {
                    self.pressed = Some(TERMINAL_PILL);
                }

                if context.tapped(TERMINAL_PILL) {
                    self.screen = Screen::Terminal;
                    self.open_shell();
                }
            }
            Screen::Terminal => {
                if context.input.pressing(BACK) {
                    self.pressed = Some(BACK);
                }

                if context.tapped(BACK) {
                    self.screen = Screen::Detail;
                }

                if context.input.pressing(HANG_UP) {
                    self.pressed = Some(HANG_UP);
                }

                if context.tapped(HANG_UP) {
                    host::nas_shell_close();
                    self.opened = false;
                    self.screen = Screen::Detail;
                }

                for (index, command) in COMMANDS.iter().enumerate() {
                    let rect = command_rect(index);

                    if context.input.pressing(rect) {
                        self.pressed = Some(rect);
                    }

                    if context.tapped(rect) {
                        // `\r`：那是回车。退格与 `^C` 也能从这里出去，而这个包没有键盘。
                        let mut line = [0u8; 24];
                        let len = command.len().min(line.len() - 1);

                        line[..len].copy_from_slice(&command.as_bytes()[..len]);
                        line[len] = b'\r';

                        host::nas_shell_write(&line[..len + 1]);
                    }
                }
            }
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);
        self.poll();

        match self.screen {
            Screen::List => self.list(ui),
            Screen::Detail => self.detail(ui),
            Screen::Terminal => self.terminal(ui),
        }
    }
}

impl Monitor {
    /// 列表：抬头说有几台，一行一台，右上角「添加」。
    fn list(&mut self, ui: &mut Ui) {
        let mut head = [0u8; 32];
        let mut buffer = app_sdk::text::Buf::new(&mut head);
        let devices = self.devices;
        let subtitle = if devices == 0 {
            "还没有服务器"
        } else {
            format_into!(buffer, "{devices} 台服务器")
        };

        widgets::page_header(
            ui,
            Rect::new(18.0, 18.0, 288.0, 44.0),
            icons::DNS,
            ACCENT,
            "服务器监控",
            subtitle,
        );

        widgets::pill(ui, ADD, "添加", icons::ADD, ACCENT, self.pressed == Some(ADD));

        if self.devices == 0 {
            widgets::empty(
                ui,
                Rect::new(90.0, 180.0, 340.0, 90.0),
                icons::DNS,
                "还没有服务器，点右上角的「添加」",
                theme::MUTED,
            );

            if self.hint {
                widgets::note(
                    ui,
                    Rect::new(24.0, 404.0, 432.0, 20.0),
                    icons::INFO,
                    "在 设置 → 服务器 里添加一台，填地址、账号与密码",
                    theme::MUTED,
                );
            }

            return;
        }

        let shown = self.devices.min(ROWS);

        for index in 0..shown {
            let rect = device_rect(index);
            let host = self.device_host(index);
            let user = self.device_user(index);

            // 状态只有当前盯着的那一台有：别的机器的读数要一次连接，而列表页不该等六次连接。
            let online = index == self.chosen && self.status == app_abi::nas::ONLINE;

            let mut storage = [0u8; 48];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let line = if online {
                format_into!(buffer, "{user} · 在线")
            } else {
                format_into!(buffer, "{user} · 未读取")
            };

            widgets::row(
                ui,
                rect,
                if online { icons::DNS } else { icons::DNS },
                if online { theme::OK } else { ACCENT },
                host,
                line,
                "",
                self.pressed == Some(rect),
                if index == self.chosen {
                    theme::SURFACE_HIGH
                } else {
                    theme::SURFACE
                },
                if index == self.chosen {
                    Some(ACCENT)
                } else {
                    None
                },
            );

            // 状态点在右端：八行里八个点，一眼看出哪几台已经读到过。
            widgets::dot(
                ui,
                (rect.right() - 22.0, rect.y + rect.height / 2.0),
                5.0,
                if online { theme::OK } else { theme::DIVIDER },
            );
        }

        blocks_note(ui, self.hint);
    }

    /// 详情：抬头是这一台的地址，右上角「终端」，下面六格。
    fn detail(&mut self, ui: &mut Ui) {
        self.crumb(ui, self.chosen);

        widgets::pill(
            ui,
            TERMINAL_PILL,
            "终端",
            icons::TERMINAL,
            ACCENT,
            self.pressed == Some(TERMINAL_PILL),
        );

        // 状态行：一个点、一句话、右边一颗「重读」。
        let online = self.status == app_abi::nas::ONLINE;

        widgets::dot(
            ui,
            (26.0, 90.0),
            6.0,
            if online { theme::OK } else { theme::DANGER },
        );

        let mut storage = [0u8; 64];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let age = self.age;
        let state = self.state();

        let line = if online {
            format_into!(buffer, "{state} · {age} 秒前取的读数")
        } else if self.status == app_abi::nas::UNCONFIGURED {
            "在设置里填一台机器"
        } else {
            self.fault()
        };

        ui.text(42.0, 78.0, line, 16.0, if online { theme::MUTED } else { theme::DANGER });

        widgets::pill(
            ui,
            REFRESH,
            "重读",
            icons::REFRESH,
            if online { ACCENT } else { theme::SURFACE_HIGH },
            self.pressed == Some(REFRESH),
        );

        if !online {
            widgets::empty(
                ui,
                Rect::new(110.0, 200.0, 300.0, 90.0),
                icons::DNS,
                if self.status == app_abi::nas::UNCONFIGURED {
                    "还没有配置机器"
                } else {
                    self.fault()
                },
                theme::MUTED,
            );

            return;
        }

        // 六格：CPU 与温度、内存、网络、磁盘读写、存储、磁盘繁忙。内置版就是这六格。
        let (cpu_percent, temperature) = (self.cpu, self.temperature);
        let (used, total) = (self.memory_used_kb, self.memory_total_kb);
        let (received, sent) = (self.received, self.sent);
        let (read, write) = (self.disk_sum(5), self.disk_sum(6));
        let (size, disks) = (self.disk_sum(2), self.disks);

        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            let mut ba = app_sdk::text::Buf::new(&mut a);
            let mut bb = app_sdk::text::Buf::new(&mut b);

            let cpu = format_into!(ba, "{}%", cpu_percent / 100);
            let heat = if temperature == i32::MIN {
                "—"
            } else {
                format_into!(bb, "{}°C", temperature / 100)
            };

            tile(ui, tile_rect(0), icons::DVR, "CPU", cpu, heat, ACCENT);
        }

        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 32];
            let mut ba = app_sdk::text::Buf::new(&mut a);
            let mut bb = app_sdk::text::Buf::new(&mut b);

            let percent = if total == 0 {
                "—"
            } else {
                format_into!(ba, "{}%", used as u64 * 100 / total.max(1) as u64)
            };
            let detail = format_into!(bb, "{} / {} MB", used / 1024, total / 1024);

            tile(ui, tile_rect(1), icons::MEMORY, "内存", percent, detail, ACCENT);
        }

        {
            let mut a = [0u8; 24];
            let mut b = [0u8; 40];
            let mut ba = app_sdk::text::Buf::new(&mut a);
            let mut bb = app_sdk::text::Buf::new(&mut b);

            let down = format_into!(ba, "{} B/s", received);
            let detail = format_into!(bb, "接收 {} · 发送 {}", received, sent);

            tile(ui, tile_rect(2), icons::SPEED, "网络", down, detail, theme::OK);
        }

        {
            let mut a = [0u8; 24];
            let mut b = [0u8; 40];
            let mut ba = app_sdk::text::Buf::new(&mut a);
            let mut bb = app_sdk::text::Buf::new(&mut b);

            let sum = format_into!(ba, "{} B/s", read + write);
            let detail = format_into!(bb, "读 {} · 写 {}", read, write);

            tile(ui, tile_rect(3), icons::HARD_DRIVE, "磁盘读写", sum, detail, theme::OK);
        }

        {
            let mut a = [0u8; 16];
            let mut b = [0u8; 16];
            let mut ba = app_sdk::text::Buf::new(&mut a);
            let mut bb = app_sdk::text::Buf::new(&mut b);

            let bytes = format_into!(ba, "{}", gigabytes(size));
            let detail = format_into!(bb, "{disks} 块硬盘");

            tile(ui, tile_rect(4), icons::STORAGE, "存储", bytes, detail, ACCENT);
        }

        {
            let busy = if disks == 0 {
                "—"
            } else {
                core::str::from_utf8(self.disk_field(0, 4)).unwrap_or("—")
            };

            let mut a = [0u8; 16];
            let mut b = [0u8; 32];
            let mut ba = app_sdk::text::Buf::new(&mut a);
            let mut bb = app_sdk::text::Buf::new(&mut b);

            let percent = format_into!(ba, "{busy}%");
            let name = format_into!(bb, "{}", self.disk_name(0));

            tile(ui, tile_rect(5), icons::BAR_CHART, "磁盘繁忙", percent, name, theme::DANGER);
        }
    }

    /// 终端：最后十几行输出，四个命令，一个「断开」。
    fn terminal(&mut self, ui: &mut Ui) {
        self.crumb(ui, self.chosen);

        widgets::pill(
            ui,
            HANG_UP,
            "断开",
            icons::POWER_OFF,
            theme::SURFACE_HIGH,
            self.pressed == Some(HANG_UP),
        );

        let area = Rect::new(18.0, 74.0, 444.0, 300.0);
        widgets::card(ui, area, theme::SURFACE);

        if self.shell_len == 0 {
            widgets::empty(
                ui,
                Rect::new(area.x + 80.0, area.y + 100.0, 300.0, 90.0),
                icons::TERMINAL,
                if self.opened { "等它回话…" } else { "点「终端」开一个会话" },
                theme::MUTED,
            );
        } else {
            draw_tail(ui, Rect::new(area.x + 12.0, area.y + 10.0, area.width - 24.0, area.height - 20.0), &self.shell[..self.shell_len]);
        }

        // 状态与四个命令：这一页能做的事就这些，说清楚比让人猜好。
        let mut storage = [0u8; 48];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let state = self.shell_state();
        let fault = if host::nas_shell_status() == app_abi::nas_shell::LOST {
            self.fault()
        } else {
            ""
        };
        let line = format_into!(buffer, "{state} {fault}");

        ui.text(24.0, 384.0, line.trim_end(), 15.0, theme::MUTED);

        for (index, command) in COMMANDS.iter().enumerate() {
            let rect = command_rect(index);

            widgets::button(
                ui,
                rect,
                command,
                18.0,
                if host::nas_shell_status() == app_abi::nas_shell::READY {
                    ACCENT
                } else {
                    theme::SURFACE_HIGH
                },
                theme::TEXT,
                self.pressed == Some(rect),
            );
        }
    }

    /// 两个子页共用的抬头：返回箭头、这一台的地址、下面一条细线。
    fn crumb(&mut self, ui: &mut Ui, index: usize) {
        ui.rounded_rect(BACK, 12.0, if self.pressed == Some(BACK) { theme::SURFACE_HIGH } else { theme::SURFACE });
        ui.icon(BACK.x + 14.0, BACK.y + 12.0, 20.0, theme::TEXT, icons::ARROW_BACK);

        let host = self.device_host(index);

        ui.text(78.0, 24.0, host, 22.0, theme::TEXT);
        ui.text(78.0, 50.0, self.device_user(index), 14.0, theme::MUTED);
    }
}

/// 一台机器那一行。
fn device_rect(index: usize) -> Rect {
    Rect::new(18.0, 84.0 + index as f32 * ROW, 444.0, 68.0)
}

/// 六宫格里的第 `index` 格：两列三行。
fn tile_rect(index: usize) -> Rect {
    let column = index % 2;
    let row = index / 2;

    Rect::new(
        18.0 + column as f32 * 230.0,
        116.0 + row as f32 * 104.0,
        214.0,
        96.0,
    )
}

/// 终端里那四个命令按钮。
fn command_rect(index: usize) -> Rect {
    Rect::new(18.0 + index as f32 * 114.0, 410.0, 108.0, 46.0)
}

/// 六宫格里的一格：左边一条竖线（颜色是这一格的意思）、一个小标题、一个大数字、一行小字。
fn tile(ui: &mut Ui, rect: Rect, glyph: char, title: &str, value: &str, note: &str, color: Color) {
    widgets::card(ui, rect, theme::SURFACE);

    // 竖线：六格里六色，扫一眼就知道哪一格在报警。
    ui.rounded_rect(Rect::new(rect.x, rect.y + 12.0, 4.0, rect.height - 24.0), 2.0, color);

    ui.icon(rect.x + 18.0, rect.y + 14.0, 18.0, color, glyph);
    ui.text(rect.x + 44.0, rect.y + 12.0, title, 15.0, theme::MUTED);
    ui.text(rect.x + 18.0, rect.y + 40.0, value, 30.0, color);
    ui.text(rect.x + 18.0, rect.y + 72.0, note, 13.0, theme::MUTED);
}

/// 列表页底部那一句：添加与编辑在哪儿。
fn blocks_note(ui: &mut Ui, hint: bool) {
    widgets::note(
        ui,
        Rect::new(24.0, 404.0, 432.0, 20.0),
        icons::INFO,
        if hint {
            "在 设置 → 服务器 里添加、编辑或删除"
        } else {
            "点一台看它的读数，右上角可以加一台"
        },
        theme::MUTED,
    );
}

/// 输出区的最后几行：终端总是显示最新的那一行，而不是最早的那一行。
fn draw_tail(ui: &mut Ui, area: Rect, text: &[u8]) {
    let rows = (area.height / 20.0) as usize;
    let mut start = 0;
    let mut seen = 0;

    // 从尾部往回数 `rows` 个换行，找到要画的第一行。
    for index in (0..text.len()).rev() {
        if text[index] == b'\n' {
            seen += 1;

            if seen > rows {
                start = index + 1;
                break;
            }
        }
    }

    let mut line = &text[start..];
    let mut y = area.y;

    while !line.is_empty() && y + 18.0 <= area.bottom() {
        let (row, rest) = match line.iter().position(|byte| *byte == b'\n') {
            Some(at) => (&line[..at], &line[at + 1..]),
            None => (line, &line[line.len()..]),
        };

        // 回车在输出里是常见的（进度条、彩色提示），画之前去掉：一块面板上它只会变成一个方块。
        let row: &[u8] = match row.iter().position(|byte| *byte == b'\r') {
            Some(at) => &row[..at],
            None => row,
        };

        ui.text(area.x, y, core::str::from_utf8(row).unwrap_or(""), 14.0, theme::TEXT);

        y += 18.0;
        line = rest;
    }
}

/// 一份「一行一条、字段用 `\t` 分开」的文本里，第 `index` 行的第 `field` 个字段。
fn field<'a>(text: &'a [u8], index: usize, field: usize) -> &'a [u8] {
    let mut line = text;

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

/// 有几行（末尾没有换行也算一行）。
fn lines(text: &[u8]) -> usize {
    if text.is_empty() {
        return 0;
    }

    let mut count = text.iter().filter(|byte| **byte == b'\n').count();

    if text.last() != Some(&b'\n') {
        count += 1;
    }

    count
}

/// 一段十进制数字。读不出来的当 0：一块说不上来自己多大的盘不该让整页显示不出来。
fn decimal(bytes: &[u8]) -> u64 {
    let mut value = 0u64;

    for byte in bytes {
        match byte {
            b'0'..=b'9' => value = value.saturating_mul(10).saturating_add((byte - b'0') as u64),
            _ => break,
        }
    }

    value
}

/// 字节换成 `58G` 这样的一行，带一位小数：存储这一格说的是「大概多大」，不是「多少字节」。
fn gigabytes(bytes: u64) -> Gauge {
    Gauge(bytes)
}

/// `gigabytes` 的返回值：自己实现 `Display`，因为 `format_into!` 要的是一个能印的东西，而这个
/// 包没有浮点格式化（`core::fmt` 里没有 `{:.1}` 之外的浮点支持，手算更快也更小）。
struct Gauge(u64);

impl core::fmt::Display for Gauge {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let bytes = self.0;

        if bytes == 0 {
            return out.write_str("—");
        }

        // TB 与 GB 都只保留一位小数：`58.2G`、`1.8T`。
        let (value, unit) = if bytes >= 1_000_000_000_000 {
            (bytes * 10 / 1_000_000_000_000, "T")
        } else if bytes >= 1_000_000_000 {
            (bytes * 10 / 1_000_000_000, "G")
        } else if bytes >= 1_000_000 {
            (bytes * 10 / 1_000_000, "M")
        } else {
            (bytes / 1_000, "K")
        };

        write!(out, "{}.{}{}", value / 10, value % 10, unit)
    }
}

app!(Monitor);
