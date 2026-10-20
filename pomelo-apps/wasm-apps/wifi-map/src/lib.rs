//! 信道地图，作为一个可安装的包。
//!
//! 2.4 GHz 一共十三条信道，每条上面挤了多少个网络，画成十三根柱子；柱子下面是从这次扫描里
//! 挑出来的几个最强的网络。扫描是宿主的事（`WIFI` 权限）：这个包只会说「扫一次」，然后每帧
//! 问一次扫完了没有，扫完了把结果抄进来 —— 它自己没有无线电，也不会因为一直问而让谁等。
//!
//! # 为什么柱子是「几个网络」而不是「信号多强」
//!
//! 因为这才是信道地图要回答的问题：同一条信道上的每一个网络都在争同一段时间，跟它的信号
//! 有多强无关。把两个网络的 dBm 加在一起是错的加法，数一数是对的。颜色是另一个判断 ——
//! 一条信道上超过三个网络，换信道已经没有意义了，那根柱子就换成警告色。
//!
//! 扫不出来（没权限、没无线电、或者这次失败了）时页面写清楚是哪一种，而不是画一张空的图：
//! 一张空的信道图和一个没有信道的房子，看上去是一回事。

#![no_std]

use app_sdk::{
    app, app_abi, format_into, host, icons, theme, widgets, App, Context, Event, Rect, Ui,
};

/// 这个包自己的颜色，与 `tools/build_apps.py` 里 wifi-map 那条目的 `#00897B` 是同一个。
const ACCENT: app_sdk::ui::Color = 0x0000_897B;

/// 右上角那颗「重新扫描」，与页面底部那两个翻页的是同一套矩形。
const RESCAN: Rect = Rect::new(318.0, 22.0, 144.0, 44.0);

/// 网络列表一屏四行。
const ROWS: usize = 4;

const PREV: Rect = Rect::new(18.0, 416.0, 140.0, 48.0);
const NEXT: Rect = Rect::new(322.0, 416.0, 140.0, 48.0);

/// 2.4 GHz 的信道。13 是这一带能用的最后一条（14 只在日本，而且只给旧制式）。
const CHANNELS: usize = 13;

/// 一次扫描最多记住这么多个网络：面板上一次能看到的最多也就这么多，而包没有堆。
const MAX_APS: usize = 32;

/// 一条信道上挤到这么多个网络，换信道就没意义了。
const CROWDED: usize = 3;

/// 扫描结果里的一个网络。
#[derive(Clone, Copy, Default)]
struct Ap {
    ssid: [u8; 24],
    ssid_len: usize,
    rssi: i32,
    channel: u32,
    secure: u32,
}

impl Ap {
    fn name(&self) -> &str {
        match core::str::from_utf8(&self.ssid[..self.ssid_len]) {
            Ok(name) if !name.is_empty() => name,
            // 没有名字的网络是常有的事（隐藏 SSID），而一个空行在列表里是看不懂的。
            _ => "(隐藏)",
        }
    }
}

struct WifiMap {
    /// 这次扫描抄进来的网络。
    aps: [Ap; MAX_APS],
    count: usize,
    /// 每条信道上有几个网络，下标 0 是信道 1。
    per_channel: [u8; CHANNELS],
    /// 每条信道承多少压力：不只是自己这条上有几个，还有隔着一两条的那些 —— 一个 20 MHz 的
    /// 网络是占着旁边两条的。
    pressure: [u32; CHANNELS],
    /// 建议换到哪条上去，0 是没有建议（一个网络也没扫到）。
    suggested: usize,
    /// 网络列表翻到第几页：一屏四行，三十二个网络是八屏。
    page: usize,
    list_pressed: Option<usize>,
    /// 手指正按着哪个翻页按钮。
    page_pressed: Option<Rect>,
    /// 抄过了没有：一个扫描只会抄一次，重扫时清零。
    captured: bool,
    /// 这一帧的扫描状态，以及上一帧的：状态从「在扫」变成「扫完」的那一帧才去抄。
    scan: u32,
    connected: bool,
    ssid: [u8; 32],
    ssid_len: usize,
    rssi: i32,
    rescan_pressed: bool,
}

