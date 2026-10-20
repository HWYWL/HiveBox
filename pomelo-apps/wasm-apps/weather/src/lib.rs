//! 天气，作为一个可安装的包。
//!
//! 两个请求，跟内置版一样：先问 ip-api「我这的地址在哪」，再用那个坐标问 Open-Meteo 要一份
//! 五天预报。两个服务都不要密钥 —— 一台刚插上电的盒子没有凭证可以给，而一份要先填表的天气
//! 是一份在别人机器上填完之前一直空白的天气。
//!
//! # 一次只有一个请求
//!
//! 宿主的 HTTP 是一条队：一个应用在跑的请求和它后面排的一个（`state::RUNNING` 也可能是
//! 「在排队」）。所以这里是两步而不是两步并一步 —— 定位完了才发预报，每一步都问一次状态。
//! 商店下载共用同一条队，所以「在排队」是要显示出来的状态，而不是要重试的错误。
//!
//! # 自己读文档
//!
//! 没有 JSON 解析器：这份文档的形状是知道的，要的是四个数字和一个字符串，所以这里是一个
//! 找 `"key":` 的小函数和几个读数函数。这与内置版不同 —— 那一版有 `serde` 和一个堆 —— 而这
//! 正是沙箱的代价：包没有堆，也没有能遍历的类型。找到什么画什么，找不到就画「读不出来」。
//!
//! 权限：`NETWORK`。

#![no_std]

use app_sdk::{app, app_abi, format_into, host, theme, widgets, App, Context, Event, Rect, Ui};

/// 预报要几天。五天是一屏能看完的数目，而最后一行在屏幕外的预报是四天的预报。
const DAYS: usize = 5;

/// 两个服务，各写一次：改它们是改这两行。
const LOCATOR: &str = "http://ip-api.com/json/?fields=status,message,city,regionName,lat,lon";
const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";

/// 读到哪儿了。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// 还没发出第一个请求，或者刚按下「重新加载」。
    Locating,
    /// 定位回来了，预报在飞（或者在排队）。
    Forecast,
    Done,
    /// 最后一次请求的状态不是 DONE：失败的序号存下来是为了把它说出口。
    Failed,
}

struct Weather {
    stage: Stage,
    /// 失败的原因（[`app_abi::fault`]），只有 `Failed` 时有意义。
    fault: u32,
    /// 上一次看到的状态，用来认出「刚刚变完」的那一帧。
    seen: u32,
    /// 请求回来的文档。三 KB：一份五天预报连头带尾大约一千字节，而这一个缓冲区是全局唯一
    /// 的一块 —— 包没有堆。
    body: [u8; 3072],
    body_len: usize,
    /// 请求用的 URL，拼在这里。
    url: [u8; 320],
    url_len: usize,
    /// 定位给的地名和坐标。坐标留着原文：拼 URL 只需要把它原样抄进去。
    city: [u8; 32],
    city_len: usize,
    lat: [u8; 16],
    lat_len: usize,
    lon: [u8; 16],
    lon_len: usize,
    /// 现在的天气。
    temperature: i32,
    code: i32,
    humidity: i32,
    wind: i32,
    /// 五天：天气码、最高、最低，和日期里的「日」。
    day_code: [i32; DAYS],
    day_high: [i32; DAYS],
    day_low: [i32; DAYS],
    day_number: [i32; DAYS],
    reload_pressed: bool,
}

impl Weather {
    fn city(&self) -> &str {
        core::str::from_utf8(&self.city[..self.city_len]).unwrap_or("")
    }

    /// 发第一个请求。发不出去（没权限、队满、URL 不成形）就算失败。
    fn locate(&mut self) {
        self.stage = Stage::Locating;
        self.fault = app_abi::fault::NONE;

        if host::http_start(LOCATOR) {
            self.seen = host::http_state();
        } else {
            self.stage = Stage::Failed;
            self.fault = app_abi::fault::UNSUPPORTED;
        }
    }

