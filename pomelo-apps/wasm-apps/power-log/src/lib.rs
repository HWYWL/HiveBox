//! 电量日志，作为一个可安装的包。
//!
//! 每三十秒记一次电量，画成一条一小时的曲线，并把这条曲线留在包自己的文件里：关掉再打开，
//! 线接着画下去。电池本身这个包碰不到 —— 电量、电压、是否在充电，全部来自宿主的
//! [`host::system`]，包只是把它们按时间排起来。
//!
//! # 为什么采样在 `view` 里
//!
//! 因为这一页没有自己的时钟：帧是宿主给的，`Context::now_ms` 是宿主那一帧的时刻，包只是
//! 问「距上次采样过了多久」。这样就没有定时器要维护，也没有两条时间线会对不上 —— 一个
//! 应用只画宿主说的那个时刻，时钟包也是这个道理。
//!
//! 读数一帧只取一次（[`host::system`] 的文档说得很清楚：一帧一次可以，一帧几次不行），取完
//! 存在自己身上，曲线和下面那几行都读同一份。

#![no_std]

use app_sdk::{app, format_into, host, theme, widgets, App, Context, Event, Rect, Ui};

/// 一小时的采样：三十秒一个，一百二十个。
const SAMPLES: usize = 120;
const INTERVAL_MS: i64 = 30_000;

/// 曲线存在包自己的这个文件里，长度固定：两字节的个数，然后是每个采样两个字节。
const FILE: &str = "power-log.bin";
const HEADER: usize = 2;
const FILE_LEN: usize = HEADER + SAMPLES * 2;

/// 采样太密是浪费，太疏则看不出一次充电的台阶；三十秒是这两件事之间的地方。
struct PowerLog {
    percent: [u8; SAMPLES],
    charging: [u8; SAMPLES],
    /// 已经记了多少个，最多 [`SAMPLES`]。
    count: usize,
    /// 环里下一个写入的位置。
    at: usize,
    /// 上次采样的时刻。第一个采样在打开时立刻取，所以初值是 `i64::MIN`。
    sampled_ms: i64,
    /// 这一帧的读数，取一次给整帧用。
    battery: u32,
    millivolts: u32,
    charging_now: u32,
    uptime: u32,
    clear_pressed: bool,
}

impl PowerLog {
    /// 环里第 `index` 个（0 是最老的一个）在数组里的下标。
    fn slot(&self, index: usize) -> usize {
        let start = if self.count < SAMPLES {
            0
        } else {
            self.at % SAMPLES
        };

        (start + index) % SAMPLES
    }

    fn push(&mut self, percent: u8, charging: u8) {
        self.percent[self.at] = percent;
        self.charging[self.at] = charging;
        self.at = (self.at + 1) % SAMPLES;

        if self.count < SAMPLES {
            self.count += 1;
        }
    }

    fn clear(&mut self) {
        self.percent = [0; SAMPLES];
        self.charging = [0; SAMPLES];
        self.count = 0;
        self.at = 0;
        self.sampled_ms = i64::MIN;
    }

    /// 曲线写回文件。写失败不算什么：下一次打开只是从头开始画，比为了一个日志让页面报错好。
    fn save(&self) {
        let mut buffer = [0u8; FILE_LEN];
        buffer[..HEADER].copy_from_slice(&(self.count as u16).to_le_bytes());

        for index in 0..self.count {
            let slot = self.slot(index);
            let at = HEADER + index * 2;

            buffer[at] = self.percent[slot];
            buffer[at + 1] = self.charging[slot];
        }

        host::file_write(FILE, &buffer[..HEADER + self.count * 2]);
    }

    /// 上次留下来的曲线。个数超过 [`SAMPLES`] 的文件不是这个版本写的，那就当没有。
    fn load(&mut self) {
        let mut buffer = [0u8; FILE_LEN];

        let Some(len) = host::file_read(FILE, &mut buffer) else {
            return;
        };

        if len < HEADER {
            return;
        }

        let count = u16::from_le_bytes([buffer[0], buffer[1]]) as usize;

        if count > SAMPLES || len < HEADER + count * 2 {
            return;
        }

        for index in 0..count {
            let at = HEADER + index * 2;

            self.push(buffer[at], buffer[at + 1]);
        }
    }

    /// 曲线上第 `index` 个点，落在 `rect` 里的坐标。
    fn point(&self, index: usize, rect: Rect) -> (f32, f32) {
        let span = (self.count.max(2) - 1) as f32;
        let x = rect.x + 8.0 + (rect.width - 16.0) * (index as f32 / span);
        let percent = self.percent[self.slot(index)] as f32 / 100.0;
        let y = rect.bottom() - 10.0 - (rect.height - 20.0) * percent;

        (x, y)
    }
}

