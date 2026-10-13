//! The forecast for where this box is: two requests, and a reading of each of them.
//!
//! One screen, and it is a drawing of the last request's answer. Where this box *is* comes from a
//! locator service and what the weather there is comes from Open-Meteo: the page asks the first, reads
//! a place out of the answer, and asks the second about that place — which is the whole difference
//! between a forecast for the reader and a forecast for whichever city this file used to name. Both
//! requests go out when the page opens and again when somebody presses the button; everything after
//! that is reading [`Weather::status`].
//!
//! # The request is asynchronous, and this page is where that shows
//!
//! [`pomelo_hal::HttpBackend`] has no blocking call: a request is `start`, then `state` until it is
//! `Done`, then `take_result`. That is not a detail of the client — a request is a second of TLS
//! handshakes and keep-alives, and a page that waited for it would be a page that stopped drawing. So
//! a request is polled: [`Message::Tick`] asks how it is going, and a frame is asked for *only* while
//! there is an answer still to come. See [`Weather::subscription`].
//!
//! # Three kinds of not-having-a-forecast, drawn as two
//!
//! The network's faults are [`HttpFault`] — its own closed list, straight from the backend. The
//! document's are [`api::Fault`], found by reading the answer. And [`Status::Unreadable`] is the page's
//! own judgement that whatever arrived was not a forecast.
//!
//! The first two are drawn differently, because they need different things from a person: the network's
//! faults are the list [`Words::fault`] gives a sentence and a thing to do for, and a document that
//! could not be read is one line saying so. Everything else the page might have wanted to say — a
//! `404`, a captive portal's sign-in page, a body that was not text at all — is that same one line,
//! because it is the same thing to do about it: try again in a while.
//!
//! # What this page does *not* do
//!
//! It does not cache, and it does not refresh itself on a timer. A forecast is a couple of kilobytes
//! from somebody else's server, this box is on a shelf, and a page that asked every quarter of an hour
//! would be a page spending a battery to draw the same number. Somebody presses the button.

mod api;
mod json;
pub mod style;
#[cfg(test)]
mod tests;
mod ui;

use std::sync::Arc;
use std::time::Instant;

use iced::theme::Palette;
use iced::{Element, Subscription, Theme};

use pomelo_hal::{Board, HalError, HttpFault, HttpState};
use pomelo_widgets::preferences::{Language, SystemPreferences, ThemeMode};

pub use api::{forecast_url, parse, Condition, Current, Day, Fault, Forecast, Location};
pub use style::Sizes;

/// What the page reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// A frame, which is the clock the request's progress is read on.
    Tick(Instant),
    /// The refresh button: ask the service again.
    Refresh,
}

/// What the page has to draw.
///
/// Four states rather than a `Forecast` and a couple of flags: a page can be waiting, or have a
/// forecast, or have a fault from the network, or have something that was not a forecast at all — and
/// there is no fifth combination of those. The reading and the reason it is missing travel together in
/// the same value because they are the same thing: what the page shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// A request is on its way. Nothing to draw yet.
    Loading,
    /// The service's answer, as the page reads it.
    Ready(Forecast),
    /// The request did not come back, and this is why.
    Failed(HttpFault),
    /// Something answered, and it was not a forecast: a refusal with a body in it, a page that is not
    /// JSON, a document missing the fields this page needs, or bytes that are not text.
    ///
    /// The four are one state because they are one thing to a person: the page does not know what the
    /// weather is, and asking again later is the whole of the response.
    Unreadable,
}

/// Which of the two requests on the way to a forecast is the one in flight.
///
/// The page asks two services in turn — where this box is, then what the weather there is — through one
/// client, which holds one request at a time. Nothing about the arriving bytes says which question they
/// answer, so this is the record of what was asked: without it a locator's document and a forecast's
/// would be two documents of the same shape arriving at the same function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Asking the locator where this box is.
    Locating,
    /// Asking the forecast service about the place the locator named.
    Forecasting,
}

/// The forecast page.
pub struct Weather {
    board: Arc<Board>,
    /// What the page shows, which is also what it is waiting for.
    pub(crate) status: Status,
    /// Where the forecast is for. Nowhere until the locator has named somewhere.
    pub(crate) location: Location,
    /// Which of the two answers [`Weather::status`] is waiting for.
    pub(crate) step: Step,
    pub(crate) preferences: SystemPreferences,
}

