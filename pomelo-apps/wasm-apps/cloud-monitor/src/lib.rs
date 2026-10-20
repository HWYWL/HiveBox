//! 云服务，作为一个可安装的包。
//!
//! 一个账号下面的机器：列出来，选中一台，开机、关机、重启。密钥不在这个包里 —— 设置页填进去
//! 的那把存在固件那一侧，这个包只会问「这家的密钥配好了没有」（[`host::cloud_key`]），然后请
//! 宿主替它发请求。签名是一道需要用密钥做的算术，而密钥不该进沙箱。
//!
//! # 一次一件事
//!
//! 云的这条队跟 HTTP 那条一样：一次一个请求。所以「列一次」和「对某一台下命令」不会同时
//! 在飞：命令发出去之后列表就旧了，而页面把这件事说出来（「命令已接受，再列一次」），不是
//! 假装列表已经更新。
//!
//! 返回的四行（`I` 一台机器、`A` 命令被接受、`R` 被拒、`F` 这一家整个失败了）各有一句话，
//! 被拒的那句连厂家自己的码一起显示在页面上：这一页能做的最有用的事，就是把「为什么不行」
//! 原样带回来。权限：`CLOUD`。

#![no_std]

use app_sdk::{app, app_abi, format_into, host, theme, widgets, App, Context, Event, Rect, Ui};

/// 一屏最多列这么多台：再往下是滚动，而这一版没有滚动。
const MAX_INSTANCES: usize = 8;

/// 三家的名字，顺序跟 [`app_abi::vendor`] 一致。
const VENDORS: [(&str, u32); 3] = [
    ("腾讯云", app_abi::vendor::TENCENT),
    ("阿里云", app_abi::vendor::ALIYUN),
    ("AWS", app_abi::vendor::AWS),
];

/// 一次下命令要传的目标：列表传厂家，命令传 `16 + 厂家`。
const POWER_TARGET: u32 = 16;

#[derive(Clone, Copy, Default)]
struct Instance {
    id: [u8; 32],
    id_len: usize,
    name: [u8; 24],
    name_len: usize,
    ip: [u8; 24],
    ip_len: usize,
    state: u32,
}

impl Instance {
    fn id(&self) -> &str {
        core::str::from_utf8(&self.id[..self.id_len]).unwrap_or("")
    }

    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }

    fn ip(&self) -> &str {
        core::str::from_utf8(&self.ip[..self.ip_len]).unwrap_or("")
    }
}

struct Cloud {
    vendor: u32,
    /// 这一家的密钥状态：`masked\tregion`。
    key: [u8; 64],
    key_len: usize,
    /// 列表要用的区域，从密钥那一行里来。
    region: [u8; 24],
    region_len: usize,
    instances: [Instance; MAX_INSTANCES],
    count: usize,
    selected: Option<usize>,
    /// 最后一句话：接受、拒绝、失败，或者「列出来了几台」。
    note: [u8; 96],
    note_len: usize,
    /// 请求在飞的时候不重复发：这一页只在状态变了的那一帧取一次。
    seen: u32,
    /// 三个厂家按钮、三行命令按钮、列表行，各有一个按下的状态。
    pressed: Option<Rect>,
    list_pressed: Option<usize>,
}

impl Cloud {
    fn region(&self) -> &str {
        core::str::from_utf8(&self.region[..self.region_len]).unwrap_or("")
    }

    fn note(&self) -> &str {
        core::str::from_utf8(&self.note[..self.note_len]).unwrap_or("")
    }

    fn say(&mut self, text: &str) {
        let len = text.len().min(self.note.len());

        self.note[..len].copy_from_slice(&text.as_bytes()[..len]);
        self.note_len = len;
    }

    /// 列一次。
    fn list(&mut self) {
        if host::cloud_start(self.vendor, app_abi::cloud_action::LIST, "", self.region()) {
            self.seen = host::cloud_state();
        } else {
            self.say("这个包没有云服务的权限");
            self.seen = app_abi::state::IDLE;
        }
    }

    /// 对选中的那台下一个命令。
    fn command(&mut self, action: u32) -> bool {
        let Some(index) = self.selected else {
            self.say("先选一台机器");
            return false;
        };

        let instance = self.instances[index];

        if instance.id().is_empty() {
            self.say("这一台没有 ID");
            return false;
        }

        if host::cloud_start(
            POWER_TARGET + self.vendor,
            action,
            instance.id(),
            self.region(),
        ) {
            self.seen = host::cloud_state();
            true
        } else {
            self.say("这个包没有云服务的权限");
            false
        }
    }

