//! 服务器，作为一个可安装的包。
//!
//! 两页：**列表**（板子的配置里有哪几台机器）、**详情**（选中的那一台的六个数字）。列表页每行
//! 右侧是「编辑」与「删除」，右上角是「添加」—— 按下去弹出的是一张带键盘的表格，地址、端口、
//! 账号、密码都在这一页上填完，写进的是板子自己的 `nas.conf`。
//!
//! # 键盘为什么画在包里
//!
//! 这一块屏幕只有手指，没有物理键盘，而 ABI 里没有「请给我一个输入框」这一条：能画的只有矩形、
//! 文字与图标。所以键盘就画在包自己的画布上 —— 它和别的控件一样是矩形，按下去一样收到坐标，填
//! 完的那几个字由 [`host::nas_remember`] 递给板子。
//!
//! # 终端是别人的页面
//!
//! 详情页右上角的「终端」不再在这一页里开 shell，而是请外壳把**终端应用**翻上来：
//! [`host::nas_terminal`]。会话、回滚、`^C` 都在那边，这个包只说了「开在哪台机器上」。
//!
//! # 有哪几台机器，是板子说的
//!
//! 「列表里有几行」来自 [`host::nas_devices`] —— 板子读过的那份 `nas.conf`。密码不过边界：这
//! 一页只拿到 `host` 与 `user`，改一台机器时密码那格留空就是「不改」。权限：`NAS`。
#![no_std]

use app_sdk::{
    app, app_abi, format_into, host, icons, theme, widgets, App, Context, Event, Rect, Ui,
    ui::Color,
};

/// 这个包自己的颜色，与 `tools/build_apps.py` 里 nas-monitor 那条目的 `#705CF0` 是同一个。
const ACCENT: Color = 0x0070_5CF0;

/// 列表页右上角的「添加」、详情页右上角的「终端」、以及详情页那颗「重读」。
///
/// 都压在 56 以下：屏幕最上面那 48 像素是外壳从屏幕顶边拉下任务列表的那一条，落在那里的手指
/// 归它，应用一个事件也收不到 —— 摆在 22 上的那颗「添加」按下去没有反应，就是这个原因。
const ADD: Rect = Rect::new(318.0, 56.0, 144.0, 44.0);
const TERMINAL_PILL: Rect = Rect::new(300.0, 52.0, 162.0, 44.0);
const REFRESH: Rect = Rect::new(318.0, 100.0, 144.0, 36.0);
const BACK: Rect = Rect::new(18.0, 52.0, 48.0, 44.0);

/// 一台机器一行，一行 80：两行字，右边两颗按钮。
const ROW: f32 = 80.0;

/// 列表一屏最多几台。第四台之后要翻页，而一台盒子被交代过的机器很少有那么多。
const ROWS: usize = 4;

/// 盘的清单：`device\tmodel\tsize_bytes\ttemperature_c\tbusy_percent\tread_per_sec\t
/// write_per_sec\tpurpose`，一行一块。
const DISK_LINES: usize = 8;

/// 表格里那四格能有多长。地址与密码宽一些，账号与端口短。
const HOST_MAX: usize = 64;
const USER_MAX: usize = 32;
const SECRET_MAX: usize = 64;
const PORT_MAX: usize = 5;

/// 键盘里那一格字能有多长：外壳那边限到 96 个字符，这里的取字缓冲跟它一样大。
const KEYBOARD_MAX: usize = 96;

/// 表格的四格。地址与密码各占一行，端口与账号并排 —— 键盘抬起来以后下面那条是它的，
/// 要填的东西都得让到 `app_abi::KEYBOARD_HEIGHT`（480 高的屏上是 230.4，从 249.6 起）以上。
const FIELD_X: f32 = 104.0;
const FIELD_W: f32 = 352.0;
const FIELD_H: f32 = 42.0;
const FIELD_TOP: f32 = 54.0;
const FIELD_STEP: f32 = 48.0;
/// 端口那格窄一些，账号接在它右边。
const PORT_W: f32 = 116.0;

/// 「保存」与「取消」。也在键盘那一条以上，但不在顶边那 48 像素里（见 [`ADD`]）。
const SAVE: Rect = Rect::new(300.0, 198.0, 156.0, 42.0);
const CANCEL: Rect = Rect::new(164.0, 198.0, 128.0, 42.0);

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    List,
    Detail,
}

/// 表格里的四格，从上到下。
#[derive(Clone, Copy, PartialEq)]
enum Field {
    Host,
    Port,
    User,
    Secret,
}

impl Field {
    const ALL: [Field; 4] = [Field::Host, Field::Port, Field::User, Field::Secret];
}