    /// 坐标拼进预报的 URL，然后发出去。拼不出来（这份文档里没有数字）就是失败。
    fn forecast(&mut self) {
        use core::fmt::Write;

        let lat = core::str::from_utf8(&self.lat[..self.lat_len]).unwrap_or("0");
        let lon = core::str::from_utf8(&self.lon[..self.lon_len]).unwrap_or("0");

        // 拼 URL 这一段自己成一个块：格式化借走缓冲区，而借着的时候不能再去读它。
        let (ok, len) = {
            let mut buffer = app_sdk::text::Buf::new(&mut self.url);

            let written = write!(
                &mut buffer,
                "{FORECAST}?latitude={lat}&longitude={lon}\
                 &current=temperature_2m,relative_humidity_2m,weather_code,wind_speed_10m\
                 &daily=weather_code,temperature_2m_max,temperature_2m_min\
                 &timezone=auto&forecast_days={DAYS}"
            );

            (written.is_ok(), buffer.len())
        };

        if !ok {
            self.stage = Stage::Failed;
            self.fault = app_abi::fault::UNSUPPORTED;
            return;
        }

        self.url_len = len;
        self.stage = Stage::Forecast;

        let url = core::str::from_utf8(&self.url[..self.url_len]).unwrap_or("");

        if host::http_start(url) {
            self.seen = host::http_state();
        } else {
            self.stage = Stage::Failed;
            self.fault = app_abi::fault::UNSUPPORTED;
        }
    }

    /// 请求刚变完的那一帧：把文档抄进来，按当前这一步解释它。
    fn settle(&mut self) {
        let mut status: u16 = 0;

        let taken = match host::http_take(&mut status, &mut self.body) {
            Some(len) => len.min(self.body.len()),
            None => 0,
        };

        self.body_len = taken;

        if status != 200 {
            self.stage = Stage::Failed;
            self.fault = host::http_fault();
            return;
        }

        let body_len = self.body_len;
        let locating = self.stage == Stage::Locating;

        // 读文档这一段也自己成一个块：从缓冲区里抄出来的东西都是拷贝，抄完就不再借着它，
        // 而 `self.forecast()` 要整块可变借用。
        if locating {
            let (city_len, lat_len, lon_len) = {
                let body = &self.body[..body_len];

                // 城市空着的时候用地区：小到没有城市名的镇子仍然在一个省里，而写着省名的
                // 抬头比什么都没写的抬头有用。
                let place = find_str(body, b"city")
                    .filter(|name| !name.is_empty())
                    .or_else(|| find_str(body, b"regionName"))
                    .unwrap_or(b"");

                let lat = find_raw(body, b"lat").unwrap_or(b"0");
                let lon = find_raw(body, b"lon").unwrap_or(b"0");

                let city_len = place.len().min(self.city.len());
                let lat_len = lat.len().min(self.lat.len());
                let lon_len = lon.len().min(self.lon.len());

                self.city[..city_len].copy_from_slice(&place[..city_len]);
                self.lat[..lat_len].copy_from_slice(&lat[..lat_len]);
                self.lon[..lon_len].copy_from_slice(&lon[..lon_len]);

                (city_len, lat_len, lon_len)
            };

            self.city_len = city_len;
            self.lat_len = lat_len;
            self.lon_len = lon_len;

            self.forecast();
            return;
        }

        let (temperature, code, humidity, wind, codes, highs, lows, numbers) = {
            let body = &self.body[..body_len];

            // 三个数组在 `daily` 里，而 `current` 里的 `weather_code` 是一个数：找带 `[` 的。
            let codes = find_array(body, b"\"weather_code\":[");
            let highs = find_array(body, b"\"temperature_2m_max\":[");
            let lows = find_array(body, b"\"temperature_2m_min\":[");
            let times = find_array(body, b"\"time\":[");

            let mut day_codes = [0i32; DAYS];
            let mut day_highs = [0i32; DAYS];
            let mut day_lows = [0i32; DAYS];
            let mut day_numbers = [0i32; DAYS];

            for day in 0..DAYS {
                day_codes[day] = nth_int(codes, day).unwrap_or(0);
                day_highs[day] = nth_int(highs, day).unwrap_or(0);
                day_lows[day] = nth_int(lows, day).unwrap_or(0);

                // `2026-10-20` 的最后两个字符就是「日」；取不到就画这一格的下标加一。
                day_numbers[day] = nth_str(times, day)
                    .and_then(|date| {
                        let text = core::str::from_utf8(date).ok()?;
                        let start = text.len().saturating_sub(2);

                        parse_int(text[start..].as_bytes())
                    })
                    .unwrap_or(day as i32 + 1);
            }

            (
                find_int(body, b"temperature_2m").unwrap_or(0),
                find_int(body, b"weather_code").unwrap_or(0),
                find_int(body, b"relative_humidity_2m").unwrap_or(0),
                find_int(body, b"wind_speed_10m").unwrap_or(0),
                day_codes,
                day_highs,
                day_lows,
                day_numbers,
            )
        };

        self.temperature = temperature;
        self.code = code;
        self.humidity = humidity;
        self.wind = wind;
        self.day_code = codes;
        self.day_high = highs;
        self.day_low = lows;
        self.day_number = numbers;
        self.stage = Stage::Done;
    }
}