    /// 把回来的话读一遍：四行各一句，机器的那几行抄进 `instances`。
    fn settle(&mut self) {
        let mut buffer = [0u8; 2048];

        let Some(len) = host::cloud_take(&mut buffer) else {
            return;
        };

        let text = &buffer[..len.min(buffer.len())];
        self.count = 0;
        self.instances = [Instance::default(); MAX_INSTANCES];

        for line in text.split(|byte| *byte == b'\n') {
            let mut fields = line.split(|byte| *byte == b'\t');

            let Some(kind) = fields.next() else {
                continue;
            };

            match kind.first() {
                Some(&app_abi::cloud::INSTANCE) => {
                    if self.count >= MAX_INSTANCES {
                        continue;
                    }

                    let _provider = fields.next();
                    let state = fields.next().and_then(field_int).unwrap_or(0);
                    let id = fields.next().unwrap_or(&[]);
                    let name = fields.next().unwrap_or(&[]);
                    let ip = fields.next().unwrap_or(&[]);

                    let mut instance = Instance::default();
                    instance.id_len = copy_into(&mut instance.id, id);
                    instance.name_len = copy_into(&mut instance.name, name);
                    instance.ip_len = copy_into(&mut instance.ip, ip);
                    instance.state = state;

                    self.instances[self.count] = instance;
                    self.count += 1;
                }
                Some(&app_abi::cloud::ACCEPTED) => self.say("命令已接受，再列一次"),
                Some(&app_abi::cloud::REFUSED) => {
                    // 厂家自己的话是要带回来的：这一页能做的最有用的事就是这个。
                    let fault = fields.next().and_then(field_int).unwrap_or(0);
                    let message = fields.nth(1).unwrap_or(&[]);

                    let mut storage = [0u8; 96];
                    let mut buffer = app_sdk::text::Buf::new(&mut storage);
                    let text = core::str::from_utf8(message).unwrap_or("");

                    let note = match fault {
                        app_abi::fault::AUTHENTICATION => "密钥不对",
                        app_abi::fault::UNREACHABLE => "连不上",
                        _ => text,
                    };

                    self.say(format_into!(buffer, "被拒绝：{note}"));
                }
                Some(&app_abi::cloud::FAILED_PRODUCT) => self.say("这一家整个失败了"),
                _ => {}
            }
        }

        if self.count > 0 && self.selected.map_or(true, |index| index >= self.count) {
            self.selected = Some(0);
        }
    }
}

fn copy_into(out: &mut [u8], bytes: &[u8]) -> usize {
    let len = bytes.len().min(out.len());

    out[..len].copy_from_slice(&bytes[..len]);
    len
}

fn field_int(bytes: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    let mut any = false;

    for byte in bytes {
        if !byte.is_ascii_digit() {
            break;
        }

        value = value * 10 + (byte - b'0') as u32;
        any = true;
    }

    any.then_some(value)
}

/// 一台机器的状态那句话。
fn state_text(state: u32) -> &'static str {
    match state {
        app_abi::cloud::RUNNING => "运行中",
        app_abi::cloud::STOPPED => "已停止",
        app_abi::cloud::STARTING => "启动中",
        app_abi::cloud::STOPPING => "停止中",
        app_abi::cloud::REBOOTING => "重启中",
        _ => "其他",
    }
}

/// 三个厂家按钮。
fn vendor_rect(index: usize) -> Rect {
    Rect::new(18.0 + index as f32 * 150.0, 62.0, 144.0, 48.0)
}

/// 三条命令。
fn command_rect(index: usize) -> Rect {
    Rect::new(18.0 + index as f32 * 150.0, 404.0, 144.0, 56.0)
}

fn list_row(index: usize) -> Rect {
    Rect::new(18.0, 190.0 + index as f32 * 42.0, 444.0, 38.0)
}