impl Weather {
    /// The page, asking the service for a forecast.
    ///
    /// The question is asked here rather than left to the first frame, for the reason the Wi-Fi page
    /// scans in its own constructor: the page's whole subject is the forecast, and a page that opened
    /// on a fault until something asked would be a page that looked broken for a second.
    pub fn new(board: Arc<Board>) -> Self {
        let mut app = Self {
            board,
            // Overwritten by the `refresh` below, and there so that the field is never a state the
            // page cannot be in. A constructor that could not ask would leave this in place, and a
            // card saying "loading" forever is the honest drawing of a page that never got to ask.
            status: Status::Loading,
            location: Location::default(),
            step: Step::Locating,
            preferences: SystemPreferences::default(),
        };

        app.refresh();

        app
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// Where the forecast is for.
    pub fn location(&self) -> &Location {
        &self.location
    }

    /// What the page has to draw.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The forecast, if there is one.
    pub fn forecast(&self) -> Option<&Forecast> {
        match &self.status {
            Status::Ready(forecast) => Some(forecast),
            _ => None,
        }
    }

    /// Whether the page is still waiting for something.
    ///
    /// The one question [`Weather::subscription`] is asked, and it is the whole of the page's frame
    /// budget: a card of numbers that are not changing is not a moving picture, so nothing asks for
    /// frames once the answer has been read.
    pub fn waiting(&self) -> bool {
        matches!(self.status, Status::Loading)
    }

    /// Handles a message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tick(_) => self.pull(),
            Message::Refresh => self.refresh(),
        }
    }

    /// Asks the locator where this box is.
    ///
    /// The URL is built here and the client is handed the string: what a request *is*, in this HAL, is
    /// its URL — see [`pomelo_hal::HttpBackend`] — so the question and the asking are one line apart.
    ///
    /// This is the first of the page's two requests, and the only one a refresh starts: the forecast is
    /// asked for in [`Weather::located`], the moment there is a place to ask about. Two requests out of
    /// one button, then, and deliberately not two at once — the client holds one request at a time, and
    /// the second request's URL is the first one's answer.
    pub fn refresh(&mut self) {
        self.step = Step::Locating;
        self.ask(api::LOCATOR);
    }

    /// Hands one URL to the client, and reads a refusal as the state it is.
    fn ask(&mut self, url: &str) {
        match self.board.http().start(url) {
            // `Busy` is the state seen from the other side: it means a request *is* on its way, which
            // is exactly what `Loading` says. The two are one arm because they are one situation, and
            // the page has to go on asking for frames in both — stopping here would leave an answer
            // nobody ever collects.
            Ok(()) | Err(HalError::Busy) => self.status = Status::Loading,
            // Not `Busy` and not `Ok`: the client refused the URL this page built. That is not a
            // network event and no amount of trying will change it — the URL is the page's own string,
            // so a backend that will not take it is a board whose image has no HTTP client in it. The
            // fault is [`HttpFault::Unsupported`] because that is the one a person can do nothing
            // about, which is the honest thing to say about a page that cannot ask at all.
            Err(_) => self.status = Status::Failed(HttpFault::Unsupported),
        }
    }

    /// Reads what has arrived, and nothing else.
    ///
    /// No `collected` flag, unlike the Wi-Fi page: `take_result` *takes*, so the state that said an
    /// answer was there is gone the moment it is read and this cannot run twice for one request. A
    /// frame on which nothing has changed does nothing at all.
    fn pull(&mut self) {
        // The state is read into a local *before* the match, and that is not style: `http()` hands out
        // a guard on the board's client, and a guard taken in a match's scrutinee lives until the end
        // of the match — so calling `take_result` inside an arm would lock a mutex this function is
        // already holding, and wait for itself forever.
        let state = self.board.http().state();

        match state {
            // Which reading the answer gets is which question was asked — and, for the locator, it is
            // also what decides whether there is a second request to make at all.
            HttpState::Done => match self.step {
                Step::Locating => self.located(),
                Step::Forecasting => self.forecasted(),
            },
            HttpState::Failed(fault) => self.status = Status::Failed(fault),
            // `Running` is this page's own waiting, already drawn as [`Status::Loading`], and `Idle`
            // is where a taken answer leaves the client: neither is news.
            HttpState::Idle | HttpState::Running => {}
        }
    }

    /// Reads the locator's answer, and asks for the forecast the moment it names a place.
    ///
    /// The two requests are one refresh: a page that stopped in between would be a page waiting for
    /// somebody to press the button a second time, which is not what the button said it would do.
    fn located(&mut self) {
        // Taken out of the client before anything is asked of it again, and for the reason `pull`
        // gives: a guard on the client lives as long as the expression that took it, and the arm
        // below takes another one.
        let answer = self.board.http().take_result();

        let place = match answer {
            Ok(response) if response.ok() => match response.text() {
                Some(body) => api::parse_location(body).ok(),
                None => None,
            },
            _ => None,
        };

        match place {
            Some(location) => {
                self.location = location;
                self.step = Step::Forecasting;
                let url = api::forecast_url(&self.location);
                self.ask(&url);
            }
            // No place, and so no forecast to ask about: this page will not invent one to have
            // something to draw. [`Status::Unreadable`] rather than a network fault because the network
            // did its job — something answered — and what it answered was not a place.
            None => self.status = Status::Unreadable,
        }
    }

    /// Reads the forecast's answer, which is the last of a refresh's two requests.
    ///
    /// No `collected` flag, unlike the Wi-Fi page: `take_result` *takes*, so the state that said an
    /// answer was there is gone the moment it is read and this cannot run twice for one request. A
    /// frame on which nothing has changed does nothing at all.
    fn forecasted(&mut self) {
        self.status = match self.board.http().take_result() {
            Ok(response) if response.ok() => match response.text() {
                Some(body) => match api::parse(body) {
                    Ok(forecast) => Status::Ready(forecast),
                    // A document this page could not read: the service answering something
                    // else, or answering this in a shape it has since changed.
                    Err(_) => Status::Unreadable,
                },
                // A body that was not text. The bytes arrived and nothing can be made of them.
                None => Status::Unreadable,
            },
            // A refusal — the status is not one meaning "here is the answer" — or nothing at
            // all to take. The first is a server saying no, which is a thing to try again
            // about rather than a fault of the network's.
            _ => Status::Unreadable,
        };
    }

    /// Frames, and only while this page is waiting for one. See [`Weather::waiting`].
    ///
    /// On the board this is never asked for: the launcher merges the subscriptions of the apps it is
    /// actually running, and its own pump is what sends [`Message::Tick`] here. A window has no such
    /// pump, so the app takes the frames it is offered — while there is a reason to.
    pub fn subscription(&self) -> Subscription<Message> {
        if self.waiting() {
            iced::window::frames().map(Message::Tick)
        } else {
            Subscription::none()
        }
    }

    /// The theme, by the system's preference.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    background: style::surface(ThemeMode::Dark),
                    ..Palette::DARK
                },
            ),
            ThemeMode::Light => Theme::custom(
                "PomeloLight",
                Palette {
                    background: style::surface(ThemeMode::Light),
                    ..Palette::LIGHT
                },
            ),
        }
    }

    /// The page.
    pub fn view(&self) -> Element<'_, Message> {
        ui::page(self)
    }
}