impl WifiMap {
    /// `ssid\trssi\tsecure\tchannel` 一行，抄进 `aps`。
    fn take(&mut self, index: u32) {
        let mut line = [0u8; 96];

        let Some(len) = host::wifi_scan_ap(index, &mut line) else {
            return;
        };

        let line = &line[..len.min(line.len())];
        let mut fields = line.split(|byte| *byte == b'\t');

        let ssid = fields.next().unwrap_or(&[]);
        let rssi = field(fields.next().unwrap_or(&[]));
        let secure = field(fields.next().unwrap_or(&[])) as u32;
        let channel = field(fields.next().unwrap_or(&[])) as u32;

        if self.count >= MAX_APS {
            return;
        }

        let mut ap = Ap {
            ssid: [0; 24],
            ssid_len: ssid.len().min(24),
            rssi,
            channel,
            secure,
        };

        ap.ssid[..ap.ssid_len].copy_from_slice(&ssid[..ap.ssid_len]);
        self.aps[self.count] = ap;
        self.count += 1;

        if channel >= 1 && channel <= CHANNELS as u32 {
            let at = channel as usize - 1;

            if self.per_channel[at] < u8::MAX {
                self.per_channel[at] += 1;
            }
        }
    }

    fn capture(&mut self) {
        self.aps = [Ap::default(); MAX_APS];
        self.count = 0;
        self.per_channel = [0; CHANNELS];

        let found = host::wifi_scan_count().min(MAX_APS as u32);

        for index in 0..found {
            self.take(index);
        }

        // 强的排前面：这张图下面那几行是给「换到哪条信道上」这个问题用的，而回答它只需要看
        // 最响的那几个。冒泡就够了 —— 三十二个，而且只在扫完的那一帧排一次。
        for pass in 0..self.count {
            for index in 1..self.count - pass {
                if self.aps[index].rssi > self.aps[index - 1].rssi {
                    self.aps.swap(index, index - 1);
                }
            }
        }

        self.captured = true;
        self.page = 0;
        self.measure();
    }

    /// 每条信道承多少压力，以及建议换到哪一条上去。
    ///
    /// 一个 20 MHz 的网络是占着旁边那两条的：隔一条还有一大半在压着，隔四条才真的没有。所以
    /// 压力是把每个网络按距离摊到旁边的信道上，而不是数自己这条上有几个 —— 数个数只看得见
    /// 两个网络正好撞在同一条上的那种拥挤。
    fn measure(&mut self) {
        self.pressure = [0; CHANNELS];

        for index in 0..self.count {
            let ap = self.aps[index];

            if ap.channel < 1 || ap.channel > CHANNELS as u32 {
                continue;
            }

            let at = ap.channel as usize - 1;

            for other in 0..CHANNELS {
                let distance = if other > at { other - at } else { at - other };

                self.pressure[other] += overlap(distance);
            }
        }

        // 建议只在 1、6、11 里挑：只有这三条彼此不重叠，而一条「不挤但和谁都撞」的信道是要
        // 不得的。一个网络也没有的时候不给建议。
        let mut suggested = 0usize;
        let mut least = u32::MAX;

        for channel in [1usize, 6, 11] {
            let pressure = self.pressure[channel - 1];

            if pressure < least {
                least = pressure;
                suggested = channel;
            }
        }

        self.suggested = if self.count == 0 { 0 } else { suggested };
    }
}

/// 隔这么远，一个 20 MHz 的网络还压着多少。整数权重：全压是 4，隔四条就是 0。
fn overlap(distance: usize) -> u32 {
    match distance {
        0 => 4,
        1 => 3,
        2 => 2,
        3 => 1,
        _ => 0,
    }
}

/// 一个十进制字段，空字段当 0。
fn field(bytes: &[u8]) -> i32 {
    let text = core::str::from_utf8(bytes).unwrap_or("");
    let mut value: i32 = 0;
    let mut negative = false;

    for (at, byte) in text.bytes().enumerate() {
        match byte {
            b'-' if at == 0 => negative = true,
            b'0'..=b'9' => value = value * 10 + (byte - b'0') as i32,
            _ => break,
        }
    }

    if negative {
        -value
    } else {
        value
    }
}

