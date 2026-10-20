//! 文件管理，作为一个可安装的包。
//!
//! 两个卷（内部存储和 SD 卡）里的目录：进去、退出来、看大小和类型、删掉一个。列目录是宿主的
//! 事（`FILES` 权限）：这个包只会说「打开这个路径」「下一条」「关掉」，而路径由宿主检查 ——
//! 带 `..` 的、不在两个挂载点下面的，都被拒绝在门外面，而不是被悄悄改成一个能看的路径。
//!
//! # 为什么一屏只有七行
//!
//! 因为包没有堆，也不能滚动：六十四个条目记在自己身上，一屏画七行，上下翻页。一个文件管理器
//! 的完整形态是复制、移动、重命名、建目录 —— 那些操作都要宿主再开口子，而这一版先给「看得见
//! 并拿得走」这一半：能进到文件那里，能删掉它。剩下的等 ABI 有相应的服务。
//!
//! 删除要按两次：第一次选中并说清楚要删的是哪一个，第二次才真的删。一个误触就删掉一张卡上
//! 的东西的文件管理器，比一个没有删除按钮的更糟。

#![no_std]

use app_sdk::{app, app_abi, format_into, host, theme, widgets, App, Context, Event, Rect, Ui};

/// 两个卷，顺序跟板子上报的一致：内置的在前，卡在后。
///
/// 挂载点是写在这里的，因为 ABI 这一版没有「列出卷」这个服务；打不开的那个卷（卡不在槽里）
/// 读出来就是一个失败的 `dir_open`，页面上也就写成「读不到」—— 一个空槽和一个坏掉的卷在这
/// 一版看上去是一回事，这是 ABI 还没有那个服务的代价。
const VOLUMES: [(&str, &str); 2] = [("内部存储", "/internal"), ("SD 卡", "/sdcard")];

/// 一次记住多少个条目。
const MAX_ENTRIES: usize = 64;
/// 一屏画几行。
const ROWS: usize = 7;
/// 一个名字最长多少字节。
const NAME: usize = 40;
/// 一个路径最长多少字节。
const PATH: usize = 128;

/// 一个条目。`Default` 是自己写的而不是推导的：四十字节的数组推导不出 `Default`（只有三十二
/// 以内的能），而清空整个列表是一次 `self.entries = [Entry::default(); N]`。
#[derive(Clone, Copy)]
struct Entry {
    kind: u8,
    name: [u8; NAME],
    name_len: usize,
    size: u64,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            kind: 0,
            name: [0; NAME],
            name_len: 0,
            size: 0,
        }
    }
}

impl Entry {
    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }

    fn is_dir(&self) -> bool {
        self.kind == app_abi::dir_entry::DIR as u8
    }
}

struct Files {
    volume: usize,
    cwd: [u8; PATH],
    cwd_len: usize,
    entries: [Entry; MAX_ENTRIES],
    count: usize,
    page: usize,
    selected: Option<usize>,
    /// 删除按两次：这里记着第一次按下的是哪一个。
    confirm: Option<usize>,
    /// 上一次列目录的结果，用来区分「这个目录是空的」和「这个卷读不到」。
    listed: bool,
    pressed: Option<Rect>,
    row_pressed: Option<usize>,
}

impl Files {
    fn cwd(&self) -> &str {
        core::str::from_utf8(&self.cwd[..self.cwd_len]).unwrap_or("")
    }