/// The words on the page, in the language the system is set to.
///
/// One struct with both languages in it — the shape the rest of this project's pages use — so that a
/// page never has a sentence with a language of its own, and adding a word is adding a field the
/// compiler will insist be filled in twice.
pub(crate) struct Words {
    pub(crate) language: Language,
    pub(crate) title: &'static str,
    pub(crate) refresh: &'static str,
    pub(crate) loading: &'static str,
    pub(crate) now: &'static str,
    pub(crate) days: &'static str,
    pub(crate) today: &'static str,
    pub(crate) tomorrow: &'static str,
    pub(crate) humidity: &'static str,
    pub(crate) wind: &'static str,
    pub(crate) source: &'static str,
    pub(crate) unreadable: &'static str,
    pub(crate) unreadable_hint: &'static str,
}

impl Words {
    /// The page's words, for `language`.
    pub(crate) fn of(language: Language) -> Self {
        match language {
            Language::Chinese => Self {
                language,
                title: "天气",
                refresh: "刷新",
                loading: "正在获取…",
                now: "现在",
                days: "未来几天",
                today: "今天",
                tomorrow: "明天",
                humidity: "湿度",
                wind: "风速",
                source: "数据来自 Open-Meteo",
                unreadable: "收到的不是天气预报",
                unreadable_hint: "稍后再试",
            },
            Language::English => Self {
                language,
                title: "Weather",
                refresh: "Refresh",
                loading: "Fetching…",
                now: "Now",
                days: "The days ahead",
                today: "Today",
                tomorrow: "Tomorrow",
                humidity: "Humidity",
                wind: "Wind",
                source: "From Open-Meteo",
                unreadable: "That was not a forecast",
                unreadable_hint: "Try again in a while",
            },
        }
    }