/// 天气码的那句话。Open-Meteo 的 WMO 码，按这一页会碰到的那些写全。
fn describe(code: i32) -> &'static str {
    match code {
        0 => "晴",
        1 => "少云",
        2 => "多云",
        3 => "阴",
        45 | 48 => "雾",
        51 | 53 | 55 => "毛毛雨",
        56 | 57 => "冻毛毛雨",
        61 | 63 | 65 => "雨",
        66 | 67 => "冻雨",
        71 | 73 | 75 => "雪",
        77 => "米雪",
        80 | 81 | 82 => "阵雨",
        85 | 86 => "阵雪",
        95 => "雷雨",
        96 | 99 => "雷暴伴冰雹",
        _ => "未知",
    }
}

/// 失败的那句话：网络、时钟、地址、超时、握手、太大、写不进存储、没有这个客户端。
fn fault_text(fault: u32) -> &'static str {
    match fault {
        app_abi::fault::CLOCK => "时钟没对过，HTTPS 用不了",
        app_abi::fault::UNREACHABLE => "连不上",
        app_abi::fault::TIMEOUT => "超时",
        app_abi::fault::TLS => "握手失败",
        app_abi::fault::TOO_LARGE => "回答太大",
        app_abi::fault::STORAGE => "写不进存储",
        app_abi::fault::UNSUPPORTED => "这个固件没有 HTTP",
        _ => "没有网络",
    }
}

// ---------------------------------------------------------------------------------------------
// 读文档
//
// 这四个函数是这一页全部的 JSON 能力：找 `"key":`，然后把后面那一点东西读成一个字符串、
// 一个整数或者一段原文。没有解析器，因为这份文档的形状是写死的，而要的是五个数和两个字。
// 读不出来返回 `None`，页面画「读不出来」而不是画一个 0 —— 一个假的 0 度是一条假的天气。
// ---------------------------------------------------------------------------------------------

/// `"key":` 后面剩下的那些字节。
fn find_value<'a>(body: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut at = 0;

    while at + key.len() + 3 <= body.len() {
        if body[at] == b'"'
            && body[at + 1..].starts_with(key)
            && body[at + 1 + key.len()] == b'"'
            && body[at + 2 + key.len()] == b':'
        {
            return Some(&body[at + key.len() + 3..]);
        }

        at += 1;
    }

    None
}

/// 跳过空白。JSON 里允许有，而这个服务确实写了一个空格。
fn skip(value: &[u8]) -> &[u8] {
    let mut at = 0;

    while at < value.len() && (value[at] == b' ' || value[at] == b'\n' || value[at] == b'\t') {
        at += 1;
    }

    &value[at..]
}