/// 表格：一条正在填的机器，或者没有表格。
struct Form {
    /// 改的是清单里的第几台；新的一条是 `None`。
    editing: Option<usize>,
    host: [u8; HOST_MAX],
    host_len: usize,
    port: [u8; PORT_MAX],
    port_len: usize,
    user: [u8; USER_MAX],
    user_len: usize,
    secret: [u8; SECRET_MAX],
    secret_len: usize,
    focus: Field,
    error: Option<&'static str>,
    pressed: Option<Rect>,
}

/// 表格上按下来的那一下。
#[derive(Clone, Copy, PartialEq)]
enum Action {
    None,
    Cancel,
    Focus(Field),
    Save,
}

impl Form {
    /// 空表格：新的一条。端口先填 22，那是九成的机器。
    fn blank() -> Self {
        let mut form = Self {
            editing: None,
            host: [0; HOST_MAX],
            host_len: 0,
            port: [0; PORT_MAX],
            port_len: 0,
            user: [0; USER_MAX],
            user_len: 0,
            secret: [0; SECRET_MAX],
            secret_len: 0,
            focus: Field::Host,
            error: None,
            pressed: None,
        };

        form.put(Field::Port, "22");

        form
    }

    /// 改清单里的第 `index` 台：地址与账号先填上，端口与密码留空 —— 空就是「不动它」。密码不在
    /// 这一页上出现，也就不该要求人再输一遍。
    fn edit(index: usize, host: &str, user: &str) -> Self {
        let mut form = Self::blank();

        form.editing = Some(index);
        form.focus = Field::Host;

        form.put(Field::Host, host);
        form.put(Field::Port, "");
        form.put(Field::User, user);

        form
    }

    fn buffer_mut(&mut self, field: Field) -> (&mut [u8], &mut usize) {
        match field {
            Field::Host => (&mut self.host[..HOST_MAX - 1], &mut self.host_len),
            Field::Port => (&mut self.port[..PORT_MAX - 1], &mut self.port_len),
            Field::User => (&mut self.user[..USER_MAX - 1], &mut self.user_len),
            Field::Secret => (&mut self.secret[..SECRET_MAX - 1], &mut self.secret_len),
        }
    }

    fn len(&self, field: Field) -> usize {
        match field {
            Field::Host => self.host_len,
            Field::Port => self.port_len,
            Field::User => self.user_len,
            Field::Secret => self.secret_len,
        }
    }

    fn text(&self, field: Field) -> &str {
        let bytes = match field {
            Field::Host => &self.host[..self.host_len],
            Field::Port => &self.port[..self.port_len],
            Field::User => &self.user[..self.user_len],
            Field::Secret => &self.secret[..self.secret_len],
        };

        core::str::from_utf8(bytes).unwrap_or("")
    }

    /// 往某一格里写一整段字，从头上开始。
    fn put(&mut self, field: Field, text: &str) {
        let count = text.len();
        let (buffer, len) = self.buffer_mut(field);
        let count = count.min(buffer.len());

        buffer[..count].copy_from_slice(&text.as_bytes()[..count]);
        *len = count;
    }

    /// 端口那格只留数字 —— 键盘是外壳的，什么都能敲进来，而一串字母存回去只会变成一个 0。
    fn only_digits(&mut self) {
        let mut kept = 0;

        for index in 0..self.port_len {
            if self.port[index].is_ascii_digit() {
                self.port[kept] = self.port[index];
                kept += 1;
            }
        }

        self.port_len = kept;
    }

    /// 端口：空的是 0，等于「22 或原来那个」，由板子决定。
    fn port(&self) -> u16 {
        let mut value = 0u16;

        for byte in &self.port[..self.port_len] {
            value = value.saturating_mul(10).saturating_add(u16::from(byte - b'0'));
        }

        value
    }

    /// 这一格在框里写什么：密码换成星号，空的时候是灰色的提示。
    fn shown(&self, field: Field) -> &str {
        if self.len(field) > 0 {
            return self.text(field);
        }

        match field {
            Field::Host => "192.168.1.10",
            Field::Port => "22",
            Field::User => "root",
            Field::Secret => "密码",
        }
    }

