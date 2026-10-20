//! 音乐，作为一个可安装的包。
//!
//! 卡上 `music` 目录里的一列曲目：点一首，它就放；下面是音量、暂停和停。解码和扬声器是固件
//! 那一侧的事（`AUDIO` 权限），列出目录是宿主的（`FILES` 权限）—— 这个包没有解码器，也没有
//! 一次 `read_dir`：它只会问「这个目录下一条是什么」，而名字的后缀决定要不要把它列出来。
//!
//! # 播什么由后缀决定，是什么由解码器决定
//!
//! 内置版是这么说的，这里照抄：`mp3`/`wav`/`flac`/`ogg`/`m4a`/`aac` 只决定「要不要试一试」，
//! 一个叫 `.wav` 却装着别的东西的文件，是 `hb_audio_play` 拒绝的，而不是这一页假装能放的。
//!
//! # 没有扫描，没有播放列表文件
//!
//! 一打开就读一遍目录，记住四十首，翻页看。卡片拔掉、曲子删掉，按一下「重读」就有了。一个
//! 自己维护着一份列表文件的播放器，和一份会过期的列表文件，是两件都要有人照顾的东西。

#![no_std]

use app_sdk::{app, app_abi, format_into, host, theme, widgets, App, Context, Event, Rect, Ui};

/// 依序试这几个目录：卡上的 `music`，然后卡根，然后内置存储。第一个读得到又有东西的就用它。
const DIRS: [&str; 3] = ["/sdcard/music", "/sdcard", "/internal"];

/// 一次记住多少首。
const MAX_TRACKS: usize = 40;
/// 一屏画几行。
const ROWS: usize = 8;
/// 一个名字最长多少字节。
const NAME: usize = 48;

/// 认这几个后缀。小写比较：一张卡上的名字是什么大小写都有。
const EXTS: [&[u8]; 6] = [b"mp3", b"wav", b"flac", b"ogg", b"m4a", b"aac"];

/// 一首。`Default` 自己写：数组超过三十二字节推导不出 `Default`，而清空列表要用它。
#[derive(Clone, Copy)]
struct Track {
    name: [u8; NAME],
    name_len: usize,
    /// 完整路径，播放时原样交给宿主。
    path: [u8; NAME + 24],
    path_len: usize,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            name: [0; NAME],
            name_len: 0,
            path: [0; NAME + 24],
            path_len: 0,
        }
    }
}

impl Track {
    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("?")
    }

    fn path(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("")
    }
}

struct Player {
    /// 现在看的是 `DIRS` 里的第几个。
    dir: usize,
    tracks: [Track; MAX_TRACKS],
    count: usize,
    page: usize,
    /// 正在放的是第几首，没有就是 `None`。
    playing: Option<usize>,
    /// 宿主的读数：在放吗，音量多少。
    is_playing: bool,
    volume: u32,
    pressed: Option<Rect>,
    row_pressed: Option<usize>,
}

impl Player {
    /// 读一遍现在这个目录：后缀对得上的留下，其余跳过。
    fn scan(&mut self) {
        self.tracks = [Track::default(); MAX_TRACKS];
        self.count = 0;
        self.page = 0;

        if !host::dir_open(DIRS[self.dir]) {
            host::dir_close();
            return;
        }

        let mut line = [0u8; NAME + 64];

        while self.count < MAX_TRACKS {
            let Some(len) = host::dir_next(&mut line) else {
                break;
            };

            let line = &line[..len.min(line.len())];
            let mut fields = line.split(|byte| *byte == b'\t');

            let kind = fields.next().unwrap_or(&[]);
            let name = fields.next().unwrap_or(&[]);

            // 目录不看：这一版不做递归，一个目录套一个目录的音乐库要等 ABI 有能力往下走。
            if kind.first() == Some(&b'1') || !matches_ext(name) {
                continue;
            }

            let mut track = Track::default();
            track.name_len = name.len().min(NAME);
            track.name[..track.name_len].copy_from_slice(&name[..track.name_len]);

            let dir = DIRS[self.dir].trim_end_matches('/');
            let mut at = 0;

            for byte in dir.as_bytes() {
                if at >= track.path.len() {
                    break;
                }

                track.path[at] = *byte;
                at += 1;
            }

            if at < track.path.len() {
                track.path[at] = b'/';
                at += 1;
            }

            for byte in &name[..track.name_len] {
                if at >= track.path.len() {
                    break;
                }

                track.path[at] = *byte;
                at += 1;
            }

            track.path_len = at;
            self.tracks[self.count] = track;
            self.count += 1;
        }

        host::dir_close();
    }

    fn pages(&self) -> usize {
        self.count.div_ceil(ROWS).max(1)
    }

    /// 音量加减用的是宿主的档，而宿主的档是百分比。
    fn nudge(&self, step: i32) {
        let volume = (self.volume as i32 + step).clamp(0, 100);

        host::audio_control(app_abi::audio::VOLUME, volume);
    }
}

