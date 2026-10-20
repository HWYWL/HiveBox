//! 网页管理，作为一个可安装的包。
//!
//! 一个开关和一个地址。开关是板子上那个 HTTP 服务的开关（宿主的 `hb_web_set`），地址是同一
//! 个网络里一台浏览器该打开的地方 —— 而地址是宿主算的，不是这个包拼的：一台不在网络上的盒子
//! 没有地址可给，一个没开的服务什么也不提供，这两件事只有固件那一侧知道。
//!
//! # 没有密码，而且这句话要写在页面上
//!
//! 同一个网络里的任何人都能读写这台盒子的文件、改它的网络。那是关于这台盒子的一个决定，不是
//! 这个页面漏掉的一项 —— 所以页面把这句话写在开关下面，而不是藏进一句免责声明里。内置版是
//! 这么做的，这里也就这么做。
//!
//! 权限：`WEB`。这是 ABI 里最宽的一个权限位，商店在安装前会把它列出来。

#![no_std]

use app_sdk::{app, host, icons, theme, widgets, App, Context, Event, Rect, Ui};

/// 这个包自己的颜色，与 `tools/build_apps.py` 里 web-manager 那条目的 `#2384F5` 是同一个。
const ACCENT: app_sdk::ui::Color = 0x0023_84F5;

/// 那颗开关：页面上一个矩形，按下与抬起的判断都用它。
const SWITCH: Rect = Rect::new(18.0, 316.0, 444.0, 68.0);

/// 开关按下去之后不要立刻相信它：板子才是权威，所以每帧都读一次状态。
struct WebManager {
    running: bool,
    /// 地址，拷在自己的缓冲区里，空着就是「没有地址可给」。
    url: [u8; 64],
    url_len: usize,
    /// 最后一次调用的结果，读不到就是「这一台没有这个服务」。
    code: i32,
    pressed: bool,
}

impl WebManager {
    fn url(&self) -> &str {
        core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("")
    }

    /// 状态那句话：开着、关着，或者这一台根本没有这个服务。
    fn state(&self) -> &'static str {
        if self.code < 0 {
            return "这一台没有网页服务";
        }

        if self.running {
            "运行中"
        } else {
            "已停止"
        }
    }
}

impl App for WebManager {
    fn new(_: &Context) -> Self {
        Self {
            running: false,
            url: [0; 64],
            url_len: 0,
            code: 0,
            pressed: false,
        }
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.pressed = context.input.pressing(SWITCH);

        if context.tapped(SWITCH) {
            host::web_set(!self.running);
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        // 每帧问一次，而不是相信上一次的调用：一次 `start` 可能半途而废，而一个说自己在运行
        // 的页面如果不是，比一个什么都不说的页面更糟。
        let mut line = [0u8; 96];

        match host::web_status(&mut line) {
            Some(len) => {
                self.code = 0;

                let len = len.min(line.len());
                let mut fields = line[..len].split(|byte| *byte == b'\t');

                self.running = fields.next() == Some(&b"1"[..]);

                let url = fields.next().unwrap_or(&[]);
                self.url_len = url.len().min(self.url.len());
                self.url[..self.url_len].copy_from_slice(&url[..self.url_len]);
            }
            None => self.code = -1,
        }

        // 标题居中、用这个包自己的颜色，下面是它这一页是关于什么的：内置版是这么开的头。
        ui.text_centered(Rect::new(0.0, 26.0, 480.0, 40.0), "网页管理", 28.0, ACCENT);
        ui.text_centered(
            Rect::new(0.0, 70.0, 480.0, 24.0),
            "局域网文件与网络管理",
            16.0,
            theme::MUTED,
        );

        let card = Rect::new(18.0, 116.0, 444.0, 176.0);
        widgets::card(ui, card, theme::SURFACE);

        // 状态：一个勾或者一个圈，然后是「运行中」那句话 —— 图标是这一行最先被看到的东西。
        let state_color = if self.running { theme::OK } else { theme::MUTED };

        ui.icon(
            card.x + 28.0,
            card.y + 36.0,
            34.0,
            state_color,
            if self.running {
                icons::CHECK_CIRCLE
            } else {
                icons::PUBLIC
            },
        );
        ui.text(card.x + 76.0, card.y + 40.0, self.state(), 30.0, state_color);

        // 地址，或者没有地址的理由：开着但没有地址是一件事（还没连上 Wi-Fi），关着没有地址是
        // 另一件（开了才有）。这两句话不一样，页面上也就分开写。
        let detail = if !self.url().is_empty() {
            self.url()
        } else if self.running {
            "未连接 Wi-Fi：局域网内暂无地址"
        } else {
            "启动后即可在浏览器中打开"
        };

        ui.text(card.x + 28.0, card.y + 100.0, detail, 18.0, theme::TEXT);
        ui.text(
            card.x + 28.0,
            card.y + 132.0,
            "在浏览器里打开它，就能管理卡上的文件与网络",
            14.0,
            theme::MUTED,
        );

        widgets::button(
            ui,
            SWITCH,
            if self.running { "停止服务器" } else { "启动服务器" },
            26.0,
            if self.running { theme::DANGER } else { ACCENT },
            theme::TEXT,
            self.pressed,
        );

        // 这句话跟着开关，因为它就是这个开关的意思。
        widgets::note(
            ui,
            Rect::new(24.0, 404.0, 432.0, 20.0),
            icons::INFO,
            "服务没有密码，请仅在可信网络中开启",
            theme::MUTED,
        );
        widgets::note(
            ui,
            Rect::new(24.0, 428.0, 432.0, 20.0),
            icons::WARNING,
            "同一网络里的任何人都能读写这台盒子的文件",
            theme::MUTED,
        );
    }
}

app!(WebManager);