    /// 密码格里那几个星号。写在一份自己的缓冲上，因为 [`Form::shown`] 借的是表格。
    fn stars<'a>(&self, buffer: &'a mut [u8; SECRET_MAX]) -> &'a str {
        let count = self.secret_len.min(buffer.len());

        for slot in &mut buffer[..count] {
            *slot = b'*';
        }

        core::str::from_utf8(&buffer[..count]).unwrap_or("")
    }
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

    /// 下一回向板子要读数的时刻（`now_ms` 的刻度）。详情页靠它活着：不点什么都五秒一换新。
    next_ask: i64,

    /// 正在填的那一条，或者没有表格。
    form: Option<Form>,

    /// 详情页底部那一句：终端没开成时它说为什么。
    notice: Option<&'static str>,

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
}

impl Monitor {
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

        // 详情页自己活着：到点就要一次新读数。要是一次看失败了 —— 板子刚醒、Wi-Fi 还没就绪、
        // 那台机器睡着了 —— 板子会照它自己的钟点再敲，这一页只要一直要，就一直有得画；不点的
        // 话，六宫格上的数要等下一次手指头才换新。
        //
        // 五秒一拍，与板子认的那台机器的间隔是同一个量级；板子那边把凑太近的请求并成一次看。
        let now = host::now_ms();
        if self.screen == Screen::Detail && now >= self.next_ask {
            host::nas_refresh();
            self.next_ask = now + 5_000;
        }
    }

    /// 把清单里第 `index` 台删掉。删掉的要是正看着的那台，板子会自己换一台看，读数下帧就变。
    fn forget(&mut self, index: usize) {
        let mut storage = [0u8; HOST_MAX];
        let host = self.device_host(index);
        let len = host.len().min(storage.len());
        storage[..len].copy_from_slice(&host.as_bytes()[..len]);

        let host = core::str::from_utf8(&storage[..len]).unwrap_or("");
        host::nas_forget(host);

        self.poll();

        if self.devices == 0 {
            self.chosen = 0;
            self.screen = Screen::List;
        } else if self.chosen >= self.devices {
            self.chosen = self.devices - 1;
        }

        host::nas_refresh();
    }

    /// 表格上按了「保存」：检查、递过去、收起来。
    fn save(&mut self) {
        let mut host = [0u8; HOST_MAX];
        let mut user = [0u8; USER_MAX];
        let mut secret = [0u8; SECRET_MAX];

        let (host_len, user_len, secret_len, port, editing) = {
            let Some(form) = self.form.as_ref() else {
                return;
            };

            let (host_len, user_len, secret_len) = (form.host_len, form.user_len, form.secret_len);

            host[..host_len].copy_from_slice(&form.host[..host_len]);
            user[..user_len].copy_from_slice(&form.user[..user_len]);
            secret[..secret_len].copy_from_slice(&form.secret[..secret_len]);

            (host_len, user_len, secret_len, form.port(), form.editing)
        };

        let host = core::str::from_utf8(&host[..host_len]).unwrap_or("");
        let user = core::str::from_utf8(&user[..user_len]).unwrap_or("");
        let secret = core::str::from_utf8(&secret[..secret_len]).unwrap_or("");

        // 一条登录得有地址与账号；新的一条还得有密码，而编辑时空密码是「不改原来那个」。
        if host.is_empty() {
            return self.fail("地址不能为空");
        }

        if user.is_empty() {
            return self.fail("账号不能为空");
        }

        if editing.is_none() && secret.is_empty() {
            return self.fail("密码不能为空");
        }

        if !host::nas_remember(host, port, user, secret) {
            return self.fail("板子没记住这一台");
        }

        self.form = None;
        host::keyboard_close();
        self.screen = Screen::List;
        self.notice = None;

        self.poll();

        // 新加的那一台就是刚存进去的那一行，把它选中；改的那一台还在原来那个位置。
        if editing.is_none() {
            for index in 0..self.devices {
                if self.device_host(index) == host {
                    self.chosen = index;
                    break;
                }
            }
        }

        host::nas_refresh();
    }

    fn fail(&mut self, reason: &'static str) {
        if let Some(form) = self.form.as_mut() {
            form.error = Some(reason);
        }
    }

    /// 表格上按下来的那一下：先看清是哪一格，再由 [`Monitor::apply`] 动手。
    ///
    /// 分开是因为两个都要 `self`：看清楚按的是哪一格要读表格，改表格要写它。中间隔一个动作值，
    /// 借用在按完就结束了。
    ///
    /// 键盘不在这里按：它是外壳的，那一排键压根不在这一页上，字是每一帧从这个应用的
    /// [`pull_keyboard`](Monitor::pull_keyboard) 里取回来的。
    fn form_update(&mut self, context: &mut Context) {
        self.pull_keyboard();

        let mut action = Action::None;
        let mut down = None;

        for (index, field) in Field::ALL.iter().enumerate() {
            let rect = field_rect(index);

            if context.input.pressing(rect) {
                down = Some(rect);
            }

            if context.tapped(rect) {
                action = Action::Focus(*field);
            }
        }

        for rect in [SAVE, CANCEL] {
            if context.input.pressing(rect) {
                down = Some(rect);
            }
        }

        if context.tapped(SAVE) {
            action = Action::Save;
        }

        if context.tapped(CANCEL) {
            action = Action::Cancel;
        }

        if let Some(form) = self.form.as_mut() {
            form.pressed = down;
        }

        self.apply(action);
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Cancel => {
                self.form = None;
                host::keyboard_close();
            }
            Action::Save => self.save(),
            Action::Focus(field) => {
                if let Some(form) = self.form.as_mut() {
                    form.focus = field;
                    form.error = None;
                }

                self.open_keyboard();
            }
        }
    }

    /// 请外壳把它自己那块键盘抬起来，替眼前这一格收字。
    ///
    /// 键盘是终端与设置里那同一块，键比应用自己画的大一倍 —— 480 宽的屏上自己排五行，
    /// 一个键只有 43 像素宽，手指按不准。
    fn open_keyboard(&self) {
        let Some(form) = self.form.as_ref() else {
            return;
        };

        let field = form.focus;

        let hint = match field {
            Field::Host => "服务器地址",
            Field::Port => "端口",
            Field::User => "账号",
            Field::Secret => "密码",
        };

        host::keyboard_open(field == Field::Secret, hint, form.text(field));
    }

    /// 把外壳那块键盘里此刻的字落进正在填的那一格。
    ///
    /// 每一帧来取一次：键盘上的字只有外壳知道，应用这边只认「这一刻是什么」。「完成」之后
    /// 状态变成 `DONE`，取完这一遍就把键盘收起来 —— 那一格的修改这时才算落定。
    fn pull_keyboard(&mut self) {
        let state = host::keyboard_state();

        if state == app_abi::keyboard::CLOSED {
            return;
        }

        let mut bytes = [0u8; KEYBOARD_MAX];
        let len = host::keyboard_text(&mut bytes).min(bytes.len());
        let text = core::str::from_utf8(&bytes[..len]).unwrap_or("");

        let Some(form) = self.form.as_mut() else {
            host::keyboard_close();
            return;
        };

        let field = form.focus;

        if form.text(field) != text {
            form.put(field, text);

            if field == Field::Port {
                form.only_digits();
            }
        }

        if state == app_abi::keyboard::DONE {
            host::keyboard_close();
        }
    }

    /// 打开表格改第 `index` 台。地址与账号从板子递来的那行原文里取。
    fn begin_edit(&mut self, index: usize) {
        let mut host = [0u8; HOST_MAX];
        let mut user = [0u8; USER_MAX];

        {
            let text = self.device_host(index);
            let len = text.len().min(host.len());
            host[..len].copy_from_slice(&text.as_bytes()[..len]);

            let text = self.device_user(index);
            let len = text.len().min(user.len());
            user[..len].copy_from_slice(&text.as_bytes()[..len]);
        }

        let mut host_len = 0;
        while host_len < host.len() && host[host_len] != 0 {
            host_len += 1;
        }

        let mut user_len = 0;
        while user_len < user.len() && user[user_len] != 0 {
            user_len += 1;
        }

        let host = core::str::from_utf8(&host[..host_len]).unwrap_or("");
        let user = core::str::from_utf8(&user[..user_len]).unwrap_or("");

        self.form = Some(Form::edit(index, host, user));
        self.open_keyboard();
    }
}