    fn root(&self) -> &'static str {
        VOLUMES[self.volume].1
    }

    /// 进到 `path` 并重新列一次。
    fn open(&mut self, path: &str) {
        let len = path.len().min(self.cwd.len());

        self.cwd[..len].copy_from_slice(&path.as_bytes()[..len]);
        self.cwd_len = len;
        self.page = 0;
        self.selected = None;
        self.confirm = None;

        self.reload();
    }

    /// 列一次：打开、逐条抄进来、关掉。抄不进来（读不到、被拒绝）就把条目清空并记下来。
    ///
    /// 路径先拷到本地再问宿主：借着自己的东西去改自己，借不出第二次。
    fn reload(&mut self) {
        self.entries = [Entry::default(); MAX_ENTRIES];
        self.count = 0;

        let mut path = [0u8; PATH];
        let len = self.cwd_len.min(path.len());
        path[..len].copy_from_slice(&self.cwd[..len]);

        let path = core::str::from_utf8(&path[..len]).unwrap_or("");

        self.listed = host::dir_open(path);

        if !self.listed {
            host::dir_close();
            return;
        }

        let mut line = [0u8; NAME + 32];

        while self.count < MAX_ENTRIES {
            let Some(len) = host::dir_next(&mut line) else {
                break;
            };

            let line = &line[..len.min(line.len())];
            let mut fields = line.split(|byte| *byte == b'\t');

            let kind = parse_u32(fields.next().unwrap_or(&[])) as u8;
            let name = fields.next().unwrap_or(&[]);
            let size = parse_u64(fields.next().unwrap_or(&[]));

            let mut entry = Entry {
                kind,
                name: [0; NAME],
                name_len: name.len().min(NAME),
                size,
            };

            entry.name[..entry.name_len].copy_from_slice(&name[..entry.name_len]);
            self.entries[self.count] = entry;
            self.count += 1;
        }

        host::dir_close();
    }

    /// 选中那一项的完整路径，用来进去或者删掉。
    fn path_of(&self, index: usize) -> ([u8; PATH], usize) {
        let mut path = [0u8; PATH];
        let entry = self.entries[index];
        let dir = self.cwd().trim_end_matches('/');

        let mut at = 0;

        for byte in dir.as_bytes() {
            if at >= path.len() {
                break;
            }

            path[at] = *byte;
            at += 1;
        }

        if at < path.len() {
            path[at] = b'/';
            at += 1;
        }

        for byte in &entry.name[..entry.name_len] {
            if at >= path.len() {
                break;
            }

            path[at] = *byte;
            at += 1;
        }

        (path, at)
    }

    /// 上一级：在卷的根就停住，因为再往上是同一个地方。
    fn up(&mut self) {
        let mut owned = [0u8; PATH];
        let len = self.cwd_len.min(owned.len());
        owned[..len].copy_from_slice(&self.cwd[..len]);

        let dir = core::str::from_utf8(&owned[..len])
            .unwrap_or("")
            .trim_end_matches('/');
        let root = self.root();

        if dir.len() <= root.len() {
            return;
        }

        let above = match dir.rfind('/') {
            Some(0) => "/",
            Some(index) => {
                let above = &dir[..index];

                if above.len() < root.len() {
                    root
                } else {
                    above
                }
            }
            None => root,
        };

        let mut next = [0u8; PATH];
        let len = above.len().min(next.len());
        next[..len].copy_from_slice(&above.as_bytes()[..len]);

        let text = core::str::from_utf8(&next[..len]).unwrap_or(root);

        self.open(text);
    }

    fn pages(&self) -> usize {
        self.count.div_ceil(ROWS).max(1)
    }
}

fn parse_u32(bytes: &[u8]) -> u32 {
    let mut value: u32 = 0;

    for byte in bytes {
        if !byte.is_ascii_digit() {
            break;
        }

        value = value * 10 + (byte - b'0') as u32;
    }

    value
}

fn parse_u64(bytes: &[u8]) -> u64 {
    let mut value: u64 = 0;

    for byte in bytes {
        if !byte.is_ascii_digit() {
            break;
        }

        value = value * 10 + (byte - b'0') as u64;
    }

    value
}

/// 一个大小的那句话：字节、KB、MB、GB，各留一位小数，因为「1.2 MB」比「1258291」更像一眼
/// 能拿走的东西。
fn size_text(size: u64, out: &mut [u8]) -> &str {
    const UNITS: [&str; 4] = [" B", " KB", " MB", " GB"];
    let mut value = size as f32;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    let mut buffer = app_sdk::text::Buf::new(out);

    if unit == 0 {
        let _ = core::fmt::Write::write_fmt(&mut buffer, format_args!("{}{}", size, UNITS[unit]));
    } else {
        let whole = value as u32;
        let tenths = ((value - whole as f32) * 10.0) as u32;
        let _ = core::fmt::Write::write_fmt(
            &mut buffer,
            format_args!("{}.{}{}", whole, tenths, UNITS[unit]),
        );
    }

    let len = buffer.len();

    core::str::from_utf8(&out[..len]).unwrap_or("")
}

fn volume_rect(index: usize) -> Rect {
    Rect::new(18.0 + index as f32 * 228.0, 62.0, 222.0, 48.0)
}

fn row_rect(index: usize) -> Rect {
    Rect::new(18.0, 152.0 + index as f32 * 32.0, 444.0, 30.0)
}

fn footer_rect(index: usize) -> Rect {
    Rect::new(18.0 + index as f32 * 150.0, 386.0, 144.0, 44.0)
}

const DELETE: Rect = Rect::new(18.0, 438.0, 444.0, 38.0);

