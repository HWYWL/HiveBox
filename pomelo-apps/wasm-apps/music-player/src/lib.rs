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

use app_sdk::{
    app, app_abi, format_into, host, icons, theme, widgets, App, Context, Event, Rect, Ui,
};

/// 依序试这几个目录：卡上的 `music`，然后卡根，然后内置存储。第一个读得到又有东西的就用它。
const DIRS: [&str; 3] = ["/sdcard/music", "/sdcard", "/internal"];

/// 一次记住多少首。
const MAX_TRACKS: usize = 40;
/// 一屏画几行。
const ROWS: usize = 6;
/// 一个名字最长多少字节。
const NAME: usize = 48;

/// 认这几个后缀。小写比较：一张卡上的名字是什么大小写都有。
const EXTS: [&[u8]; 6] = [b"mp3", b"wav", b"flac", b"ogg", b"m4a", b"aac"];

/// 这个包自己的颜色，与 `tools/build_apps.py` 里 music-player 那条目的 `#E94560` 是同一个 ——
/// 内置版正在播的那一行就是这个红。
const ACCENT: app_sdk::ui::Color = 0x00E9_4560;

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
    /// 宿主的读数：在放吗，音量多少，播到哪儿了，这一首有多长。
    is_playing: bool,
    volume: u32,
    position_ms: u32,
    duration_ms: u32,
    pressed: Option<Rect>,
    row_pressed: Option<usize>,
    /// 手指正按着哪一条（进度或音量）：按下与拖动是画在帧上的，而 `view` 看不到输入。
    bar_pressed: Option<Rect>,
}

impl Player {
    /// 一帧的开头问一次：板子才是权威，而这个包只把它说的话画出来。
    fn poll(&mut self) {
        if let Some(state) = host::audio_state() {
            self.is_playing = state.playing != 0;
            self.volume = state.volume;
            self.position_ms = state.position_ms;
            self.duration_ms = state.duration_ms;
        }
    }
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
    Rect::new(18.0, 108.0 + index as f32 * 36.0, 444.0, 34.0)
}

/// 翻页：一屏六首，四十首就是七屏，而看不到第二屏的列表是六首的列表。
fn prev_rect() -> Rect {
    Rect::new(18.0, 330.0, 120.0, 32.0)
}

fn next_rect() -> Rect {
    Rect::new(342.0, 330.0, 120.0, 32.0)
}

/// 播到哪儿了：一条可以拖的进度，而不是一句「正在播放」。
const PROGRESS: Rect = Rect::new(18.0, 374.0, 444.0, 18.0);

/// 音量：一条可以拖的，代替两个加减按钮 —— 但加减两个按钮还在，一只手拿着盒子的时候它们更快。
const VOLUME_SLIDER: Rect = Rect::new(66.0, 402.0, 356.0, 20.0);

/// 下面的四个按钮：播放 / 暂停、停止、音量减、音量加。
fn control_rect(index: usize) -> Rect {
    Rect::new(18.0 + index as f32 * 114.0, 430.0, 108.0, 44.0)
}

fn dir_rect() -> Rect {
    Rect::new(18.0, 68.0, 444.0, 32.0)
}

/// 这一帧是不是「手指刚落到条上，或者刚离开」：拖动的两个端点，中间那些帧不去打扰宿主。
fn touched(context: &Context) -> bool {
    context.input.just_down || context.input.just_up
}

/// 手指在一条横条上的位置，0 到 1。
fn fraction_of(context: &Context, rect: Rect) -> f32 {
    let fraction = (context.input.x - rect.x) / rect.width;

    if fraction < 0.0 {
        0.0
    } else if fraction > 1.0 {
        1.0
    } else {
        fraction
    }
}