impl App for Monitor {
    fn new(_context: &Context) -> Self {
        Self {
            screen: Screen::List,
            chosen: 0,
            devices: 0,
            device_text: [0; 512],
            device_len: 0,
            status: app_abi::nas::UNCONFIGURED,
            fault: 0,
            cpu: 0,
            has_cpu: false,
            temperature: 0,
            memory_used_kb: 0,
            memory_total_kb: 0,
            sent: 0,
            received: 0,
            uptime: 0,
            age: 0,
            disks: 0,
            disk_text: [0; 1024],
            disk_len: 0,
            next_ask: 0,
            form: None,
            notice: None,
            pressed: None,
        }
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.pressed = None;

        // 表格是盖在页面上的一层：它在的时候，下面那一页一个也不该收到手指。
        if self.form.is_some() {
            self.form_update(context);
            return;
        }

        // 表格不在，键盘也不该在：它是替表格那一格收字的。
        if host::keyboard_state() != app_abi::keyboard::CLOSED {
            host::keyboard_close();
        }

        self.notice = None;

        match self.screen {
            Screen::List => {
                if context.input.pressing(ADD) {
                    self.pressed = Some(ADD);
                }

                if context.tapped(ADD) {
                    self.form = Some(Form::blank());
                    self.open_keyboard();
                    return;
                }

                for index in 0..self.devices.min(ROWS) {
                    let row = row_rect(index);
                    let edit = edit_rect(index);
                    let forget = forget_rect(index);

                    if context.input.pressing(edit) {
                        self.pressed = Some(edit);
                    }

                    if context.input.pressing(forget) {
                        self.pressed = Some(forget);
                    }

                    // 两颗按钮压在行里，所以先问按钮：问过就该走了，不能再当成「点了这一行」。
                    if context.tapped(edit) {
                        self.begin_edit(index);
                        return;
                    }

                    if context.tapped(forget) {
                        self.forget(index);
                        return;
                    }

                    if context.input.pressing(row) {
                        self.pressed = Some(row);
                    }

                    if context.tapped(row) {
                        // 板子只盯一台：点哪一行，就要它改看哪一台 —— 不然这一页画的永远是
                        // 板子原来那台的数，几台机器看着一模一样。
                        //
                        // 地址先抄进一份自己的缓冲：`device_host` 借的是这台机器，而下面要改它。
                        let mut storage = [0u8; HOST_MAX];
                        let len = self.device_host(index).len().min(storage.len());
                        storage[..len].copy_from_slice(&self.device_host(index).as_bytes()[..len]);
                        let watched =
                            core::str::from_utf8(&storage[..len]).is_ok_and(host::nas_watch);

                        self.chosen = index;
                        self.screen = Screen::Detail;

                        if watched {
                            host::nas_refresh();
                            // 刚换的机器第一眼只有事实没有速率，速率来自一秒后的第二眼 —— 到点
                            // 这一页自己会再要，不用手指头催。
                            self.next_ask = host::now_ms() + 2_000;
                        } else {
                            self.notice = Some("这台机器已经不在清单里了");
                        }
                    }
                }
            }
            Screen::Detail => {
                if context.input.pressing(BACK) {
                    self.pressed = Some(BACK);
                }

                if context.tapped(BACK) {
                    self.screen = Screen::List;
                    return;
                }

                if context.input.pressing(REFRESH) {
                    self.pressed = Some(REFRESH);
                }

                if context.input.pressing(TERMINAL_PILL) {
                    self.pressed = Some(TERMINAL_PILL);
                }

                if context.tapped(REFRESH) {
                    host::nas_refresh();
                    return;
                }

                if context.tapped(TERMINAL_PILL) {
                    let opened = host::nas_terminal(self.device_host(self.chosen));

                    if !opened {
                        self.notice = Some("这一台还没存进清单，终端开不了");
                    }
                }
            }
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);
        self.poll();

        // 画之前再取一遍键盘里的字：键盘上的键是外壳收的，应用这里没有事件，`update` 那一遍
        // 不一定跑得到 —— 只在 `update` 里取，字就要等下一次点屏幕才出现。
        self.pull_keyboard();

        // 表格在的时候**只画表格**：不清屏是没用的，宿主把文字画在所有图形之上，那一层盖不住
        // 下面那一页的字 —— 两张页面的字会叠在一起。
        if let Some(form) = self.form.as_ref() {
            sheet(ui, form);
            return;
        }

        match self.screen {
            Screen::List => self.list(ui),
            Screen::Detail => self.detail(ui),
        }
    }
}