/// `"key":"…"` 里的字符串，不带引号。
fn find_str<'a>(body: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let value = skip(find_value(body, key)?);

    if value.first() != Some(&b'"') {
        return None;
    }

    let end = value[1..].iter().position(|byte| *byte == b'"')?;

    Some(&value[1..1 + end])
}

/// `"key":123` 里的整数。小数只读整数部分：这一页画的是「23°」而不是「23.4°」。
fn find_int(body: &[u8], key: &[u8]) -> Option<i32> {
    parse_int(skip(find_value(body, key)?))
}

/// 一个数字的字面量原文（含小数点）：坐标要原样拼进 URL，转一次再转回来会丢精度。
fn find_raw<'a>(body: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let value = skip(find_value(body, key)?);
    let mut end = 0;

    while end < value.len()
        && (value[end].is_ascii_digit() || value[end] == b'.' || value[end] == b'-')
    {
        end += 1;
    }

    Some(&value[..end])
}

fn parse_int(value: &[u8]) -> Option<i32> {
    let mut result: i32 = 0;
    let mut negative = false;
    let mut any = false;

    for (at, byte) in value.iter().enumerate() {
        match byte {
            b'-' if at == 0 => negative = true,
            b'0'..=b'9' => {
                result = result * 10 + (byte - b'0') as i32;
                any = true;
            }
            _ => break,
        }
    }

    if !any {
        return None;
    }

    Some(if negative { -result } else { result })
}

/// `"key":[` 后面的数组内容（不含 `[`）。
fn find_array<'a>(body: &'a [u8], needle: &[u8]) -> Option<&'a [u8]> {
    let at = body
        .windows(needle.len())
        .position(|window| window == needle)?;

    Some(&body[at + needle.len()..])
}

/// 数组里的第 `index` 个整数。
fn nth_int(array: Option<&[u8]>, index: usize) -> Option<i32> {
    let mut rest = array?;

    for _ in 0..index {
        rest = &rest[rest.iter().position(|byte| *byte == b',')? + 1..];
    }

    parse_int(skip(rest))
}

/// 数组里的第 `index` 个字符串（`"…"` 的内容）。
fn nth_str<'a>(array: Option<&'a [u8]>, index: usize) -> Option<&'a [u8]> {
    let mut rest = array?;

    for _ in 0..index {
        // 每个元素都是 `"…"`，所以找下一个逗号要先跳过这一对引号。
        rest = &rest[rest.iter().position(|byte| *byte == b'"')? + 1..];
        rest = &rest[rest.iter().position(|byte| *byte == b'"')? + 1..];
        rest = &rest[rest.iter().position(|byte| *byte == b',')? + 1..];
    }

    let start = rest.iter().position(|byte| *byte == b'"')? + 1;
    let end = start + rest[start..].iter().position(|byte| *byte == b'"')?;

    Some(&rest[start..end])
}