/// 名字的后缀是不是一个要试一试的后缀。
fn matches_ext(name: &[u8]) -> bool {
    let Some(dot) = name.iter().rposition(|byte| *byte == b'.') else {
        return false;
    };

    let ext = &name[dot + 1..];
    let mut lower = [0u8; 8];
    let len = ext.len().min(lower.len());

    for (at, byte) in lower[..len].iter_mut().enumerate() {
        *byte = ext[at].to_ascii_lowercase();
    }

    EXTS.iter().any(|known| &lower[..len] == *known)
}

fn row_rect(index: usize) -> Rect {
    Rect::new(18.0, 130.0 + index as f32 * 34.0, 444.0, 32.0)
}

/// 下面的四个按钮：播放 / 暂停、停止、音量减、音量加。
fn control_rect(index: usize) -> Rect {
    Rect::new(18.0 + index as f32 * 114.0, 392.0, 108.0, 60.0)
}

fn dir_rect() -> Rect {
    Rect::new(18.0, 84.0, 444.0, 36.0)
}

impl App for Player {
    fn new(_: &Context) -> Self {
        let mut player = Self {
            dir: 0,
            tracks: [Track::default(); MAX_TRACKS],
            count: 0,
            page: 0,
            playing: None,
            is_playing: false,
            volume: 0,
            pressed: None,
            row_pressed: None,
        };

        player.scan();

        player
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.pressed = None;
        self.row_pressed = None;

        let start = self.page * ROWS;
        let shown = (self.count - start).min(ROWS);

        for index in 0..shown {
            let rect = row_rect(index);
            let track = start + index;

            if context.input.pressing(rect) {
                self.row_pressed = Some(index);
            }

            if context.tapped(rect) {
                // 后缀决定要不要试，而这才是试：播不播得了是固件那一侧的回答。
                host::audio_play(self.tracks[track].path());
                self.playing = Some(track);

                return;
            }
        }

        // 一个按钮在两个动作之间换，而不是两个按钮各管一个：播放和暂停是同一件事的两面，而
        // 一个按下去什么也不会发生的按钮是更难看的那种。
        let controls = [
            if self.is_playing {
                app_abi::audio::PAUSE
            } else {
                app_abi::audio::RESUME
            },
            app_abi::audio::STOP,
        ];

        for index in 0..controls.len() {
            let rect = control_rect(index);

            if context.input.pressing(rect) {
                self.pressed = Some(rect);
            }

            if context.tapped(rect) {
                host::audio_control(controls[index], 0);
            }
        }

        for index in 2..4 {
            let rect = control_rect(index);

            if context.input.pressing(rect) {
                self.pressed = Some(rect);
            }

            if context.tapped(rect) {
                self.nudge(if index == 2 { -10 } else { 10 });
            }
        }

        if context.input.pressing(dir_rect()) {
            self.pressed = Some(dir_rect());
        }

        // 换目录：卡不在槽里的时候，这一下就是「再看一次」。
        if context.tapped(dir_rect()) {
            self.dir = (self.dir + 1) % DIRS.len();
            self.playing = None;
            self.scan();
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        if let Some(state) = host::audio_state() {
            self.is_playing = state.playing != 0;
            self.volume = state.volume;
        }

        ui.text(24.0, 28.0, "音乐", 22.0, theme::MUTED);

        let mut storage = [0u8; 24];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let volume = self.volume;
        let reading = format_into!(buffer, "音量 {volume}");

        ui.text_in(
            Rect::new(24.0, 24.0, 432.0, 28.0),
            reading,
            18.0,
            theme::MUTED,
            app_abi::Align::Right,
        );

        widgets::button(
            ui,
            dir_rect(),
            DIRS[self.dir],
            16.0,
            theme::SURFACE_HIGH,
            theme::TEXT,
            self.pressed == Some(dir_rect()),
        );

        if self.count == 0 {
            ui.text_centered(
                Rect::new(18.0, 200.0, 444.0, 60.0),
                "这个目录里没有音乐，点上面换一个",
                20.0,
                theme::MUTED,
            );
        } else {
            let start = self.page * ROWS;
            let shown = (self.count - start).min(ROWS);

            for index in 0..shown {
                let track = self.tracks[start + index];
                let rect = row_rect(index);
                let chosen = self.playing == Some(start + index);

                widgets::list_row(
                    ui,
                    rect,
                    track.name(),
                    "",
                    chosen || self.row_pressed == Some(index),
                );

                if chosen {
                    ui.text_in(
                        Rect::new(rect.x + 16.0, rect.y + 8.0, rect.width - 32.0, 20.0),
                        if self.is_playing { "正在播放" } else { "已暂停" },
                        14.0,
                        theme::OK,
                        app_abi::Align::Right,
                    );
                }
            }

            let mut storage = [0u8; 24];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let page = self.page + 1;
            let pages = self.pages();
            let note = format_into!(buffer, "{page} / {pages}");

            ui.text_in(
                Rect::new(24.0, 60.0, 432.0, 20.0),
                note,
                14.0,
                theme::MUTED,
                app_abi::Align::Right,
            );
        }

        let first = if self.is_playing { "暂停" } else { "继续" };

        let labels = [first, "停止", "音量 -", "音量 +"];

        for (index, label) in labels.iter().enumerate() {
            let rect = control_rect(index);

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
    }
}

app!(Player);