impl Monitor {
    /// 列表：板子记着的机器，一行一台，右边两颗按钮。
    fn list(&mut self, ui: &mut Ui) {
        let mut storage = [0u8; 48];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let count = self.devices;

        let subtitle = if count == 0 {
            "还没有机器"
        } else {
            format_into!(buffer, "板子记着 {count} 台")
        };

        widgets::page_header(
            ui,
            Rect::new(18.0, 8.0, 292.0, 68.0),
            icons::DVR,
            ACCENT,
            "服务器",
            subtitle,
        );

        widgets::pill(ui, ADD, "添加", icons::ADD, ACCENT, self.pressed == Some(ADD));

        if count == 0 {
            widgets::empty(
                ui,
                Rect::new(18.0, 128.0, 444.0, 172.0),
                icons::DVR,
                "点「添加」填上地址、账号与密码",
                theme::MUTED,
            );
        } else {
            for index in 0..count.min(ROWS) {
                self.row(ui, index);
            }
        }

        widgets::note(
            ui,
            Rect::new(24.0, 424.0, 432.0, 40.0),
            icons::SETTINGS,
            "密码留在板子那一份 nas.conf 里，这一页看不到",
            theme::MUTED,
        );
    }

    fn row(&self, ui: &mut Ui, index: usize) {
        let rect = row_rect(index);
        let chosen = index == self.chosen;
        let pressed = self.pressed == Some(rect);

        widgets::card_outlined(
            ui,
            rect,
            if pressed {
                theme::SURFACE_HIGH
            } else {
                theme::SURFACE
            },
            if chosen { ACCENT } else { theme::DIVIDER },
        );

        if chosen {
            ui.rounded_rect(Rect::new(rect.x, rect.y + 18.0, 3.0, rect.height - 36.0), 1.5, ACCENT);
        }

        ui.circle((rect.x + 22.0, rect.y + 28.0), 3.5, if chosen { ACCENT } else { theme::MUTED });

        // 字只在按钮左边那一截里排：地址长起来的时候，宁可截掉也不要压到「编辑」上。
        ui.clip(Rect::new(rect.x + 36.0, rect.y, rect.width - 152.0, rect.height));
        ui.text(rect.x + 36.0, rect.y + 16.0, self.device_host(index), 18.0, theme::TEXT);
        ui.text(rect.x + 36.0, rect.y + 42.0, self.device_user(index), 13.0, theme::MUTED);
        ui.clip_end();

        let edit = edit_rect(index);
        let forget = forget_rect(index);

        widgets::icon_button(
            ui,
            edit,
            icons::EDIT,
            18.0,
            if self.pressed == Some(edit) {
                theme::DIVIDER
            } else {
                theme::SURFACE_HIGH
            },
            theme::TEXT,
            self.pressed == Some(edit),
        );

        widgets::icon_button(
            ui,
            forget,
            icons::DELETE,
            18.0,
            if self.pressed == Some(forget) {
                theme::DANGER
            } else {
                theme::SURFACE_HIGH
            },
            theme::DANGER,
            self.pressed == Some(forget),
        );
    }