    /// The condition's name, which the service reports as a number.
    pub(crate) fn condition(&self, condition: Condition) -> &'static str {
        match (self.language, condition) {
            (Language::Chinese, Condition::Clear) => "晴",
            (Language::Chinese, Condition::PartlyCloudy) => "多云",
            (Language::Chinese, Condition::Overcast) => "阴",
            (Language::Chinese, Condition::Fog) => "雾",
            (Language::Chinese, Condition::Drizzle) => "毛毛雨",
            (Language::Chinese, Condition::FreezingRain) => "冻雨",
            (Language::Chinese, Condition::Rain) => "雨",
            (Language::Chinese, Condition::Snow) => "雪",
            (Language::Chinese, Condition::Showers) => "阵雨",
            (Language::Chinese, Condition::Thunderstorm) => "雷雨",
            // The code was one this page does not know, and saying so is the point: a page that drew
            // an unknown sky as `Clear` would be answering a question it was not told the answer to.
            (Language::Chinese, Condition::Unknown) => "未知",

            (_, Condition::Clear) => "Clear",
            (_, Condition::PartlyCloudy) => "Partly cloudy",
            (_, Condition::Overcast) => "Overcast",
            (_, Condition::Fog) => "Fog",
            (_, Condition::Drizzle) => "Drizzle",
            (_, Condition::FreezingRain) => "Freezing rain",
            (_, Condition::Rain) => "Rain",
            (_, Condition::Snow) => "Snow",
            (_, Condition::Showers) => "Showers",
            (_, Condition::Thunderstorm) => "Thunderstorm",
            (_, Condition::Unknown) => "Unknown",
        }
    }

    /// What went wrong with the request, as one line.
    ///
    /// One per [`HttpFault`], because each of them is a different thing that happened — and the match
    /// is exhaustive and has no `_`, so a fault added to the HAL stops this page compiling rather than
    /// reaching a person as a blank card.
    pub(crate) fn fault(&self, fault: HttpFault) -> &'static str {
        match (self.language, fault) {
            (Language::Chinese, HttpFault::NoNetwork) => "没有连接到网络",
            (Language::Chinese, HttpFault::ClockNotSet) => "时钟还没有校准",
            (Language::Chinese, HttpFault::Unreachable) => "没有服务器回应",
            (Language::Chinese, HttpFault::Timeout) => "请求超时",
            (Language::Chinese, HttpFault::Tls) => "安全连接失败",
            (Language::Chinese, HttpFault::TooLarge) => "返回内容超出上限",
            (Language::Chinese, HttpFault::Unsupported) => "这台设备无法发起请求",

            (_, HttpFault::NoNetwork) => "Not on a network",
            (_, HttpFault::ClockNotSet) => "The clock has never been set",
            (_, HttpFault::Unreachable) => "Nothing answered",
            (_, HttpFault::Timeout) => "The request timed out",
            (_, HttpFault::Tls) => "The secure connection failed",
            (_, HttpFault::TooLarge) => "The answer was larger than this board holds",
            (_, HttpFault::Unsupported) => "This board cannot make requests",
        }
    }

    /// What to do about it, as one line — and it is a *second* line because a person needs both.
    ///
    /// The pair is the whole point of the page drawing its faults rather than saying "failed": a fault
    /// line that does not say what to do sends somebody to the network settings for a clock problem.
    pub(crate) fn fault_hint(&self, fault: HttpFault) -> &'static str {
        match (self.language, fault) {
            (Language::Chinese, HttpFault::NoNetwork) => "请在设置里打开 Wi-Fi",
            (Language::Chinese, HttpFault::ClockNotSet) => "先校准时间，再试一次",
            (Language::Chinese, HttpFault::Unreachable) => "检查网络后重试",
            (Language::Chinese, HttpFault::Timeout) => "稍后重试",
            (Language::Chinese, HttpFault::Tls) => "稍后重试",
            (Language::Chinese, HttpFault::TooLarge) => "稍后重试",
            (Language::Chinese, HttpFault::Unsupported) => "这台设备上无解",
            (_, HttpFault::NoNetwork) => "Turn the Wi-Fi on in Settings",
            (_, HttpFault::ClockNotSet) => "Set the clock, then try again",
            (_, HttpFault::Unreachable) => "Check the network, then try again",
            (_, HttpFault::Timeout) => "Try again in a while",
            (_, HttpFault::Tls) => "Try again in a while",
            (_, HttpFault::TooLarge) => "Try again in a while",
            (_, HttpFault::Unsupported) => "Nothing to be done about it here",
        }
    }
}