impl App for WifiMap {
    fn new(_: &Context) -> Self {
        let map = Self {
            aps: [Ap::default(); MAX_APS],
            count: 0,
            per_channel: [0; CHANNELS],
            captured: false,
            scan: app_abi::scan::IDLE,
            connected: false,
            ssid: [0; 32],
            ssid_len: 0,
            rssi: 0,
            pressure: [0; CHANNELS],
            suggested: 0,
            page: 0,
            rescan_pressed: false,
            list_pressed: None,
            page_pressed: None,
        };

        host::wifi_scan_start();

        map
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.rescan_pressed = context.input.pressing(RESCAN);
        self.list_pressed = None;
        self.page_pressed = None;

        if context.tapped(RESCAN) {
            self.captured = false;
            host::wifi_scan_start();
            return;
        }

        // 翻页：一屏四行，三十二个网络是八屏 —— 看不到第二屏的列表是四个网络的列表。
        if context.input.pressing(PREV) {
            self.page_pressed = Some(PREV);
        }

        if context.tapped(PREV) && self.page > 0 {
            self.page -= 1;
        }

        if context.input.pressing(NEXT) {
            self.page_pressed = Some(NEXT);
        }

        if context.tapped(NEXT) && (self.page + 1) * ROWS < self.count {
            self.page += 1;
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        self.scan = host::wifi_scan_state();

        if self.scan == app_abi::scan::DONE && !self.captured {
            self.capture();
        }

        if let Some(status) = host::wifi_status() {
            self.connected = status.state == app_abi::wifi::CONNECTED;
            self.rssi = status.rssi;

            let len = (status.ssid_len as usize).min(self.ssid.len());
            self.ssid[..len].copy_from_slice(&status.ssid[..len]);
            self.ssid_len = len;
        }

        // 页头：图标、名字，底下是这次扫描的进展。
        let mut header = [0u8; 32];
        let mut buffer = app_sdk::text::Buf::new(&mut header);

        let subtitle = match self.scan {
            app_abi::scan::SCANNING => "正在扫描",
            app_abi::scan::ERROR => "扫描失败",
            app_abi::scan::DENIED => "这个包没有 Wi-Fi 权限",
            _ if !self.captured => "还没有扫描结果",
            _ => {
                let count = self.count;
                format_into!(buffer, "扫到 {count} 个网络")
            }
        };

        widgets::page_header(
            ui,
            Rect::new(18.0, 18.0, 288.0, 44.0),
            icons::WIFI_FIND,
            ACCENT,
            "信道地图",
            subtitle,
        );

        widgets::pill(
            ui,
            RESCAN,
            "重新扫描",
            icons::REFRESH,
            if self.scan == app_abi::scan::SCANNING {
                theme::SURFACE_HIGH
            } else {
                ACCENT
            },
            self.rescan_pressed,
        );

        // 建议信道：这一页存在的理由不是画一张图，而是说出「换到哪一条上去」。
        let advice = Rect::new(18.0, 76.0, 444.0, 60.0);
        widgets::card(ui, advice, theme::SURFACE);

        if self.suggested > 0 {
            let mut storage = [0u8; 24];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let channel = self.suggested;
            let suggestion = format_into!(buffer, "建议换到第 {channel} 信道");

            ui.icon(advice.x + 18.0, advice.y + 18.0, 24.0, ACCENT, icons::ROUTER);
            ui.text(advice.x + 56.0, advice.y + 12.0, suggestion, 24.0, theme::TEXT);
            ui.text_in(
                Rect::new(advice.x + 16.0, advice.y + 40.0, advice.width - 32.0, 18.0),
                "1、6、11 互不重叠，只在这三条里挑",
                13.0,
                theme::MUTED,
                app_abi::Align::Right,
            );
        } else {
            ui.text(
                advice.x + 18.0,
                advice.y + 22.0,
                "扫到网络之后，这里给出建议的信道",
                18.0,
                theme::MUTED,
            );
        }

        // 十三根柱子。高度是这条信道承的压力，而不是它上面有几个网络：一个 20 MHz 的网络是
        // 占着旁边两条的，只数自己这条上的，看不见那种拥挤。
        let chart = Rect::new(18.0, 144.0, 444.0, 152.0);
        widgets::card(ui, chart, theme::SURFACE);

        widgets::section(
            ui,
            Rect::new(chart.x + 16.0, chart.y + 12.0, chart.width - 32.0, 22.0),
            icons::BAR_CHART,
            ACCENT,
            "信道压力",
            "20 MHz 重叠",
        );

        let gap = 6.0;
        let width = (chart.width - 16.0 - gap * (CHANNELS - 1) as f32) / CHANNELS as f32;
        let peak = self.pressure.iter().copied().max().unwrap_or(0).max(4) as f32;
        let floor = chart.bottom() - 22.0;

        for channel in 0..CHANNELS {
            let count = self.per_channel[channel] as usize;
            let load = self.pressure[channel] as f32;
            let height = (floor - (chart.y + 42.0)) * (load / peak);
            let x = chart.x + 8.0 + channel as f32 * (width + gap);
            let bar = Rect::new(x, floor - height, width, height.max(2.0));

            // 建议的那条画成绿的：这一页给的答案，应该在图上看得见。
            let color = if self.suggested == channel + 1 {
                theme::OK
            } else if count >= CROWDED {
                theme::DANGER
            } else if count > 0 {
                ACCENT
            } else {
                theme::SURFACE_HIGH
            };

            ui.rounded_rect(bar, 3.0, color);

            if count > 0 {
                let mut storage = [0u8; 4];
                let mut buffer = app_sdk::text::Buf::new(&mut storage);
                let number = format_into!(buffer, "{}", count);

                ui.text_centered(
                    Rect::new(x, bar.y - 17.0, width, 16.0),
                    number,
                    12.0,
                    theme::TEXT,
                );
            }

            let mut storage = [0u8; 4];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let label = format_into!(buffer, "{}", channel + 1);

            ui.text_centered(Rect::new(x, floor, width, 18.0), label, 12.0, theme::MUTED);
        }

        // 网络列表：一屏四行，翻页在下面。强的排前面 —— 换信道要知道的就是谁最响。
        let list = Rect::new(18.0, 304.0, 444.0, 104.0);
        widgets::card(ui, list, theme::SURFACE);

        let start = self.page * ROWS;
        let shown = (self.count - start).min(ROWS);

        if shown == 0 {
            widgets::empty(
                ui,
                Rect::new(list.x + 100.0, list.y + 28.0, 300.0, 70.0),
                icons::WIFI,
                "还没有扫到网络",
                theme::MUTED,
            );
        }

        for index in 0..shown {
            let ap = self.aps[start + index];
            let rect = Rect::new(
                list.x + 10.0,
                list.y + 8.0 + index as f32 * 24.0,
                list.width - 20.0,
                22.0,
            );

            let mut storage = [0u8; 40];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let rssi = ap.rssi;
            let channel = ap.channel;

            let value = if ap.secure != 0 {
                format_into!(buffer, "{rssi} dBm · 信道 {channel} · 锁")
            } else {
                format_into!(buffer, "{rssi} dBm · 信道 {channel}")
            };

            // 信号强弱是这一行的图标：一个「锁」是加密，而换信道要看的是谁最响。
            let color = if ap.rssi >= -55 {
                theme::OK
            } else if ap.rssi >= -75 {
                ACCENT
            } else {
                theme::MUTED
            };

            widgets::row(
                ui,
                rect,
                icons::SIGNAL_CELLULAR_ALT,
                color,
                ap.name(),
                "",
                value,
                self.list_pressed == Some(index),
                theme::SURFACE_HIGH,
                None,
            );
        }

        widgets::button(
            ui,
            PREV,
            "上一页",
            18.0,
            if self.page > 0 { ACCENT } else { theme::SURFACE_HIGH },
            theme::TEXT,
            self.page_pressed == Some(PREV),
        );
        widgets::button(
            ui,
            NEXT,
            "下一页",
            18.0,
            if (self.page + 1) * ROWS < self.count {
                ACCENT
            } else {
                theme::SURFACE_HIGH
            },
            theme::TEXT,
            self.page_pressed == Some(NEXT),
        );

        if self.count > 0 {
            let mut storage = [0u8; 24];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let page = self.page + 1;
            let pages = (self.count + ROWS - 1) / ROWS;
            let note = format_into!(buffer, "{page} / {pages}");

            ui.text_centered(Rect::new(158.0, 416.0, 164.0, 48.0), note, 18.0, theme::MUTED);
        }
    }
}

app!(WifiMap);