    /// 详情：这一台的六个数字。终端那一颗不在这里开会话，它请外壳翻到终端应用。
    fn detail(&mut self, ui: &mut Ui) {
        widgets::icon_button(
            ui,
            BACK,
            icons::ARROW_BACK,
            20.0,
            theme::SURFACE,
            theme::TEXT,
            self.pressed == Some(BACK),
        );

        ui.text(78.0, 62.0, self.device_host(self.chosen), 20.0, theme::TEXT);

        widgets::pill(
            ui,
            TERMINAL_PILL,
            "终端",
            icons::TERMINAL,
            ACCENT,
            self.pressed == Some(TERMINAL_PILL),
        );

        let online = self.status == app_abi::nas::ONLINE;

        ui.circle((26.0, 108.0), 4.0, if online { theme::OK } else { theme::DANGER });

        let mut storage = [0u8; 64];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let age = self.age;
        let state = self.state();

        let line = if online {
            format_into!(buffer, "{state} · {age} 秒前取的读数")
        } else if self.status == app_abi::nas::UNCONFIGURED {
            "板子还没配置这一台"
        } else {
            self.fault()
        };

        ui.text(42.0, 100.0, line, 16.0, if online { theme::MUTED } else { theme::DANGER });

        widgets::pill(
            ui,
            REFRESH,
            "重读",
            icons::REFRESH,
            theme::SURFACE_HIGH,
            self.pressed == Some(REFRESH),
        );

        for index in 0..6 {
            self.tile(ui, index);
        }

        if let Some(note) = self.notice {
            ui.text(24.0, 438.0, note, 13.0, theme::DANGER);
        } else {
            widgets::note(
                ui,
                Rect::new(24.0, 424.0, 432.0, 40.0),
                icons::TERMINAL,
                "「终端」会把整块屏幕交给终端应用",
                theme::MUTED,
            );
        }
    }

    fn tile(&self, ui: &mut Ui, index: usize) {
        let rect = tile_rect(index);
        widgets::card(ui, rect, theme::SURFACE);

        let mut storage = [0u8; 32];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);