impl App for Cloud {
    fn new(_: &Context) -> Self {
        let mut cloud = Self {
            vendor: app_abi::vendor::TENCENT,
            key: [0; 64],
            key_len: 0,
            region: [0; 24],
            region_len: 0,
            instances: [Instance::default(); MAX_INSTANCES],
            count: 0,
            selected: None,
            note: [0; 96],
            note_len: 0,
            seen: app_abi::state::IDLE,
            pressed: None,
            list_pressed: None,
        };

        // 密钥那一行带区域，而列表要区域：所以先问密钥，再列。
        if let Some(len) = host::cloud_key(cloud.vendor, &mut cloud.key) {
            cloud.key_len = len.min(cloud.key.len());

            let mut fields = cloud.key[..cloud.key_len].split(|byte| *byte == b'\t');
            let _masked = fields.next();
            let region = fields.next().unwrap_or(&[]);

            cloud.region_len = copy_into(&mut cloud.region, region);
        }

        cloud.list();

        cloud
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.pressed = None;
        self.list_pressed = None;

        for index in 0..VENDORS.len() {
            let rect = vendor_rect(index);

            if context.input.pressing(rect) {
                self.pressed = Some(rect);
            }

            if context.tapped(rect) {
                self.vendor = VENDORS[index].1;
                self.count = 0;
                self.instances = [Instance::default(); MAX_INSTANCES];
                self.selected = None;
                self.region_len = 0;

                if let Some(len) = host::cloud_key(self.vendor, &mut self.key) {
                    self.key_len = len.min(self.key.len());

                    let mut fields = self.key[..self.key_len].split(|byte| *byte == b'\t');
                    let _masked = fields.next();
                    let region = fields.next().unwrap_or(&[]);

                    self.region_len = copy_into(&mut self.region, region);
                }

                self.list();
            }
        }

        for index in 0..self.count {
            let rect = list_row(index);

            if context.input.pressing(rect) {
                self.list_pressed = Some(index);
            }

            if context.tapped(rect) {
                self.selected = Some(index);
            }
        }

        let actions = [
            (app_abi::cloud_action::START, "开机"),
            (app_abi::cloud_action::STOP, "关机"),
            (app_abi::cloud_action::REBOOT, "重启"),
        ];

        for index in 0..actions.len() {
            let rect = command_rect(index);

            if context.input.pressing(rect) {
                self.pressed = Some(rect);
            }

            if context.tapped(rect) {
                self.command(actions[index].0);
            }
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        let state = host::cloud_state();

        if state != self.seen {
            self.seen = state;

            match state {
                app_abi::state::DONE => self.settle(),
                app_abi::state::FAILED => self.say("请求失败"),
                app_abi::state::DENIED => self.say("这个包没有云服务的权限"),
                _ => {}
            }
        }

        ui.text(24.0, 28.0, "云服务", 22.0, theme::MUTED);

        for (index, (name, vendor)) in VENDORS.iter().enumerate() {
            let rect = vendor_rect(index);

            widgets::button(
                ui,
                rect,
                name,
                20.0,
                if *vendor == self.vendor {
                    theme::ACCENT
                } else {
                    theme::SURFACE_HIGH
                },
                theme::TEXT,
                self.pressed == Some(rect),
            );
        }

        // 密钥和区域：这一页不存密钥，只说配好了没有。
        let masked = self.key[..self.key_len]
            .split(|byte| *byte == b'\t')
            .next()
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("");

        let region = self.region();

        let mut storage = [0u8; 64];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let line = format_into!(buffer, "密钥 {masked}，区域 {region}");

        ui.text(24.0, 126.0, line, 16.0, theme::MUTED);

        widgets::divider(ui, Rect::new(24.0, 156.0, 432.0, 1.0));

        if self.count == 0 {
            ui.text_centered(
                Rect::new(18.0, 200.0, 444.0, 120.0),
                if state == app_abi::state::RUNNING {
                    "正在问…"
                } else {
                    "这一家没有机器，或者密钥没配好"
                },
                20.0,
                theme::MUTED,
            );
        } else {
            for index in 0..self.count {
                let instance = self.instances[index];
                let rect = list_row(index);
                let chosen = self.selected == Some(index);

                widgets::list_row(
                    ui,
                    rect,
                    instance.name(),
                    "",
                    chosen || self.list_pressed == Some(index),
                );

                let mut storage = [0u8; 64];
                let mut buffer = app_sdk::text::Buf::new(&mut storage);
                let line = format_into!(
                    buffer,
                    "{state}  {ip}",
                    state = state_text(instance.state),
                    ip = instance.ip()
                );

                ui.text(rect.x + 16.0, rect.y + 20.0, line, 14.0, theme::MUTED);
            }
        }

        if !self.note().is_empty() {
            ui.text(24.0, 372.0, self.note(), 16.0, theme::MUTED);
        }

        let actions = [
            (app_abi::cloud_action::START, "开机"),
            (app_abi::cloud_action::STOP, "关机"),
            (app_abi::cloud_action::REBOOT, "重启"),
        ];

        for (index, (_, label)) in actions.iter().enumerate() {
            let rect = command_rect(index);

            widgets::button(
                ui,
                rect,
                label,
                22.0,
                theme::SURFACE_HIGH,
                theme::TEXT,
                self.pressed == Some(rect),
            );
        }
    }
}

app!(Cloud);