impl App for PowerLog {
    fn new(context: &Context) -> Self {
        let mut log = Self {
            percent: [0; SAMPLES],
            charging: [0; SAMPLES],
            count: 0,
            at: 0,
            sampled_ms: i64::MIN,
            battery: 0,
            millivolts: 0,
            charging_now: 0,
            uptime: 0,
            clear_pressed: false,
        };

        log.load();

        if let Some(readings) = host::system() {
            log.battery = readings.battery_percent;
            log.millivolts = readings.battery_mv;
            log.charging_now = readings.charging;
            log.uptime = readings.uptime_secs;

            // 一打开就有一个点：空白的曲线要等三十秒才有第一条，那是看着像坏了。
            log.push(readings.battery_percent.min(100) as u8, readings.charging as u8);
            log.sampled_ms = context.now_ms;
        }

        log
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        let clear = Rect::new(18.0, 396.0, 444.0, 56.0);

        self.clear_pressed = context.input.pressing(clear);

        if context.tapped(clear) {
            self.clear();
            host::file_write(FILE, &[]);
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        let now = host::now_ms();

        if let Some(readings) = host::system() {
            self.battery = readings.battery_percent;
            self.millivolts = readings.battery_mv;
            self.charging_now = readings.charging;
            self.uptime = readings.uptime_secs;

            if now - self.sampled_ms >= INTERVAL_MS {
                self.push(readings.battery_percent.min(100) as u8, readings.charging as u8);
                self.sampled_ms = now;
                self.save();
            }
        }

        ui.text(24.0, 28.0, "电量日志", 22.0, theme::MUTED);

        let mut storage = [0u8; 8];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);
        let percent = format_into!(buffer, "{}%", self.battery);

        widgets::reading(
            ui,
            Rect::new(24.0, 58.0, 240.0, 90.0),
            percent,
            if self.charging_now != 0 {
                "充电中"
            } else {
                "使用电池"
            },
            52.0,
        );

        widgets::progress(
            ui,
            Rect::new(24.0, 136.0, 432.0, 12.0),
            self.battery as f32 / 100.0,
            if self.battery <= 20 {
                theme::DANGER
            } else {
                theme::OK
            },
        );

        // 曲线：一小时的电量，充电那段用另一种颜色，因为一次充电在曲线上是一级台阶，看得见
        // 它什么时候插上的比知道它插了多久更有用。
        let curve = Rect::new(18.0, 170.0, 444.0, 150.0);
        widgets::card(ui, curve, theme::SURFACE);

        for step in [25u32, 50, 75] {
            let y = curve.bottom() - 10.0 - (curve.height - 20.0) * (step as f32 / 100.0);
            ui.rect(Rect::new(curve.x + 8.0, y, curve.width - 16.0, 1.0), theme::DIVIDER);
        }

        if self.count < 2 {
            ui.text_centered(curve, "正在采样", 20.0, theme::MUTED);
        } else {
            for index in 1..self.count {
                let (from_x, from_y) = self.point(index - 1, curve);
                let (to_x, to_y) = self.point(index, curve);

                let charging = self.charging[self.slot(index)] != 0;
                ui.line(
                    (from_x, from_y),
                    (to_x, to_y),
                    2.0,
                    if charging { theme::ACCENT } else { theme::OK },
                );
            }
        }

        let mut storage = [0u8; 24];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);

        let volts = format_into!(buffer, "{}.{:03} V", self.millivolts / 1000, self.millivolts % 1000);
        widgets::key_value(ui, Rect::new(24.0, 336.0, 432.0, 24.0), "电压", volts, 20.0);

        buffer.reset();
        let samples = format_into!(buffer, "{} / {}", self.count, SAMPLES);
        widgets::key_value(ui, Rect::new(24.0, 364.0, 432.0, 24.0), "采样", samples, 20.0);

        widgets::button(
            ui,
            Rect::new(18.0, 396.0, 444.0, 56.0),
            "清空",
            24.0,
            theme::SURFACE_HIGH,
            theme::TEXT,
            self.clear_pressed,
        );

        let mut storage = [0u8; 32];
        let mut buffer = app_sdk::text::Buf::new(&mut storage);

        // 开机时长放在最后，是因为它跟电量无关：它回答的是「这条曲线跨了多久」，而曲线本身
        // 断过（重启）也看不出来。
        let uptime = self.uptime;
        let note = format_into!(buffer, "三十秒一个采样，开机 {uptime} 秒");

        ui.text(24.0, 464.0, note, 14.0, theme::MUTED);
    }

    fn on_stop(&mut self) {
        self.save();
    }
}

app!(PowerLog);