        let (glyph, caption, value) = match index {
            0 => {
                let whole = self.cpu / 100;
                let frac = self.cpu % 100;
                let value = if self.has_cpu {
                    if frac < 10 {
                        format_into!(buffer, "{whole}.0{frac}%")
                    } else {
                        format_into!(buffer, "{whole}.{frac}%")
                    }
                } else {
                    "—"
                };

                (icons::SPEED, "处理器", value)
            }
            1 => {
                let used = self.memory_used_kb as u64 * 10 / 1_048_576;
                let total = self.memory_total_kb as u64 * 10 / 1_048_576;
                let value = format_into!(
                    buffer,
                    "{}.{} / {}.{} GB",
                    used / 10,
                    used % 10,
                    total / 10,
                    total % 10
                );

                (icons::MEMORY, "内存", value)
            }
            2 => {
                let whole = self.temperature / 100;
                let frac = (self.temperature % 100).abs();
                let value = if self.temperature == 0 {
                    "—"
                } else if frac < 10 {
                    format_into!(buffer, "{whole}.0{frac} C")
                } else {
                    format_into!(buffer, "{whole}.{frac} C")
                };

                (icons::THERMOSTAT, "温度", value)
            }
            3 => {
                let down = self.received / 1024;
                let up = self.sent / 1024;
                let value = format_into!(buffer, "{down} / {up} K");

                (icons::NETWORK_CHECK, "收发 KB/s", value)
            }
            4 => {
                let days = self.uptime / 86_400;
                let hours = (self.uptime % 86_400) / 3_600;
                let value = format_into!(buffer, "{days} 天 {hours} 时");

                (icons::SCHEDULE, "运行", value)
            }
            _ => {
                // 单位按这几块盘加起来的大小挑，不按别的：一块 32 GB 的盘写成「32.0 TB」是错的，
                // 而一块 4 TB 的盘写成「4096.0 GB」也好不到哪去 —— 两个都见过，就是没挑对过。
                let total = self.disk_sum(2);

                let value = if self.disks == 0 || total == 0 {
                    "—"
                } else if total >= 1_099_511_627_776 {
                    let tenths = total / 109_951_162_777;
                    format_into!(buffer, "{} 块 · {}.{} TB", self.disks, tenths / 10, tenths % 10)
                } else if total >= 1_073_741_824 {
                    let tenths = total / 107_374_182;
                    format_into!(buffer, "{} 块 · {}.{} GB", self.disks, tenths / 10, tenths % 10)
                } else {
                    let mb = total / 1_048_576;
                    format_into!(buffer, "{} 块 · {} MB", self.disks, mb)
                };

                // 第一块盘的型号比「磁盘」两个字有用：一块盘时它就是全部，几块盘时它至少说明
                // 数据在哪一类介质上。
                let caption = if self.disks > 0 {
                    self.disk_name(0)
                } else {
                    "磁盘"
                };

                (icons::HARD_DRIVE, caption, value)
            }
        };

        ui.icon(rect.x + 14.0, rect.y + 14.0, 18.0, theme::MUTED, glyph);
        ui.text(rect.x + 40.0, rect.y + 14.0, caption, 12.0, theme::MUTED);
        ui.text(rect.x + 14.0, rect.y + 62.0, value, 20.0, theme::TEXT);
    }
}

/// 一张盖住整页的表格：四格，两颗按钮。
///
/// 键盘不画在这里 —— 它是外壳的那一块，画在这幅画的下面。所以这一页只占上面那一条：屏幕
/// 480 高，键盘从 249.6 起，四格与按钮都排在 240 以上。
fn sheet(ui: &mut Ui, form: &Form) {
    let title = if form.editing.is_some() {
        "编辑服务器"
    } else {
        "添加服务器"
    };

    ui.text(24.0, 8.0, title, 22.0, theme::TEXT);

    let hint = form.error.unwrap_or(if form.editing.is_some() {
        "密码留空就是不改它"
    } else {
        "地址、账号与密码都由板子记着"
    });

    let hint_color = if form.error.is_some() {
        theme::DANGER
    } else {
        theme::MUTED
    };

    ui.clip(Rect::new(24.0, 34.0, 432.0, 18.0));
    ui.text(24.0, 34.0, hint, 12.0, hint_color);
    ui.clip_end();

    for (index, field) in Field::ALL.iter().enumerate() {
        let field = *field;
        let rect = field_rect(index);
        let focused = field == form.focus;

        let label = match field {
            Field::Host => "地址",
            Field::Port => "端口",
            Field::User => "账号",
            Field::Secret => "密码",
        };

        ui.text(24.0, rect.y + (rect.height - 17.0) / 2.0, label, 14.0, theme::MUTED);

        widgets::card_outlined(
            ui,
            rect,
            theme::SURFACE,
            if focused { ACCENT } else { theme::DIVIDER },
        );

        let mut stars = [0u8; SECRET_MAX];
        let masked = field == Field::Secret && form.secret_len > 0;

        let text = if masked {
            form.stars(&mut stars)
        } else {
            form.shown(field)
        };

        // 空着的那一格写的是提示（灰的），不是字：光标也就不该停在提示后面。
        let placeholder = !masked && form.len(field) == 0;
        let color = if placeholder { theme::MUTED } else { theme::TEXT };

        ui.clip(Rect::new(rect.x + 2.0, rect.y, rect.width - 4.0, rect.height));

        ui.text(
            rect.x + 12.0,
            rect.y + (rect.height - 17.0) / 2.0,
            text,
            17.0,
            color,
        );

        if focused && !placeholder {
            let caret = rect.x + 14.0 + ui.measure(text, 17.0);
            ui.rect(
                Rect::new(caret, rect.y + 10.0, 2.0, rect.height - 20.0),
                ACCENT,
            );
        }

        ui.clip_end();
    }

    // 保存
    ui.rounded_rect(SAVE, 10.0, ACCENT);

    if form.pressed == Some(SAVE) {
        ui.rounded_stroke(SAVE, 10.0, 2.0, theme::TEXT);
    }

    ui.text_centered(SAVE, "保存", 16.0, theme::TEXT);

    // 取消
    ui.rounded_rect(CANCEL, 10.0, theme::SURFACE_HIGH);

    if form.pressed == Some(CANCEL) {
        ui.rounded_stroke(CANCEL, 10.0, 2.0, theme::DANGER);
    }

    ui.text_centered(CANCEL, "取消", 16.0, theme::DANGER);
}

