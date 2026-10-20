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

use app_sdk::{app, host, theme, widgets, App, Context, Event, Rect, Ui};

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
        let switch = Rect::new(18.0, 300.0, 444.0, 64.0);

        self.pressed = context.input.pressing(switch);

        if context.tapped(switch) {
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

        ui.text(24.0, 28.0, "网页管理", 22.0, theme::MUTED);

        let card = Rect::new(18.0, 84.0, 444.0, 180.0);
        widgets::card(ui, card, theme::SURFACE);

        ui.text_centered(
            Rect::new(card.x, card.y + 32.0, card.width, 48.0),
            self.state(),
            40.0,
            if self.running { theme::OK } else { theme::MUTED },
        );

        // 地址，或者没有地址的理由：开着但没有地址是一件事（还没连上 Wi-Fi），关着没有地址是
        // 另一件（开了才有）。这两句话不一样，页面上也就分开写。
        let detail = if !self.url().is_empty() {
            self.url()
        } else if self.running {
            "未连接 Wi-Fi：局域网内暂无地址"
        } else {
            "启动后即可在浏览器中打开"
        };

        ui.text_centered(
            Rect::new(card.x, card.y + 96.0, card.width, 32.0),
            detail,
            20.0,
            theme::TEXT,
        );

        widgets::button(
            ui,
            Rect::new(18.0, 300.0, 444.0, 64.0),
            if self.running { "停止" } else { "启动" },
            26.0,
            if self.running { theme::DANGER } else { theme::ACCENT },
            theme::TEXT,
            self.pressed,
        );

        // 这句话跟着开关，因为它就是这个开关的意思。
        ui.text(
            24.0,
            392.0,
            "同一个网络里的任何人都能读写这台盒子的文件、改它的网络",
            15.0,
            theme::MUTED,
        );
        ui.text(24.0, 418.0, "服务没有密码，不用时请关掉", 15.0, theme::MUTED);
    }
}

app!(WebManager);