/// `1:23` 这样的一段时间，写进 `out`，返回写了几个字节。
///
/// 返回长度而不是那句话：格式化借走缓冲区，而借着它的东西活不到这个函数之外。
fn clock(out: &mut [u8], ms: u32) -> usize {
    let seconds = ms / 1000;
    let mut buffer = app_sdk::text::Buf::new(out);

    format_into!(buffer, "{}:{:02}", seconds / 60, seconds % 60).len()
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
            position_ms: 0,
            duration_ms: 0,
            pressed: None,
            row_pressed: None,
            bar_pressed: None,
        };

        player.scan();

        player
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.pressed = None;
        self.row_pressed = None;
        self.bar_pressed = None;
        self.poll();

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

        // 翻页。列表是六行一屏，而四十首的卡在一屏里放不下 —— 没有这两个按钮，第二屏之后的
        // 那些歌就谁也点不到。
        if context.input.pressing(prev_rect()) {
            self.pressed = Some(prev_rect());
        }

        if context.tapped(prev_rect()) && self.page > 0 {
            self.page -= 1;
            return;
        }

        if context.input.pressing(next_rect()) {
            self.pressed = Some(next_rect());
        }

        if context.tapped(next_rect()) && self.page + 1 < self.pages() {
            self.page += 1;
            return;
        }

        // 拖进度与拖音量：手指落在条上的那一刻与离开的那一刻各说一次，而不是每一帧都说 ——
        // 一帧一次 seek 是把解码器按住擦洗。
        if context.input.pressing(PROGRESS) {
            self.bar_pressed = Some(PROGRESS);
        }

        if self.duration_ms > 0 && context.input.pressing(PROGRESS) && touched(context) {
            let fraction = fraction_of(context, PROGRESS);

            host::audio_control(
                app_abi::audio::SEEK,
                (fraction * self.duration_ms as f32) as i32,
            );
        }

        if context.input.pressing(VOLUME_SLIDER) {
            self.bar_pressed = Some(VOLUME_SLIDER);
        }

        if context.input.pressing(VOLUME_SLIDER) && touched(context) {
            let fraction = fraction_of(context, VOLUME_SLIDER);

            host::audio_control(app_abi::audio::VOLUME, (fraction * 100.0) as i32);
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

        self.poll();

        // 页头：这一页的图标与名字，下面是这个目录里有几首、音量多少。
        let mut storage = [0u8; 32];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let (count, volume) = (self.count, self.volume);
        let subtitle = format_into!(buffer, "{count} 首 · 音量 {volume}");

        widgets::page_header(
            ui,
            Rect::new(18.0, 18.0, 300.0, 44.0),
            icons::MUSIC_NOTE,
            ACCENT,
            "音乐",
            subtitle,
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
            widgets::empty(
                ui,
                Rect::new(110.0, 200.0, 320.0, 90.0),
                icons::MUSIC_NOTE,
                "这个目录里没有音乐，点上面换一个",
                theme::MUTED,
            );
        } else {
            let start = self.page * ROWS;
            let shown = (self.count - start).min(ROWS);

            for index in 0..shown {
                let track = self.tracks[start + index];
                let rect = row_rect(index);
                let chosen = self.playing == Some(start + index);

                // 正在播的那一行整行是红的，图标也从「文件」换成「正在放 / 停着」：内置版是这么
                // 标出这一行的，一眼看得见。
                let (glyph, value) = if chosen {
                    (
                        if self.is_playing {
                            icons::PAUSE
                        } else {
                            icons::PLAY_ARROW
                        },
                        if self.is_playing { "正在播放" } else { "已暂停" },
                    )
                } else {
                    (icons::AUDIO_FILE, "")
                };

                widgets::row(
                    ui,
                    rect,
                    glyph,
                    if chosen { ACCENT } else { theme::MUTED },
                    track.name(),
                    "",
                    value,
                    self.row_pressed == Some(index),
                    if chosen {
                        theme::shade(ACCENT, 62)
                    } else {
                        theme::SURFACE
                    },
                    if chosen { Some(ACCENT) } else { None },
                );
            }

            let mut storage = [0u8; 24];
            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let page = self.page + 1;
            let pages = self.pages();

            widgets::button(
                ui,
                prev_rect(),
                "上一页",
                16.0,
                if self.page > 0 { ACCENT } else { theme::SURFACE_HIGH },
                theme::TEXT,
                self.pressed == Some(prev_rect()),
            );
            widgets::button(
                ui,
                next_rect(),
                "下一页",
                16.0,
                if self.page + 1 < pages {
                    ACCENT
                } else {
                    theme::SURFACE_HIGH
                },
                theme::TEXT,
                self.pressed == Some(next_rect()),
            );

            let note = format_into!(buffer, "{page} / {pages}");
            ui.text_centered(Rect::new(138.0, 326.0, 204.0, 32.0), note, 16.0, theme::MUTED);
        }

        // 播到哪儿了：内置版这一页有一条可以拖的进度，这一条连同「跳到哪儿」一起回来了。
        if self.playing.is_some() && self.duration_ms > 0 {
            // 三个缓冲区：两个时刻各借一个，那句话借第三个 —— 格式化是借走缓冲区的，而上一行
            // 的字还在用它。
            let mut at = [0u8; 16];
            let mut bt = [0u8; 16];
            let mut storage = [0u8; 32];

            let position_len = clock(&mut at, self.position_ms);
            let total_len = clock(&mut bt, self.duration_ms);

            let position = core::str::from_utf8(&at[..position_len]).unwrap_or("");
            let total = core::str::from_utf8(&bt[..total_len]).unwrap_or("");

            let mut buffer = app_sdk::text::Buf::new(&mut storage);
            let line = format_into!(buffer, "{position} / {total}");

            ui.text(18.0, 362.0, line, 14.0, theme::MUTED);

            let fraction = self.position_ms as f32 / self.duration_ms as f32;

            widgets::slider(
                ui,
                PROGRESS,
                fraction,
                ACCENT,
                self.bar_pressed == Some(PROGRESS),
            );
        }

        // 音量：一条可以拖的，两边是它的两个图标。加减两个按钮还在下面 —— 一只手拿着盒子的
        // 时候，按一下比拖一下准。
        ui.icon(18.0, 404.0, 20.0, theme::MUTED, icons::VOLUME_DOWN);
        ui.icon(430.0, 404.0, 20.0, theme::MUTED, icons::VOLUME_UP);

        widgets::slider(
            ui,
            VOLUME_SLIDER,
            self.volume as f32 / 100.0,
            ACCENT,
            self.bar_pressed == Some(VOLUME_SLIDER),
        );

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