impl App for Files {
    fn new(_: &Context) -> Self {
        let mut files = Self {
            volume: 0,
            cwd: [0; PATH],
            cwd_len: 0,
            entries: [Entry::default(); MAX_ENTRIES],
            count: 0,
            page: 0,
            selected: None,
            confirm: None,
            listed: false,
            pressed: None,
            row_pressed: None,
        };

        files.open(VOLUMES[0].1);

        files
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.pressed = None;
        self.row_pressed = None;

        for index in 0..VOLUMES.len() {
            let rect = volume_rect(index);

            if context.input.pressing(rect) {
                self.pressed = Some(rect);
            }

            if context.tapped(rect) && self.volume != index {
                self.volume = index;
                self.open(self.root());
            }
        }

        let start = self.page * ROWS;
        let shown = (self.count - start).min(ROWS);

        for index in 0..shown {
            let rect = row_rect(index);
            let entry = start + index;

            if context.input.pressing(rect) {
                self.row_pressed = Some(index);
            }

            if context.tapped(rect) {
                let (path, len) = self.path_of(entry);
                let path = core::str::from_utf8(&path[..len]).unwrap_or("");

                if self.entries[entry].is_dir() {
                    self.open(path);
                } else {
                    self.selected = Some(entry);
                    self.confirm = None;
                }

                return;
            }
        }

        let actions = ["上一级", "上一页", "下一页"];

        for index in 0..actions.len() {
            let rect = footer_rect(index);

            if context.input.pressing(rect) {
                self.pressed = Some(rect);
            }

            if !context.tapped(rect) {
                continue;
            }

            match index {
                0 => self.up(),
                1 => self.page = self.page.saturating_sub(1),
                _ => {
                    if (self.page + 1) * ROWS < self.count {
                        self.page += 1;
                    }
                }
            }
        }

        if context.input.pressing(DELETE) {
            self.pressed = Some(DELETE);
        }

        if context.tapped(DELETE) {
            let Some(entry) = self.selected else {
                return;
            };

            // 两次：第一次只是把话说清楚（按钮上写着要删的是哪一个），第二次才动手。
            if self.confirm == Some(entry) {
                let (path, len) = self.path_of(entry);
                let path = core::str::from_utf8(&path[..len]).unwrap_or("");

                host::path_remove(path);
                self.confirm = None;
                self.selected = None;
                self.reload();
            } else {
                self.confirm = Some(entry);
            }
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        ui.text(24.0, 28.0, "文件管理", 22.0, theme::MUTED);

        for (index, (name, _)) in VOLUMES.iter().enumerate() {
            let rect = volume_rect(index);

            widgets::button(
                ui,
                rect,
                name,
                20.0,
                if index == self.volume {
                    theme::ACCENT
                } else {
                    theme::SURFACE_HIGH
                },
                theme::TEXT,
                self.pressed == Some(rect),
            );
        }

        // 路径：一个人在文件管理器里最先要知道的是「我在哪」。
        ui.text(24.0, 126.0, self.cwd(), 16.0, theme::MUTED);
        widgets::divider(ui, Rect::new(18.0, 142.0, 444.0, 1.0));

        if !self.listed {
            ui.text_centered(
                Rect::new(18.0, 180.0, 444.0, 60.0),
                "这个卷读不到（卡不在槽里？）",
                20.0,
                theme::MUTED,
            );
        } else if self.count == 0 {
            ui.text_centered(
                Rect::new(18.0, 180.0, 444.0, 60.0),
                "这个目录是空的",
                20.0,
                theme::MUTED,
            );
        } else {
            let start = self.page * ROWS;
            let shown = (self.count - start).min(ROWS);

            for index in 0..shown {
                let entry = self.entries[start + index];
                let rect = row_rect(index);
                let chosen = self.selected == Some(start + index);

                if chosen || self.row_pressed == Some(index) {
                    ui.rect(rect, theme::SURFACE_HIGH);
                }

                let mut storage = [0u8; 24];
                let detail = if entry.is_dir() {
                    "目录"
                } else {
                    size_text(entry.size, &mut storage)
                };

                ui.text_in(
                    Rect::new(rect.x + 8.0, rect.y + 6.0, rect.width - 16.0, 20.0),
                    entry.name(),
                    16.0,
                    if entry.is_dir() { theme::TEXT } else { theme::MUTED },
                    app_abi::Align::Left,
                );

                ui.text_in(
                    Rect::new(rect.x + 8.0, rect.y + 6.0, rect.width - 16.0, 20.0),
                    detail,
                    16.0,
                    theme::MUTED,
                    app_abi::Align::Right,
                );
            }

            let mut storage = [0u8; 24];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let page = self.page + 1;
            let pages = self.pages();
            let note = format_into!(buffer, "{page} / {pages}");

            ui.text_in(
                Rect::new(24.0, 126.0, 432.0, 20.0),
                note,
                14.0,
                theme::MUTED,
                app_abi::Align::Right,
            );
        }

        let actions = ["上一级", "上一页", "下一页"];

        for (index, label) in actions.iter().enumerate() {
            let rect = footer_rect(index);

            widgets::button(
                ui,
                rect,
                label,
                18.0,
                theme::SURFACE_HIGH,
                theme::TEXT,
                self.pressed == Some(rect),
            );
        }

        // 删除：按钮上写着要删的是哪一个，第二次按下才真的动手。缓冲区在 `match` 外面声明，
        // 因为格式化借走它，而按钮要用的是那句话本身。
        let mut storage = [0u8; NAME + 24];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);

        let label = match (self.selected, self.confirm) {
            (Some(entry), Some(confirmed)) if entry == confirmed => {
                let name = self.entries[entry].name();

                format_into!(buffer, "再按一次删除 {name}")
            }
            (Some(entry), _) => {
                let name = self.entries[entry].name();

                format_into!(buffer, "删除 {name}")
            }
            _ => "先选中一个文件或目录",
        };

        widgets::button(
            ui,
            DELETE,
            label,
            16.0,
            if self.confirm.is_some() {
                theme::DANGER
            } else {
                theme::SURFACE_HIGH
            },
            theme::TEXT,
            self.pressed == Some(DELETE),
        );
    }
}

app!(Files);