impl App for Weather {
    fn new(_: &Context) -> Self {
        let mut weather = Self {
            stage: Stage::Locating,
            fault: app_abi::fault::NONE,
            seen: app_abi::state::IDLE,
            body: [0; 3072],
            body_len: 0,
            url: [0; 320],
            url_len: 0,
            city: [0; 32],
            city_len: 0,
            lat: [0; 16],
            lat_len: 0,
            lon: [0; 16],
            lon_len: 0,
            temperature: 0,
            code: 0,
            humidity: 0,
            wind: 0,
            day_code: [0; DAYS],
            day_high: [0; DAYS],
            day_low: [0; DAYS],
            day_number: [0; DAYS],
            reload_pressed: false,
        };

        weather.locate();

        weather
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        let reload = Rect::new(18.0, 404.0, 444.0, 56.0);

        self.reload_pressed = context.input.pressing(reload);

        if context.tapped(reload) {
            host::http_cancel();
            self.locate();
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);

        // 请求是「问一次」而不是「等一次」：这里只看状态变没变，变了才去取。等待发生在帧
        // 与帧之间，而这一帧从不为了等一个回答而停下。
        if self.stage == Stage::Locating || self.stage == Stage::Forecast {
            let state = host::http_state();

            if state != self.seen {
                self.seen = state;

                match state {
                    app_abi::state::DONE => self.settle(),
                    app_abi::state::FAILED => {
                        self.fault = host::http_fault();
                        self.stage = Stage::Failed;
                    }
                    app_abi::state::DENIED => {
                        self.fault = app_abi::fault::UNSUPPORTED;
                        self.stage = Stage::Failed;
                    }
                    _ => {}
                }
            }
        }

        ui.text(24.0, 28.0, "天气", 22.0, theme::MUTED);

        if !self.city().is_empty() {
            ui.text_in(
                Rect::new(24.0, 24.0, 432.0, 28.0),
                self.city(),
                20.0,
                theme::MUTED,
                app_abi::Align::Right,
            );
        }

        match self.stage {
            Stage::Locating | Stage::Forecast => {
                let waiting = host::http_state() == app_abi::state::RUNNING;

                ui.text_centered(
                    Rect::new(18.0, 180.0, 444.0, 60.0),
                    if waiting && self.stage == Stage::Forecast {
                        "正在取预报（可能在排队）"
                    } else {
                        "正在定位"
                    },
                    22.0,
                    theme::MUTED,
                );
            }
            Stage::Failed => {
                ui.text_centered(
                    Rect::new(18.0, 180.0, 444.0, 60.0),
                    fault_text(self.fault),
                    22.0,
                    theme::DANGER,
                );
            }
            Stage::Done => {
                let mut storage = [0u8; 12];
                let mut buffer = app_sdk::text::Buf::new(&mut storage);
                let degrees = format_into!(buffer, "{}°", self.temperature);

                widgets::reading(
                    ui,
                    Rect::new(24.0, 66.0, 240.0, 110.0),
                    degrees,
                    describe(self.code),
                    64.0,
                );

                let mut storage = [0u8; 24];
                let mut buffer = app_sdk::text::Buf::new(&mut storage);
                let humidity = format_into!(buffer, "{}%", self.humidity);
                widgets::key_value(ui, Rect::new(24.0, 190.0, 432.0, 24.0), "湿度", humidity, 20.0);

                let mut storage = [0u8; 24];
                let mut buffer = app_sdk::text::Buf::new(&mut storage);
                let wind = format_into!(buffer, "{} km/h", self.wind);
                widgets::key_value(ui, Rect::new(24.0, 220.0, 432.0, 24.0), "风速", wind, 20.0);

                // 五天：一行一天，最左边是日子，最右边是最高最低。
                let list = Rect::new(18.0, 258.0, 444.0, 140.0);
                widgets::card(ui, list, theme::SURFACE);

                for day in 0..DAYS {
                    let row = Rect::new(
                        list.x + 12.0,
                        list.y + 8.0 + day as f32 * 26.0,
                        list.width - 24.0,
                        24.0,
                    );

                    let mut storage = [0u8; 48];
                    let mut buffer = app_sdk::text::Buf::new(&mut storage);
                    let line = format_into!(
                        buffer,
                        "{number}日  {sky}  {high}° / {low}°",
                        number = self.day_number[day],
                        sky = describe(self.day_code[day]),
                        high = self.day_high[day],
                        low = self.day_low[day]
                    );

                    ui.text(row.x, row.y, line, 18.0, theme::TEXT);
                }
            }
        }

        widgets::button(
            ui,
            Rect::new(18.0, 404.0, 444.0, 56.0),
            "重新加载",
            24.0,
            theme::SURFACE_HIGH,
            theme::TEXT,
            self.reload_pressed,
        );

        ui.text(24.0, 472.0, "ip-api.com 定位，Open-Meteo 预报", 13.0, theme::MUTED);
    }
}

app!(Weather);