/// 四格的位置：地址一行；端口与账号并排；密码一行。
fn field_rect(index: usize) -> Rect {
    match index {
        0 => Rect::new(FIELD_X, FIELD_TOP, FIELD_W, FIELD_H),
        1 => Rect::new(FIELD_X, FIELD_TOP + FIELD_STEP, PORT_W, FIELD_H),
        2 => Rect::new(FIELD_X + PORT_W + 12.0, FIELD_TOP + FIELD_STEP, FIELD_W - PORT_W - 12.0, FIELD_H),
        _ => Rect::new(FIELD_X, FIELD_TOP + FIELD_STEP * 2.0, FIELD_W, FIELD_H),
    }
}

fn row_rect(index: usize) -> Rect {
    Rect::new(18.0, 108.0 + index as f32 * ROW, 444.0, 68.0)
}

/// 行右边那颗「编辑」。
fn edit_rect(index: usize) -> Rect {
    let row = row_rect(index);

    Rect::new(row.right() - 100.0, row.y + 14.0, 40.0, 40.0)
}

/// 行右边那颗「删除」，压在「编辑」外面一格。
fn forget_rect(index: usize) -> Rect {
    let row = row_rect(index);

    Rect::new(row.right() - 52.0, row.y + 14.0, 40.0, 40.0)
}

/// 详情页的六宫格：三列两行。
fn tile_rect(index: usize) -> Rect {
    const W: f32 = 140.0;
    const H: f32 = 128.0;
    const GAP: f32 = 12.0;

    let column = index % 3;
    let row = index / 3;

    Rect::new(18.0 + column as f32 * (W + GAP), 140.0 + row as f32 * (H + GAP), W, H)
}

/// 一份「一行一条」的文本里，第 `index` 行的第 `slot` 个字段。
fn field<'a>(text: &'a [u8], index: usize, slot: usize) -> &'a [u8] {
    let mut line = 0usize;
    let mut start = 0usize;

    for (at, byte) in text.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }

        if line == index {
            return nth(&text[start..at], slot);
        }

        line += 1;
        start = at + 1;
    }

    if line == index {
        nth(&text[start..], slot)
    } else {
        &[]
    }
}

fn nth(line: &[u8], slot: usize) -> &[u8] {
    let mut field = 0usize;
    let mut start = 0usize;

    for (at, byte) in line.iter().enumerate() {
        if *byte != b'\t' {
            continue;
        }

        if field == slot {
            return &line[start..at];
        }

        field += 1;
        start = at + 1;
    }

    if field == slot {
        &line[start..]
    } else {
        &[]
    }
}

/// 一份「一行一条」的文本里有几行。
fn lines(text: &[u8]) -> usize {
    let mut count = text.iter().filter(|byte| **byte == b'\n').count();

    if !text.is_empty() && !text.ends_with(b"\n") {
        count += 1;
    }

    count
}

/// 一串十进制数字。空串与半截数字都算它已经读到的那部分。
fn decimal(text: &[u8]) -> u64 {
    let mut value = 0u64;

    for byte in text {
        if !byte.is_ascii_digit() {
            break;
        }

        value = value.saturating_mul(10).saturating_add(u64::from(byte - b'0'));
    }

    value
}

app!(Monitor);

