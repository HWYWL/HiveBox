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

use app_sdk::{app, app_abi, format_into, host, theme, widgets, App, Context, Event, Rect, Ui};

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
            rescan_pressed: false,
        };

        host::wifi_scan_start();

        map
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        let rescan = Rect::new(18.0, 404.0, 444.0, 56.0);

        self.rescan_pressed = context.input.pressing(rescan);

        if context.tapped(rescan) {
            self.captured = false;
            host::wifi_scan_start();
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

        ui.text(24.0, 28.0, "信道地图", 22.0, theme::MUTED);

        // 十三根柱子。高度是这条信道上的网络数，最多画到四格：第五个网络和第五十个网络在
        // 这张图上要说同一句话 —— 这条信道满了。
        let chart = Rect::new(18.0, 66.0, 444.0, 170.0);
        widgets::card(ui, chart, theme::SURFACE);

        let gap = 6.0;
        let width = (chart.width - 16.0 - gap * (CHANNELS - 1) as f32) / CHANNELS as f32;
        let busiest = self.per_channel.iter().copied().max().unwrap_or(0).max(4) as usize;

        for channel in 0..CHANNELS {
            let count = self.per_channel[channel] as usize;
            let height = (chart.height - 40.0) * (count.min(4) as f32 / 4.0);
            let x = chart.x + 8.0 + channel as f32 * (width + gap);
            let bar = Rect::new(x, chart.bottom() - 24.0 - height, width, height.max(2.0));

            ui.rounded_rect(
                bar,
                3.0,
                if count >= CROWDED {
                    theme::DANGER
                } else if count > 0 {
                    theme::ACCENT
                } else {
                    theme::SURFACE_HIGH
                },
            );

            if count > busiest / 2 || count >= CROWDED {
                let mut storage = [0u8; 4];
                let mut buffer = app_sdk::text::Buf::new(&mut storage);
                let number = format_into!(buffer, "{}", count);

                ui.text_centered(
                    Rect::new(x, bar.y - 20.0, width, 18.0),
                    number,
                    14.0,
                    theme::TEXT,
                );
            }

            let mut storage = [0u8; 4];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let label = format_into!(buffer, "{}", channel + 1);

            ui.text_centered(
                Rect::new(x, chart.bottom() - 22.0, width, 18.0),
                label,
                13.0,
                theme::MUTED,
            );
        }

        // 状态行：扫完了就把最挤的那条信道说出来 —— 这张图的用处就在这一句上。
        let mut storage = [0u8; 48];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);

        let status = match self.scan {
            app_abi::scan::SCANNING => "正在扫描",
            app_abi::scan::ERROR => "扫描失败",
            app_abi::scan::DENIED => "这个包没有 Wi-Fi 权限",
            _ if !self.captured => "还没有扫描结果",
            _ => {
                let mut best = 0usize;
                let mut busiest = 0u8;

                for (channel, count) in self.per_channel.iter().copied().enumerate() {
                    if count > busiest {
                        busiest = count;
                        best = channel + 1;
                    }
                }

                if busiest == 0 {
                    "没扫到网络"
                } else {
                    format_into!(buffer, "共 {} 个网络，第 {} 信道最挤", self.count, best)
                }
            }
        };

        ui.text(24.0, 252.0, status, 18.0, theme::MUTED);

        if self.connected && self.ssid_len > 0 {
            let name = core::str::from_utf8(&self.ssid[..self.ssid_len]).unwrap_or("");
            let rssi = self.rssi;

            let mut storage = [0u8; 64];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let line = format_into!(buffer, "已连接 {name} {rssi} dBm");

            ui.text(24.0, 280.0, line, 18.0, theme::OK);
        }

        // 最强的几个：点不出来（这一版没有键盘），但「谁最响」就是换信道时要知道的全部。
        let list = Rect::new(18.0, 306.0, 444.0, 92.0);
        widgets::card(ui, list, theme::SURFACE);

        let shown = self.count.min(3);

        for index in 0..shown {
            let ap = self.aps[index];
            let row = Rect::new(list.x + 12.0, list.y + 8.0 + index as f32 * 28.0, list.width - 24.0, 26.0);

            let mut storage = [0u8; 64];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let line = format_into!(
                buffer,
                "{channel:>2}  {name}  {rssi} dBm{lock}",
                channel = ap.channel,
                name = ap.name(),
                rssi = ap.rssi,
                // 一个字而不是一把锁的图：面板的字库里有「锁」，没有 emoji。
                lock = if ap.secure != 0 { " 锁" } else { "" }
            );

            ui.text(row.x, row.y, line, 16.0, theme::TEXT);
        }

        if shown == 0 {
            ui.text_centered(list, "还没有结果", 16.0, theme::MUTED);
        }

        widgets::button(
            ui,
            Rect::new(18.0, 404.0, 444.0, 56.0),
            "重新扫描",
            24.0,
            theme::SURFACE_HIGH,
            theme::TEXT,
            self.rescan_pressed,
        );
    }
}

app!(WifiMap);
