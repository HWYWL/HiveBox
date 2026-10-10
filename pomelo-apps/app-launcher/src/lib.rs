//! The launcher, built from iced widgets — **a standard iced program**.
//!
//! Deliberately free of raster images: iced's `image` feature pulls the `image` crate into the
//! firmware, and the wallpaper and icons we already bake are RGB565 blits that `pomelo-gfx` does
//! natively. So the wallpaper is a gradient and every picture on the screen is a glyph out of
//! `pomelo_material_symbols` — the grid's on an accent square, the status bar's on its black band —
//! one font of the whole Material Symbols catalogue, which costs nothing beyond the text this
//! launcher draws anyway.
//!
//! # Hosting an app is merging its subscription
//!
//! The five apps are hosted as **widgets**: this launcher calls their `view` and `update` itself, so
//! their state is its state. What a widget cannot carry is an app's *subscriptions* — the one that
//! runs a clock (`music-player`) and the one whose *layout* follows the screen (`terminal`) both
//! express that as a `Subscription`, and a subscription belongs to whoever owns the loop.
//!
//! So the launcher, which owns the loop for its children as far as they are concerned, says in
//! [`Launcher::subscription`] which of them is alive: the one on screen, and nothing else. That is
//! the declarative form of "only the app you are looking at animates" — off screen, an app is not
//! subscribed, so it costs no frames at all.
//!
//! # The screen's size is the platform's to know
//!
//! Nothing here may assume what size it is drawing into: the grid's pages, the pager's thresholds
//! and the terminal's band are all the screen's size, and the screen is a window on a desktop and a
//! panel on the board. So the launcher subscribes to `window::resize_events()` *always* — it is an
//! event and not a clock, so it costs no frames while nothing changes — and hands what it hears to
//! the hosted app whose layout depends on it ([`Launcher::hand_over_size`]). An app opened later was
//! not on screen when the platform announced the size at boot, which is why the hand-off exists at
//! all.
//!
//! # Where the board comes from
//!
//! [`program`] takes the board its apps share, and that board is the composition root's: the
//! firmware injects the ESP32-S3 one, this project's `main.rs` the HAL's desktop simulator. Nothing
//! here decides which hardware this is — the same argument as the music player's.
//!
//! # The grid is paged, and the page slides under the finger
//!
//! A page is [`PER_PAGE`] apps, one to a quadrant, so the five the catalogue has are two pages: four
//! and one.
//! A finger turning one is a **drag**: down on the grid, across, up. iced has no widget for that — a
//! `button` captures the press it is given, and `Scrollable` scrolls for a wheel, a touch or its own
//! scrollbar and for nothing else — so the gesture is read by `pomelo-widgets`' pager.
//!
//! While the finger is down the pages follow it: the next page comes in as the current one leaves,
//! both drawn in the same frame, and a drag past the first or the last page meets a resistance
//! instead of empty space — the edge says there is nothing beyond it. The finger leaving *is* the
//! decision: past halfway, or a flick faster than a slow drag, commits the turn and the page settles
//! onto it over [`style::PAGE_SETTLE`]; short of that it settles back where it was.
//!
//! The pager owns all of that, including the frames it needs while settling (one `request_redraw`
//! per step, which `Host` turns into the next frame). What the launcher owns is the page it is on:
//! the index it hands the pager, and the [`Message::PageChanged`] it is told about when a turn
//! commits.
//!
//! # An app arrives and leaves, rather than appearing
//!
//! The desktop is what an app is drawn *over*, and the two moments that matter are the ones a phone
//! spends showing rather than cutting: the app slides in from the right when a tile is tapped, off the
//! right when back is asked for, and off the top when a finger comes up from the foot of the panel.
//! None of those is the screen changing. The screen changes when the gesture *decides* — on the tap,
//! on the key press, or on the finger leaving — and what happens after that is the app being *drawn*
//! over a desktop that is already up. See [`Launcher::view`]: one frame, two screens, and
//! [`pomelo_widgets::transition`] is what puts them both in it.
//!
//! # Everything that can move follows the finger
//!
//! Which is the difference between this and a screen that waits to be told. A drag in from the left
//! edge takes the app with it, frame by frame; a drag up from the foot of the panel puts it aside the
//! same way; and a drag down from the head of the panel brings the task switcher down over whatever is
//! up — or takes it away again when the finger goes back the other way.
//!
//! Halfway through a drag nothing has been decided: the app is still the screen and still running, the
//! sheet is still the sheet, and both are drawn exactly where the finger has taken them. What settles
//! it is the finger leaving — pushed far enough ([`style::TRANSITION_COMMIT`]) or fast enough
//! ([`style::TRANSITION_FLING`]) and the layer goes to the end it was pushed towards, and anything else
//! is it coming back to where it came from. One rule, four gestures: an app off the panel and an app
//! put aside, a sheet pulled down and a sheet pushed back up. See [`Launcher::pull_layer`] and
//! [`Launcher::release_layer`].
//!
//! The pinning that makes the edge layer possible at all is what makes this safe: only a drag that
//! *began* in an edge band is ever reported to this app, so a list in the middle of a page goes on
//! scrolling under the same finger. See [`Launcher::edge_gestures`].
//!
//! Where the app's state lives while it leaves is what makes this a transition rather than a cut
//! with a picture of the old screen in it: the app never went anywhere. Its pages, its scroll and
//! its place in `running_apps` are still this launcher's, and the slide is drawn from *that* — by
//! the same function that would draw the app if it were opened again. [`Launcher::app_screen`].
//!
//! And nothing else in the launcher changes while a screen moves, which is deliberate: the screen
//! changes on the frame the gesture decides, not on the frame the app has finished moving, so the
//! status bar, the exit key, the switcher and a press that lands in the middle of a slide all get the
//! answer a phone would give. What is in flight is a picture, and a picture is not a state machine.
//!
//! # The task switcher is a layer, and the top edge is where it comes from
//!
//! A drag down from the head of the panel brings [`recents`] over whatever is up: what is running,
//! newest first, with a cross on each card and one control that stops all of them. It is a layer and
//! not a screen — the apps behind it are still running, which is the whole of what it is *about* —
//! so back, the swipe up from the foot, and a tap on the wash all put it away, and the wash sits
//! under the cards rather than over them.
//!
//! That gesture is pinned to the top edge, and the pin is the reason the edge mechanism exists:
//! downwards is the one direction a page on this board is most likely to want for itself, because
//! every list here scrolls. See [`Launcher::edge_gestures`].
//!
//! Key layout notes:
//!
//! * a page evenly distributes apps across rows and columns using flexible spaces;
//! * only the app's icon is the pressable touch target, leaving surrounding spaces for pager swipe gestures.

mod apps;
mod recents;
mod status;
mod style;

use std::sync::Arc;

use iced::theme::Palette;
use iced::widget::{button, column, container, stack, text, Column, Row, Space};
use iced::{
    Alignment, Border, Color, Element, Length, Shadow, Size, Subscription, Task, Theme, Vector,
};

use calculator::Calculator;
use music_player::Player;
use pomelo_hal::Board;
use settings::Settings;
use terminal::Terminal;
use web_manager::WebManager;

pub use apps::{Entry, CATALOGUE};
pub use pomelo_material_symbols::Icon;
pub use pomelo_widgets::{AppIcon, BitmapIcon, FontSizeTier, Language, SystemPreferences, ThemeMode};
pub use style::{
    DOT_REST, DOT_UP, LABEL, PER_PAGE, SCREEN, STATUS_BG, STATUS_BG_DARK, STATUS_BG_LIGHT,
    STATUS_HEIGHT, STATUS_INSET,
};

/// The launcher as an iced program, with `board` for its apps.
///
/// The result is iced's own `Application`, which is both a builder — `main.rs` calls `run()` on it —
/// and a [`Program`](iced::Program), so the platform can run it directly (`app_launcher::program(board).run()`).
/// One definition, the same wiring in both places: the subject of the launcher is not a *shape* of
/// program, it is these four functions.
///
/// Text draws with the platform's default font — a Simplified-Chinese subset of Source Han Sans, installed by the
/// host unless an app installs one of its own first; see `pomelo_iced_host::fonts`. The icon font is
/// a second face on top of that, and it is installed here, through iced's own channel: the settings
/// travel with the program, so the desktop window and the board's loop both get it, and neither
/// needs to know that a widget wanted it.
pub fn program(
    board: Arc<Board>,
) -> iced::Application<impl iced::Program<State = Launcher, Message = Message, Theme = Theme>> {
    iced::application(
        move || Launcher::new(Arc::clone(&board)),
        Launcher::update,
        Launcher::view,
    )
    .font(pomelo_material_symbols::FONT)
    .theme(Launcher::theme)
    .subscription(Launcher::subscription)
}

/// Which screen is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Grid,
    App(usize),
}

/// The entries in [`CATALOGUE`] that have an iced app behind them.
///
/// Indices, because that is what the grid hands over when a tile is tapped. The tests assert that
/// each of these still names the app it says it does, and every entry has one: the catalogue and
/// the list of apps are the same five things, in the same order.
pub const MUSIC: usize = 0;
pub const WEB_MANAGER: usize = 1;
pub const TERMINAL: usize = 2;
pub const SETTINGS: usize = 3;
pub const CALCULATOR: usize = 4;

/// What the launcher reacts to.
///
/// No longer `Eq`: the terminal's own `Message` carries a `Size`, and a size is a pair of floats.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// An icon was tapped.
    Open(usize),
    /// The paged grid turned to `page`.
    PageChanged(usize),
    /// The back button, or hardware button 1 (moves to background).
    Back,
    /// The bottom edge was swiped up: back to the desktop, leaving the app running.
    ///
    /// Its own message and not [`Message::Back`], because the two differ exactly where it matters:
    /// back asks the app first — the settings list takes it to leave a sub-page — while a swipe up
    /// from the foot of the panel says which of the two things the finger meant.
    Home,
    /// The exit / kill button, or hardware button 2 (kills app and frees memory).
    Exit,
    /// A tap on the wash under the switcher's cards: put the sheet away, stopping nothing.
    ///
    /// The one way the sheet is left that is not a gesture of its own, and it is answered the same way
    /// the drags are: the sheet goes back up under its own frames. Nothing it lists is stopped — the
    /// crosses on the cards and "clear all" are the two things that stop anything.
    RecentsClose,
    /// The cross on one of the switcher's cards: stop that app.
    RecentsKill(usize),
    /// The switcher's "clear all": stop every app it lists.
    RecentsClearAll,
    /// A transition reported itself over: the app that was on its way has arrived, or has gone.
    ///
    /// The one message here that comes from a widget rather than from a finger or a key
    /// (`pomelo_widgets::ScreenTransition` publishes it on the frame the app stops moving), and it
    /// is what ends the transition — until it arrives, the app is still drawn over the desktop.
    TransitionSettled,
    /// A finger the edge layer claimed, reported once a frame: how far it has taken the app from
    /// where the app was.
    ///
    /// A *drag* and not a swipe, because the app is what moves: what the finger has done so far is
    /// where the app is drawn, and what it does next is where the app goes next. The message carries
    /// the distance from where the finger came down, which is the same number every frame and grows —
    /// see [`Launcher::pull_app`].
    Pulled(Vector),
    /// The finger left the panel, at the end of a drag: how fast it was moving when it left.
    ///
    /// Speed is half of what decides where an app ends up — a short flick is a decision as much as a
    /// long pull — and this is the velocity of the whole drag rather than of its last frame, which is
    /// what a flick is. See [`Launcher::release_app`].
    Released(Vector),
    /// The status bar's readings, driven by the [`Subscription`].
    ///
    /// Produced periodically by [`status_stream`] and whenever underlying
    /// hardware events occur (battery level, charging state, Wi-Fi status changes, clock minute updates).
    Status(String, u8, bool, u8),
    /// A second has passed on the board ([`pomelo_hal::SystemEvent::Tick`]).
    ///
    /// The launcher has no clock of its own to advance: the status bar's minute arrives with the
    /// battery push. This exists for the one hosted app that shows a *running* time — the settings
    /// list's readout — which is told to look at the board again rather than being handed a value
    /// to draw. Nothing else here reacts, and an app that was never opened makes this one match arm.
    Tick,
    /// The screen changed size: a window on a desktop, the panel on the board.
    Resized(Size),
    /// A message from one of the apps this launcher hosts.
    Calculator(calculator::Message),
    Settings(settings::Message),
    Music(music_player::Message),
    Terminal(terminal::Message),
    WebManager(web_manager::Message),
}

/// The layer over the desktop that is moving, and which way it is going.
///
/// The *resting state* is not part of this, and that is the point: a layer that is arriving is already
/// the thing it is arriving over — an app is [`Screen::App`], the switcher is `recents` — and one that
/// is leaving has already given way. Either state changes on the frame the *gesture* decides, not on
/// the frame the layer has finished moving. What is left to do after that is *show* it: the layer is
/// drawn over the desktop for as long as it takes to slide off it, by
/// [`pomelo_widgets::transition`].
///
/// While a finger is still on it, nothing has been decided at all: the layer is drawn where the finger
/// has put it, and what is behind it is only what the drag is uncovering.
///
/// Which is why a layer is remembered by name rather than by element: an app's state never left this
/// launcher — the same reason a backgrounded app comes back on the page it was on — and the view
/// builds whatever it draws out of that. See [`Launcher::view`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Transition {
    /// What is moving over the desktop.
    layer: Layer,
    motion: pomelo_widgets::Motion,
    /// Where it was when the gesture reached it, in `0.0..=1.0`.
    ///
    /// An app rests on the panel; the switcher rests *off* it or *over* it, depending on whether it
    /// was up when the finger arrived. It is where a release falls back to, and what tells a gesture
    /// that meant it from one that changed its mind. See [`Launcher::release_layer`].
    start: f32,
    /// Where the layer is now, in the launcher's own copy of it: where a finger has pulled it to while
    /// one is on it, or the place it was sent to when the finger left.
    ///
    /// Kept here rather than asked back out of the widget, because the widget decides nothing and is
    /// only ever told where to draw. One answer to "where is it" is worth more than a widget that can
    /// be asked, and it is what makes the finger and the animation unable to disagree.
    progress: pomelo_widgets::Progress,
}

/// The two things that are drawn *over* the desktop, and can be taken off it by a finger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    /// One of the apps, by the index the grid hands over.
    App(usize),
    /// The task switcher: a sheet over whatever is up, which is what makes it the layer in front
    /// whenever it is there at all.
    Switcher,
}

impl Layer {
    /// Where this layer rests when nothing is moving it.
    ///
    /// An app rests on the panel — `0.0`, drawn exactly where it always is — and the switcher rests
    /// off the panel when it is down and over it when it is up. Which is the one place its resting
    /// place has to be asked for rather than assumed, and the reason a drag of it is given a start.
    fn rest(self, switcher_is_up: bool) -> f32 {
        match self {
            Self::App(_) => 0.0,
            Self::Switcher if switcher_is_up => 1.0,
            Self::Switcher => 0.0,
        }
    }
}

/// The launcher.
pub struct Launcher {
    screen: Screen,
    calculator: Option<Calculator>,
    settings: Option<Settings>,
    music: Option<Player>,
    terminal: Option<Terminal>,
    web_manager: Option<WebManager>,
    /// Apps currently running in memory (foreground or background).
    running_apps: Vec<usize>,
    board: Arc<Board>,
    /// The screen's size, as the platform last reported it. The grid's pages and the pager's
    /// thresholds are laid out for it, and it is what the apps whose layout depends on the size are
    /// told — see [`Launcher::hand_over_size`]. Everything else here fills whatever it is given.
    size: Size,
    /// Which page of the grid is currently up.
    page: usize,
    /// Whether the task switcher is down.
    ///
    /// A `bool` and not a `Screen`, because it is not one: the sheet is a layer over whatever is on
    /// screen, and the screen under it is still what is running. What it lists is `running_apps`,
    /// which is the same list the status bar's row of icons is drawn from.
    recents: bool,
    /// An app on its way in or out, while it is moving.
    ///
    /// `Some` for the few hundred milliseconds between the gesture and the app having arrived or
    /// gone, and the only thing that makes the view draw two screens in one frame.
    transition: Option<Transition>,
    clock: String,
    battery: u8,
    charging: bool,
    wifi: u8,
    preferences: SystemPreferences,
}

impl Launcher {
    /// The launcher and every app it hosts, all sharing `board`.
    ///
    /// Hosted apps are lazily loaded on demand when first opened, rather than during boot,
    /// eliminating startup CPU overhead, track scanning, and initial heap allocations.
    pub fn new(board: Arc<Board>) -> Self {
        // The status bar starts from what the board says, and is continually updated in real-time
        // via Subscription (see [`status_stream`]).
        let (clock, _) = current_time_info();
        let battery = board.power().battery_percent().unwrap_or(0);
        let charging = board.power().is_charging().unwrap_or(false);
        let signal = board.wifi().status().signal_bars();

        Self {
            screen: Screen::Grid,
            calculator: None,
            settings: None,
            music: None,
            terminal: None,
            web_manager: None,
            running_apps: Vec::new(),
            board,
            // What the screen is until the platform says: the design panel. The first `Resized`
            // always arrives — the window manager's on a desktop, the host's first iteration on the
            // board — so this is what at most one frame is drawn from, and never what a layout is
            // decided by. A launcher is *told* how big the screen is; it may not assume it.
            size: Size::new(SCREEN as f32, SCREEN as f32),
            page: 0,
            recents: false,
            transition: None,
            clock,
            battery,
            charging,
            wifi: signal,
            preferences: SystemPreferences::default(),
        }
    }

    /// The screen the launcher is laying out for, as the platform last reported it.
    ///
    /// Read by the host and by the tests. It is the platform's number and never a constant: the
    /// grid's pages and the pager's thresholds are this width, and the apps whose layout follows the
    /// screen are handed it — see [`Launcher::hand_over_size`].
    pub fn screen_size(&self) -> Size {
        self.size
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
        self.propagate_preferences();
    }

    /// Propagates the active preferences to all hosted apps.
    /// Propagates the active preferences to all loaded apps.
    fn propagate_preferences(&mut self) {
        let prefs = self.preferences;
        if let Some(app) = &mut self.calculator {
            app.set_preferences(prefs);
        }
        if let Some(app) = &mut self.settings {
            app.set_preferences(prefs);
        }
        if let Some(app) = &mut self.music {
            app.set_preferences(prefs);
        }
        if let Some(app) = &mut self.terminal {
            app.set_preferences(prefs);
        }
        if let Some(app) = &mut self.web_manager {
            app.set_preferences(prefs);
        }
    }

    /// The active interface language.
    pub fn language(&self) -> Language {
        self.preferences.language
    }

    /// Sets the active interface language.
    pub fn set_language(&mut self, language: Language) {
        self.preferences.language = language;
        self.propagate_preferences();
    }

    pub fn get_or_create_calculator(&mut self) -> &mut Calculator {
        if self.calculator.is_none() {
            let mut app = Calculator::new();
            app.set_preferences(self.preferences);
            self.calculator = Some(app);
        }
        self.calculator.as_mut().unwrap()
    }

    pub fn get_or_create_settings(&mut self) -> &mut Settings {
        if self.settings.is_none() {
            let mut app = Settings::new(Arc::clone(&self.board));
            app.set_preferences(self.preferences);
            // The two readings the status bar does not carry. Voltage has been read here all along;
            // the temperature joins it, and comes from the PMIC — this board's battery has no NTC,
            // so the chip's own die temperature is the one the page can be given.
            let voltage_mv = self.board.power().battery_voltage_mv().unwrap_or(0) as u16;
            let temperature_c = self.board.power().chip_temperature_c().ok();
            app.set_battery(settings::Battery {
                percent: self.battery,
                charging: self.charging,
                voltage_mv,
                temperature_c,
            });
            self.settings = Some(app);
        }
        self.settings.as_mut().unwrap()
    }

    pub fn get_or_create_music(&mut self) -> &mut Player {
        if self.music.is_none() {
            let mut app = Player::new(Arc::clone(&self.board));
            app.set_preferences(self.preferences);
            self.music = Some(app);
        }
        self.music.as_mut().unwrap()
    }

    pub fn get_or_create_terminal(&mut self) -> &mut Terminal {
        if self.terminal.is_none() {
            let mut app = Terminal::new();
            app.set_preferences(self.preferences);
            app.update(terminal::Message::Resized(self.size));
            self.terminal = Some(app);
        }
        self.terminal.as_mut().unwrap()
    }

    /// The web manager is the one app here whose state outlives the app: the server it switches is
    /// the board's, so an app opened onto a server that is already up reads that from the board
    /// rather than assuming it is down — see `WebManager::new`.
    pub fn get_or_create_web_manager(&mut self) -> &mut WebManager {
        if self.web_manager.is_none() {
            let mut app = WebManager::new(Arc::clone(&self.board));
            app.set_preferences(self.preferences);
            self.web_manager = Some(app);
        }
        self.web_manager.as_mut().unwrap()
    }

    /// The hosted apps, for the host and the tests.
    pub fn calculator(&mut self) -> &Calculator {
        self.get_or_create_calculator()
    }

    pub fn settings(&mut self) -> &Settings {
        self.get_or_create_settings()
    }

    pub fn music(&mut self) -> &Player {
        self.get_or_create_music()
    }

    pub fn terminal(&mut self) -> &Terminal {
        self.get_or_create_terminal()
    }

    pub fn web_manager(&mut self) -> &WebManager {
        self.get_or_create_web_manager()
    }

    /// Sets what the status bar shows.
    ///
    /// The platform owns these, and pushes them — rather than iced pulling them from a timer,
    /// which would need an async executor this stack does not have. `wifi` is a count of bars on
    /// the HAL's own `0..=`[`style::WIFI_BARS`] scale.
    pub fn set_status(&mut self, clock: impl Into<String>, battery: u8, charging: bool, wifi: u8) {
        self.clock = clock.into();
        self.battery = battery.min(100);
        self.charging = charging;
        self.wifi = wifi.min(style::WIFI_BARS);

        if let Some(settings) = &mut self.settings {
            let voltage_mv = self.board.power().battery_voltage_mv().unwrap_or(0) as u16;
            // Read now, not carried: the firmware sends this message when the chip's temperature has
            // moved half a degree, so "the battery changed" and "the PMIC warmed up" are the same
            // arrival, and the freshest number is the one on the board this instant.
            let temperature_c = self.board.power().chip_temperature_c().ok();
            settings.set_battery(settings::Battery {
                percent: self.battery,
                charging,
                voltage_mv,
                temperature_c,
            });
        }
    }

    /// Whether `index` app is currently running in memory (foreground or background).
    pub fn is_app_running(&self, index: usize) -> bool {
        self.running_apps.contains(&index)
    }

    /// The list of apps currently running in memory (foreground or background).
    pub fn running_apps(&self) -> &[usize] {
        &self.running_apps
    }

    /// Kills the app, releasing its heap/audio memory and dropping its instance.
    pub fn kill_app(&mut self, index: usize) {
        self.running_apps.retain(|&i| i != index);
        match index {
            TERMINAL => self.terminal = None,
            CALCULATOR => self.calculator = None,
            SETTINGS => self.settings = None,
            MUSIC => self.music = None,
            WEB_MANAGER => self.web_manager = None,
            _ => {}
        }

        if self.screen == Screen::App(index) {
            self.screen = Screen::Grid;
        }

        // An app that has just been stopped is not on its way anywhere. Killing one is not a
        // transition — there is nothing left to slide, and the state the view would draw it from is
        // the state that was just freed — so a slide about this app ends with it. The switcher's
        // cross is where this happens: it can be pressed while an app is still moving.
        if matches!(self.transition, Some(transition) if transition.layer == Layer::App(index)) {
            self.transition = None;
        }
    }

    /// The screen showing the grid.
    ///
    /// Paged with [`pomelo_widgets::pager`]: the pages follow the finger while it is down, and settle
    /// onto — or back from — the turn it decided when it leaves. See the module docs.
    fn launcher(&self) -> Element<'_, Message> {
        let pages: Vec<Element<'_, Message>> =
            (0..self.pages()).map(|p| self.page(p)).collect();

        let paged_grid = pomelo_widgets::pager(pages)
            .current_page(self.page)
            .swipe_commit(style::SWIPE_COMMIT)
            .touch_slop(style::SLOP)
            .anim_duration(style::PAGE_SETTLE)
            .curve(style::PAGE_CURVE)
            .on_change(Message::PageChanged);

        let screen = column![
            self.status_bar(),
            paged_grid,
            self.dots(),
            Space::new().height(Length::Fixed(style::GUTTER)),
        ]
        .height(Length::Fill);

        container(screen).width(Length::Fill).height(Length::Fill).into()
    }

    /// One page of the grid: up to [`PER_PAGE`] tiles, evenly distributed in rows and columns.
    ///
    /// Both the columns (horizontal) and rows (vertical) are evenly spaced with flexible spaces
    /// (`space-evenly`), ensuring identical gaps between adjacent icons and towards the screen boundaries.
    fn page(&self, page: usize) -> Element<'_, Message> {
        let first = page * style::PER_PAGE;

        let mut page_column = Column::new()
            .width(Length::Fill)
            .height(Length::Fill);

        for row in 0..style::ROWS {
            page_column = page_column.push(Space::new().height(Length::Fill));

            let mut row_widget = Row::new()
                .width(Length::Fill)
                .align_y(Alignment::Center);

            for col in 0..style::COLUMNS {
                let index = first + row * style::COLUMNS + col;
                let tile: Element<'_, Message> = match CATALOGUE.get(index) {
                    Some(_) => self.tile(index),
                    None => self.placeholder_tile(),
                };

                row_widget = row_widget
                    .push(Space::new().width(Length::Fill))
                    .push(tile);
            }

            row_widget = row_widget.push(Space::new().width(Length::Fill));
            page_column = page_column.push(row_widget);
        }

        page_column = page_column.push(Space::new().height(Length::Fill));
        page_column.into()
    }

    /// One dot per page, the one that is up lit.
    fn dots(&self) -> Element<'_, Message> {
        let up = self.page;
        let is_light = self.preferences.theme.is_light();

        let dots = (0..self.pages()).map(|page| {
            let colour = if is_light {
                if page == up {
                    (31, 35, 40)
                } else {
                    (209, 213, 219)
                }
            } else {
                if page == up {
                    style::DOT_UP
                } else {
                    style::DOT_REST
                }
            };

            container(Space::new())
                .width(Length::Fixed(style::DOT))
                .height(Length::Fixed(style::DOT))
                .style(move |_theme| container::Style {
                    background: Some(Color::from_rgb8(colour.0, colour.1, colour.2).into()),
                    border: Border {
                        radius: (style::DOT / 2.0).into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                })
                .into()
        });

        container(Row::with_children(dots).spacing(style::DOT_GAP))
            .center_x(Length::Fill)
            .into()
    }

    /// How many pages the catalogue makes.
    fn pages(&self) -> usize {
        CATALOGUE.len().div_ceil(style::PER_PAGE)
    }


    fn calculator_screen(&self) -> Element<'_, Message> {
        if let Some(app) = &self.calculator {
            app.view().map(Message::Calculator)
        } else {
            Space::new().into()
        }
    }

    fn music_screen(&self) -> Element<'_, Message> {
        if let Some(app) = &self.music {
            app.view().map(Message::Music)
        } else {
            Space::new().into()
        }
    }

    fn terminal_screen(&self) -> Element<'_, Message> {
        if let Some(app) = &self.terminal {
            app.view().map(Message::Terminal)
        } else {
            Space::new().into()
        }
    }

    fn web_manager_screen(&self) -> Element<'_, Message> {
        if let Some(app) = &self.web_manager {
            app.view().map(Message::WebManager)
        } else {
            Space::new().into()
        }
    }

    /// The settings app, which is the one app here that brings its own back button: it navigates
    /// *inside* itself, so a second one from the launcher would be a second way out of a page. Its
    /// `go_back` reports whether it consumed the press, and that answer is what backgrounds it.
    fn settings_screen(&self) -> Element<'_, Message> {
        if let Some(app) = &self.settings {
            app.view().map(Message::Settings)
        } else {
            Space::new().into()
        }
    }


    /// One app tile: an icon button and label, sized strictly to [`style::TILE_WIDTH`] width.
    ///
    /// Only the app's application icon is tappable. Surrounding spaces and margins allow swipe
    /// gestures to pass through cleanly to [`pomelo_widgets::pager`].
    fn tile(&self, index: usize) -> Element<'_, Message> {
        let entry = &CATALOGUE[index];

        let icon_content: Element<'_, Message> = match entry.icon {
            AppIcon::Glyph(glyph) => container(
                text(glyph.glyph())
                    .size(style::GLYPH)
                    .font(pomelo_material_symbols::font()),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
            AppIcon::Bitmap(bitmap) => render_bitmap_icon(bitmap),
        };

        let icon_button = button(icon_content)
            .width(Length::Fixed(style::ICON))
            .height(Length::Fixed(style::ICON))
            .padding(0)
            .on_press(Message::Open(index))
            .style(move |_theme, status| icon_style(entry, status));

        let label_raw = entry.localized_name(self.preferences.language);
        let label_display = style::truncate_label(label_raw, style::LABEL_MAX_WIDTH, style::LABEL);
        let label_size = style::LABEL;
        let label_color = if self.preferences.theme.is_light() {
            Color::from_rgb8(17, 24, 39)
        } else {
            Color::WHITE
        };

        let contents = column![
            icon_button,
            text(label_display)
                .size(label_size)
                .color(label_color)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(style::GLYPH_GAP)
        .align_x(Alignment::Center);

        container(contents)
            .width(Length::Fixed(style::TILE_WIDTH))
            .center_x(Length::Fixed(style::TILE_WIDTH))
            .into()
    }

    /// An empty placeholder tile maintaining the exact geometry as [`tile`].
    fn placeholder_tile(&self) -> Element<'_, Message> {
        let label_size = style::LABEL;
        let placeholder = column![
            Space::new()
                .width(Length::Fixed(style::ICON))
                .height(Length::Fixed(style::ICON)),
            text(" ")
                .size(label_size)
                .color(Color::TRANSPARENT)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(style::GLYPH_GAP)
        .align_x(Alignment::Center);

        container(placeholder)
            .width(Length::Fixed(style::TILE_WIDTH))
            .center_x(Length::Fixed(style::TILE_WIDTH))
            .into()
    }

    /// The clock, the signal and the battery.
    ///
    /// The three readings are the platform's to push, so the bar is built from what was pushed and
    /// not read from the board here: a `Program` cannot hold a subscription to a timer, and the
    /// board is the platform's half of the pair. What each reading looks like -- and why the signal
    /// has four bars while the icon set has three -- is `status`'s to say.
    fn status_bar(&self) -> Element<'_, Message> {
        let bg_icons: Vec<Icon> = self
            .running_apps
            .iter()
            .filter(|&&index| match self.screen {
                Screen::App(current) => current != index,
                Screen::Grid => true,
            })
            .filter_map(|&index| {
                CATALOGUE
                    .get(index)
                    .map(|entry| entry.icon.as_glyph().unwrap_or(Icon::APPS))
            })
            .collect();

        status::view(
            &self.clock,
            self.battery,
            self.charging,
            self.wifi,
            &bg_icons,
            self.preferences.theme,
        )
    }
}

impl Launcher {
    /// The app's subscriptions: the screen's size, keyboard shortcuts, and hosted app subscriptions.
    ///
    /// Background apps like `music-player` and `settings` (Wi-Fi scanning) keep their subscriptions
    /// alive in the background, while on-screen apps get their subscriptions.
    pub fn subscription(&self) -> Subscription<Message> {
        let resized = iced::window::resize_events().map(|(_window, size)| Message::Resized(size));

        let keyboard = iced::keyboard::listen().filter_map(|event| {
            if let iced::keyboard::Event::KeyPressed {
                key,
                modified_key,
                physical_key,
                ..
            } = event
            {
                if key.as_ref() == iced::keyboard::Key::Character("q")
                    || key.as_ref() == iced::keyboard::Key::Character("Q")
                    || modified_key.as_ref() == iced::keyboard::Key::Character("q")
                    || modified_key.as_ref() == iced::keyboard::Key::Character("Q")
                    || matches!(
                        physical_key,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyQ)
                    )
                {
                    return Some(Message::Back);
                }

                if key.as_ref() == iced::keyboard::Key::Character("w")
                    || key.as_ref() == iced::keyboard::Key::Character("W")
                    || modified_key.as_ref() == iced::keyboard::Key::Character("w")
                    || modified_key.as_ref() == iced::keyboard::Key::Character("W")
                    || matches!(
                        physical_key,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyW)
                    )
                {
                    return Some(Message::Exit);
                }
            }
            None
        });

        let mut subs = vec![
            resized,
            keyboard,
            Subscription::run_with(
                StatusSubscription {
                    board: Arc::clone(&self.board),
                },
                status_stream,
            ),
        ];

        match self.screen {
            Screen::App(TERMINAL) => {
                if let Some(terminal) = &self.terminal {
                    subs.push(terminal.subscription().map(Message::Terminal));
                }
            }
            _ => {}
        }

        if self.running_apps.contains(&MUSIC) {
            if let Some(music) = &self.music {
                subs.push(music.subscription().map(Message::Music));
            }
        }
        if self.running_apps.contains(&SETTINGS) {
            if let Some(settings) = &self.settings {
                subs.push(settings.subscription().map(Message::Settings));
            }
        }

        Subscription::batch(subs)
    }

    /// The theme: the palette and the background the platform paints behind the tree.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    background: Color::from_rgb8(20, 22, 38),
                    ..Palette::DARK
                },
            ),
            ThemeMode::Light => Theme::custom(
                "PomeloLight",
                Palette {
                    background: Color::from_rgb8(242, 242, 247),
                    ..Palette::LIGHT
                },
            ),
        }
    }

    /// Reacts to one message.
    ///
    /// The only work that comes back is the settings app's: it scrolls its own body when the page
    /// changes, and a widget operation has to travel as a [`Task`] from whoever owns the loop.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Open(index) => {
                // Whether this app is opening *over the desktop*, which is the only way one arrives:
                // the desktop is what an app is drawn over, and two apps are never over each other.
                // A tile in the switcher, or a tile of an app already running, changes the screen
                // without a slide for that reason and not to save the frames.
                let from_the_desktop = matches!(self.screen, Screen::Grid);

                self.screen = Screen::App(index);

                // Most recent last, and an app that is already running moves there rather than
                // staying where it was. "Which app did I just open" is the question three things
                // ask — the switcher's list, the status bar's row of icons, and the exit key — and
                // a list in the order they were *first* opened answers it only by accident.
                self.running_apps.retain(|&running| running != index);
                self.running_apps.push(index);

                // Lazy load apps on demand when opened
                match index {
                    TERMINAL => {
                        self.get_or_create_terminal();
                    }
                    CALCULATOR => {
                        self.get_or_create_calculator();
                    }
                    SETTINGS => {
                        self.get_or_create_settings();
                    }
                    MUSIC => {
                        self.get_or_create_music();
                    }
                    WEB_MANAGER => {
                        self.get_or_create_web_manager();
                    }
                    _ => {}
                }

                // An app whose layout follows the screen is told the size this launcher knows — the
                // one the platform reported. The hand-off is not a courtesy: the platform announces
                // the size *once*, when the window or the panel opens, and an app that is not on
                // screen at that moment never hears it. See [`Launcher::hand_over_size`].
                self.hand_over_size();

                if from_the_desktop {
                    let layer = Layer::App(index);

                    // An app opened from the desktop is *sent* to its place: no finger is on it, so
                    // the transition takes itself there. See [`pomelo_widgets::Progress`].
                    self.transition = Some(Transition {
                        layer,
                        motion: pomelo_widgets::Motion::Open,
                        start: layer.rest(self.recents),
                        progress: pomelo_widgets::Progress::To(1.0),
                    });
                }
            }
            Message::PageChanged(page) => {
                self.page = page;
            }
            Message::Back => return self.go_back(),
            Message::Home => self.go_home(),
            Message::TransitionSettled => {
                // The layer has stopped moving, so there is nothing left to draw over the desktop and
                // nothing left to remember about how it got there. What it *is* was settled when the
                // gesture decided, several frames ago.
                self.transition = None;
            }
            Message::Pulled(delta) => self.pull_layer(delta),
            Message::Released(velocity) => self.release_layer(velocity),
            Message::Exit => match self.screen {
                Screen::App(index) => {
                    self.kill_app(index);
                }
                Screen::Grid => {
                    if let Some(&last) = self.running_apps.last() {
                        self.kill_app(last);
                    }
                }
            },
            Message::Status(clock, battery, charging, wifi) => {
                self.set_status(clock, battery, charging, wifi)
            }
            Message::RecentsClose => self.dismiss_switcher(),
            Message::RecentsKill(index) => self.kill_app(index),
            Message::RecentsClearAll => self.kill_everything(),
            Message::Tick => {
                // The call itself decides whether anything is worth reading — see
                // `Settings::refresh_system`, which does nothing at all while some other page is
                // up. This is not the launcher's business, so it is not asked here.
                if let Some(settings) = &mut self.settings {
                    settings.refresh_system();
                }
            }
            Message::Resized(size) => {
                self.size = size;
                self.hand_over_size();
            }
            Message::Calculator(message) => self.get_or_create_calculator().update(message),
            // The player navigates inside itself too — the list and the track — so its back is asked
            // before it is handled like any other message, exactly as the settings app's is. Both the
            // button on the playing screen and the hardware key arrive here as this one message.
            Message::Music(music_player::Message::Back) => return self.go_back(),
            Message::Music(message) => self.get_or_create_music().update(message),
            Message::Terminal(message) => self.get_or_create_terminal().update(message),
            Message::WebManager(message) => self.get_or_create_web_manager().update(message),
            Message::Settings(settings::Message::Back) => return self.go_back(),
            Message::Settings(message) => {
                let (task, new_prefs) = {
                    let settings = self.get_or_create_settings();

                    (settings.update(message), settings.preferences())
                };

                if self.preferences != new_prefs {
                    self.preferences = new_prefs;
                    self.propagate_preferences();
                }

                return task.map(Message::Settings);
            }
        }

        Task::none()
    }

    /// Tells the app on screen the size of the screen, if its layout depends on it.
    ///
    /// The terminal is the one app that does: its keyboard band is a share of the screen's *height*
    /// (clamped — see `touch_keyboard::band_height`) and its transcript wraps to the screen's width.
    /// Its keyboard's width is nobody's business any more, which is why the settings app — whose
    /// whole layout is flex boxes — is not told anything.
    ///
    /// The terminal can also hear the platform itself while it is on screen —
    /// [`Launcher::subscription`] merges its subscription — but it was not on screen when the size
    /// was announced, so an app opened later would lay its band out for the design panel until
    /// something resized the window. This is the hand-off that makes its first frame right instead.
    fn hand_over_size(&mut self) {
        if let Screen::App(TERMINAL) = self.screen {
            if let Some(terminal) = &mut self.terminal {
                terminal.update(terminal::Message::Resized(self.size));
            }
        }
    }

    /// Navigates back: asks the currently active app to go back if it has sub-pages
    /// (e.g. settings using enum state machine mode A). If the app consumes the back
    /// press, the launcher stays in the app. Otherwise (or if the app has no sub-pages),
    /// the launcher backgrounds the app and returns to the grid. In the grid, back does nothing.
    ///
    /// Whatever the app's back press produced comes back out: the settings app scrolls its own
    /// body, and a widget operation has to travel as a [`Task`] from whoever owns the loop.
    fn go_back(&mut self) -> Task<Message> {
        // The switcher first, and it is not about which screen is up: it is a layer over the screen,
        // so a back press reaches it before it reaches the app underneath — the same order the
        // settings app closes its password sheet and its restart question in, and for the same
        // reason. A layer a press reaches *past* is not a layer.
        if self.recents {
            self.dismiss_switcher();

            return Task::none();
        }

        // The app that would be leaving, for the case where the press is not the app's to take. It
        // is read before the match rather than inside it because two of the arms answer with a
        // `bool` that says nothing about *which* app answered.
        let leaving = match self.screen {
            Screen::App(index) => Some(index),
            Screen::Grid => None,
        };

        let (consumed, task) = match self.screen {
            Screen::Grid => (true, Task::none()),
            Screen::App(SETTINGS) => {
                if let Some(settings) = &mut self.settings {
                    let back = settings.go_back();
                    let new_prefs = settings.preferences();

                    if self.preferences != new_prefs {
                        self.preferences = new_prefs;
                        self.propagate_preferences();
                    }

                    match back {
                        Some(task) => (true, task.map(Message::Settings)),
                        None => (false, Task::none()),
                    }
                } else {
                    (false, Task::none())
                }
            }
            // The player has two screens, and back steps between them before it leaves the app: the
            // playing screen gives way to the list, and only a press *from* the list backgrounds it.
            // Nothing to scroll and nothing to redraw, so the answer is a `bool`.
            Screen::App(MUSIC) => match &mut self.music {
                Some(player) => (player.go_back(), Task::none()),
                None => (false, Task::none()),
            },
            Screen::App(_) => (false, Task::none()),
        };

        if !consumed {
            if let Some(app) = leaving {
                let layer = Layer::App(app);

                // Leaving the app is a *transition*, and the screen is the desktop from here: the
                // gesture has decided, and everything that asks which screen is up — the status bar,
                // the exit key, a press that arrives in the middle of the slide — gets the answer a
                // phone would give. What is left to do is draw the app sliding off the panel, which is
                // [`Launcher::view`]'s and [`Message::TransitionSettled`]'s business.
                //
                // Sent rather than pulled: a back *key* has no finger on the panel, so the transition
                // takes itself off the edge. The drag from the same edge is
                // [`Launcher::release_layer`]'s.
                self.transition = Some(Transition {
                    layer,
                    motion: pomelo_widgets::Motion::Back,
                    start: layer.rest(self.recents),
                    progress: pomelo_widgets::Progress::To(1.0),
                });
            }

            self.screen = Screen::Grid;
        }

        task
    }

    /// Returns to the desktop, leaving the running app where it is.
    ///
    /// What the swipe up from the foot of the panel does, and what it means on a phone: the app is
    /// not closed, it is put aside. It stays in `running_apps` and in the status bar, and its tile
    /// brings it back with the page it was on still up. On the grid there is nothing to do, because
    /// the grid *is* home.
    ///
    /// It also puts the task switcher away, because the same gesture means the same thing to it: the
    /// sheet is in front, so "put this aside" is about the sheet.
    ///
    /// The app leaves the panel the way the finger went — upwards, which is the direction that says
    /// "put this aside" rather than "go back". Which is the only difference between this and a back:
    /// the screen, the effect on `running_apps` and the app's own page are the same either way.
    ///
    /// Sent rather than pulled, like a back key: this is what a home *button* asks for. The drag up
    /// from the foot of the panel that ends in the same place is [`Launcher::release_layer`]'s.
    fn go_home(&mut self) {
        if let Screen::App(app) = self.screen {
            let layer = Layer::App(app);

            self.transition = Some(Transition {
                layer,
                motion: pomelo_widgets::Motion::Home,
                start: layer.rest(self.recents),
                progress: pomelo_widgets::Progress::To(1.0),
            });
        }

        self.screen = Screen::Grid;
        self.recents = false;
    }

    /// Takes the layer in front to where a finger has pulled it, and settles nothing.
    ///
    /// What the edge layer sends once a frame while a finger is down. The layer follows the finger:
    /// nothing changes state here and nothing is decided — a drag is a question, and this is the answer
    /// being written down as the finger asks it. What the finger *meant* is settled in
    /// [`Launcher::release_layer`].
    ///
    /// Which layer that is, and which way it is going, come off the drag itself — see
    /// [`Launcher::layer_for`]. This is the arithmetic: how far along its own axis the finger has taken
    /// it, from where the drag was recognised.
    fn pull_layer(&mut self, delta: Vector) {
        use pomelo_widgets::{Motion, Progress};

        // A layer that is already on its way is not this finger's to take: what sent it owns it until
        // it has arrived, and a second answer mid-animation would be a second animation.
        if matches!(self.transition, Some(transition) if matches!(transition.progress, Progress::To(_)))
        {
            return;
        }

        // The layer a finger has hold of: the one it already had, or — on the first frame of a drag —
        // whichever layer this direction is about. A layer that changed the edge it was leaving by
        // halfway through would be a layer that jumped.
        let held = self
            .transition
            .map(|transition| (transition.layer, transition.motion))
            .or_else(|| self.layer_for(delta));

        let Some((layer, motion)) = held else {
            return;
        };

        let start = layer.rest(self.recents);

        // The slop is what a drag spends before it is a drag at all — the finger moved and the panel
        // had not decided yet — so a pull measures from where the drag was recognised rather than from
        // where the finger came down. Without taking it off here the layer jumps that much on the first
        // frame it moves, which is the one frame it may not jump on.
        // The slop comes off *towards the origin* rather than off the number: a finger travelling
        // backwards spends the same 18 px becoming a drag as one travelling forwards, and subtracting a
        // fixed amount from a negative displacement would make the layer travel further than the finger
        // did — or, for a sheet being pushed back up, further than the panel.
        let (travelled, span) = match motion {
            Motion::Back => (delta.x - style::SLOP.copysign(delta.x), self.size.width),
            Motion::Home => (-delta.y - style::SLOP.copysign(-delta.y), self.size.height),
            Motion::Down => (delta.y - style::SLOP.copysign(delta.y), self.size.height),
            Motion::Open => return,
        };

        let pulled = if span > 0.0 {
            (start + travelled / span).clamp(0.0, 1.0)
        } else {
            start
        };

        self.transition = Some(Transition {
            layer,
            motion,
            start,
            progress: Progress::At(pulled),
        });
    }

    /// Which layer a drag of `delta` takes hold of, and which way it takes it — or `None` for a drag
    /// that is on its way to something else.
    ///
    /// The edge a drag began at is not asked here, and could not be: the gesture detector reports a drag
    /// to this launcher only when its origin pin matched, so a downward drag *is* one that began at the
    /// head of the panel and an upward one began at its foot. What is left to decide is what a direction
    /// means — and which layer is in front, because the switcher is over everything while it is down: a
    /// finger on it is talking to it and not to the app behind it.
    fn layer_for(&self, delta: Vector) -> Option<(Layer, pomelo_widgets::Motion)> {
        use pomelo_widgets::Motion;

        if self.recents {
            // The sheet is in front, and it only moves the way it came: up. A finger going down the
            // panel with it down is pushing it the way it is already clamped, and a finger going
            // sideways is on its way to something else — neither is a way to put it away. Those are the
            // gesture from the foot of the panel, a tap on the wash, and the back key.
            return (delta.y < 0.0).then_some((Layer::Switcher, Motion::Down));
        }

        // Down from the head of the panel: the sheet comes down over whatever is up.
        if delta.y > 0.0 && delta.y >= delta.x.abs() {
            return Some((Layer::Switcher, Motion::Down));
        }

        // Otherwise it is the app in front, if there is one: right takes it off to the right, and up
        // puts it aside. A finger going down the panel or going left is on its way somewhere else.
        let Screen::App(app) = self.screen else {
            return None;
        };

        if delta.x > 0.0 && delta.x >= delta.y.abs() {
            Some((Layer::App(app), Motion::Back))
        } else if delta.y < 0.0 && -delta.y > delta.x.abs() {
            Some((Layer::App(app), Motion::Home))
        } else {
            None
        }
    }

    /// Lets go of a layer a finger was holding: it goes where the gesture decided.
    ///
    /// Two ways for a drag to be a decision, and a phone reads both of them: pushed far enough from
    /// where it started, or moving fast enough when the finger left. Either way the layer goes to the
    /// end it was pushed *towards*; anything else is it coming back to where it came from.
    ///
    /// That is one rule for four gestures — an app taken off the panel and an app put aside, a sheet
    /// pulled down and a sheet pushed back up — and it is why a transition carries the place it started
    /// at: an app starts on the panel and a sheet starts over it, so the same push sends one away and
    /// keeps the other.
    ///
    /// The decision changes the layer's *state* here, on this frame, because the gesture has decided:
    /// what is left is drawing it sliding, over a desktop that has already been left behind or a sheet
    /// that is already down.
    fn release_layer(&mut self, velocity: Vector) {
        use pomelo_widgets::{Motion, Progress};

        let Some(transition) = self.transition else {
            // A drag no layer followed: the finger was over the desktop or over an app in the middle of
            // the panel, and the gesture layer only reports those if their origin pin matched — so this
            // is a drag that began at an edge and meant nothing. Nothing to do, and nothing to undo.
            return;
        };

        // Only a held layer can be let go of. One that is already on its way is on its way.
        let Progress::At(pulled) = transition.progress else {
            return;
        };

        // The distance is in shares of the panel and the speed in pixels a second, which is why they
        // are compared against two thresholds that say the same thing in two units. The sign of the
        // distance is the direction the finger pushed: a sheet pulled down and a sheet pushed up are
        // one motion with two signs.
        let (pushed, speed) = match transition.motion {
            Motion::Back => (pulled - transition.start, velocity.x),
            Motion::Home => (pulled - transition.start, -velocity.y),
            Motion::Down => (pulled - transition.start, velocity.y),
            // An arriving app is never held: what sends it is a tap, which is `Progress::To` from the
            // start and never reaches this.
            Motion::Open => return,
        };

        let meant_it = pushed.abs() >= style::TRANSITION_COMMIT
            || pushed.signum() * speed >= style::TRANSITION_FLING;

        // Where it ends up: the other resting place if the gesture meant it, its own if it did not.
        let to = if meant_it {
            if transition.start < 0.5 { 1.0 } else { 0.0 }
        } else {
            transition.start
        };

        self.transition = Some(Transition {
            progress: Progress::To(to),
            ..transition
        });

        match transition.layer {
            // An app that has reached the far end is off the panel, and the desktop is the screen.
            Layer::App(_) if to >= 1.0 => self.screen = Screen::Grid,
            // And the sheet is down over whatever is up — or gone back above it.
            Layer::Switcher => self.recents = to >= 1.0,
            Layer::App(_) => {}
        }
    }

    /// Sends the switcher back up over the panel it came from.
    ///
    /// What a tap on the wash under the cards — [`Message::RecentsClose`] — and the back key both ask
    /// for, and the same answer a finger pushing it back gets: the sheet is *sent*, because whoever
    /// asked has already taken their hand away or never put one on it.
    fn dismiss_switcher(&mut self) {
        // Nothing to put away, or it is already moving: a sheet on its way somewhere is not asked twice.
        if !self.recents || self.transition.is_some() {
            return;
        }

        let layer = Layer::Switcher;
        self.recents = false;

        self.transition = Some(Transition {
            layer,
            motion: pomelo_widgets::Motion::Down,
            start: layer.rest(true),
            progress: pomelo_widgets::Progress::To(0.0),
        });
    }

    /// Stops every app that is running: what the switcher's "clear all" asks for.
    ///
    /// One at a time through [`Launcher::kill_app`], and not by clearing the fields: killing is what
    /// gives an app the chance to release what it holds — the player its audio and its open track,
    /// the terminal its transcript — and a wipe that skipped it would free the struct and strand the
    /// device underneath it.
    pub fn kill_everything(&mut self) {
        for index in std::mem::take(&mut self.running_apps) {
            self.kill_app(index);
        }
    }

    /// The apps that are running, most recent first — what the switcher lists.
    ///
    /// Reversed, because `running_apps` is oldest-first: an app enters the list when it is opened
    /// and moves to the back of it whenever it is opened again (see [`Message::Open`]), so its last
    /// entry is the one a person was most recently looking at. The status bar draws the same list
    /// from the other end, which is why the two agree about which app is newest without either of
    /// them storing a second order.
    fn recent_apps(&self) -> Vec<usize> {
        self.running_apps.iter().rev().copied().collect()
    }

    /// Describes the interface for the current state.
    ///
    /// The screen is wrapped in the panel's edge gestures (see [`Launcher::edge_gestures`]), and
    /// this is the only place they can be put: the platform draws *this* program, so a gesture
    /// around this view is a gesture over every app the launcher hosts. One put inside an app would
    /// be a gesture that app had to know about.
    pub fn view(&self) -> Element<'_, Message> {
        let moving = self.transition;

        let screen = match moving {
            // A layer on its way in or out is two screens in one frame: what is behind it, and the
            // layer itself. Which layer it is decides what the two are, and the motion decides where
            // the one in front is drawn.
            Some(transition) => {
                let foreground = match transition.layer {
                    Layer::App(index) => self.app_screen(index),
                    Layer::Switcher => self.switcher(),
                };

                // What is behind it: the desktop for a layer over the desktop, and for the switcher the
                // screen itself — a sheet is a layer over *that*, not over the desktop, which is what
                // makes it a layer rather than another screen.
                let background = match transition.layer {
                    Layer::App(_) => self.launcher(),
                    Layer::Switcher => self.current_screen(),
                };

                pomelo_widgets::screen_transition(background, foreground)
                    .motion(transition.motion)
                    // Who is taking it where it is going: a finger, one frame at a time, or the
                    // transition itself once the gesture has decided. See [`Transition::progress`].
                    .progress(transition.progress)
                    .duration(style::TRANSITION)
                    .curve(style::TRANSITION_CURVE)
                    .on_settled(Message::TransitionSettled)
                    .into()
            }
            None => self.current_screen(),
        };

        // The task switcher, when it is down and no transition of its own is drawing it, is a layer
        // *over* that screen rather than a screen of its own — the arrangement the settings app gives
        // its restart question, and for the same reason: what is behind the sheet is what the sheet is
        // about, and it is still running.
        let screen: Element<'_, Message> = if self.recents
            && !matches!(moving, Some(transition) if transition.layer == Layer::Switcher)
        {
            stack![screen, self.switcher()].into()
        } else {
            screen
        };

        self.edge_gestures(screen).into()
    }

    /// The screen one of the apps draws.
    ///
    /// The same match [`Launcher::view`] used to make inline, and it is a method because a
    /// transition needs an app's screen whether or not that app *is* the screen: one on its way out
    /// has already given way to the desktop, and what makes it drawable anyway is that its state
    /// never went anywhere — the pages it was on, the tool it was holding, its place in
    /// `running_apps`. An index the catalogue does not have draws the desktop, the one screen that
    /// is always there.
    fn app_screen(&self, app: usize) -> Element<'_, Message> {
        match app {
            TERMINAL => self.terminal_screen(),
            CALCULATOR => self.calculator_screen(),
            SETTINGS => self.settings_screen(),
            MUSIC => self.music_screen(),
            WEB_MANAGER => self.web_manager_screen(),
            _ => self.launcher(),
        }
    }

    /// The screen the launcher is on: the desktop, or the app that is up.
    ///
    /// The one place that question is answered, because two things ask it — the view, and a transition
    /// whose layer is the switcher, which needs whatever the sheet has come down over.
    fn current_screen(&self) -> Element<'_, Message> {
        match self.screen {
            Screen::Grid => self.launcher(),
            Screen::App(index) => self.app_screen(index),
        }
    }

    /// The task switcher, as the layer it is.
    ///
    /// A method because two places draw it — the view, over a screen that is not moving, and a
    /// transition, over a screen the sheet has brought itself down over — and a sheet built twice would
    /// be two answers to what is running.
    fn switcher(&self) -> Element<'_, Message> {
        recents::view(
            &self.recent_apps(),
            self.preferences.language,
            self.preferences.theme,
        )
    }

    /// The screen, with the panel's edge gestures around it.
    ///
    /// Three gestures, and all three are ones a phone has: a drag up from the foot of the panel puts
    /// the app aside, a drag down from its head brings the task switcher over whatever is up, and a
    /// drag in from the **left** side goes back a page.
    ///
    /// All three are *drags*, and that is the whole of what this layer reports: the finger's path
    /// ([`Message::Pulled`], once a frame) and its last velocity ([`Message::Released`]). Everything
    /// they do is something that moves under the finger — an app off the panel, an app put aside, a
    /// sheet coming down — and none of it is decided until the finger leaves. See
    /// [`Launcher::pull_layer`], [`Launcher::release_layer`].
    ///
    /// Every one of them is pinned to the edge it *starts* at, and that pin is the whole of what makes
    /// this layer possible: an unpinned gesture would take every drag there is, and a page that scrolls
    /// is a page of drags. The origin is asked the moment a drag passes [`style::SLOP`], so a scroll in
    /// the middle of a page is never interrupted in the first place — see
    /// `GestureDetector::swipe_origin`. The pans need the pins as much as the swipes do, and for one
    /// more reason: a pan callback with no pin claims nothing at all.
    ///
    /// The downward pin is not a formality, it is the reason the mechanism exists. Of the four
    /// directions it is the one a *page* is most likely to want for itself — every list on this board
    /// scrolls downwards — so the head of the panel is the only place it is claimed from, and a drag
    /// that begins in the middle of an app stays that app's scroll.
    ///
    /// The two horizontal pins are opposite sides of the same idea: a phone's back gesture is a
    /// rightward drag that began at the **left** edge, and a leftward one that began at the right.
    fn edge_gestures<'b>(
        &self,
        screen: Element<'b, Message>,
    ) -> pomelo_widgets::GestureDetector<'b, Message> {
        use pomelo_widgets::{gesture_detector, Edge, SwipeDirection};

        gesture_detector(screen)
            .touch_slop(style::SLOP)
            .swipe_threshold(style::EDGE_SWIPE)
            // The finger's path, and then its last velocity. Every gesture this layer knows is a drag:
            // what the finger has done so far is where the layer is drawn, and what it meant is decided
            // when it leaves. See [`Launcher::pull_layer`] and [`Launcher::release_layer`].
            .on_pan_update(|details| Message::Pulled(details.total_delta))
            .on_pan_end(|details| Message::Released(details.velocity))
            // The one exception is the back gesture that comes in from the *right*, which is still a
            // swipe, with the behaviour
            // it had before an app could follow a finger. An app drawn sliding left as it leaves is a
            // motion this interface does not have, and inventing one for the mirrored half of a gesture
            // would make two back gestures out of one. So this half is claimed (see the pins) and
            // reported as a swipe on release: it goes back — it just goes back the way a button does
            // rather than the way a finger does.
            .on_swipe_left(Message::Back)
            .swipe_origin(SwipeDirection::Up, Edge::Bottom, style::EDGE_ZONE)
            .swipe_origin(SwipeDirection::Down, Edge::Top, style::EDGE_ZONE)
            .swipe_origin(SwipeDirection::Left, Edge::Right, style::EDGE_ZONE)
            .swipe_origin(SwipeDirection::Right, Edge::Left, style::EDGE_ZONE)
    }
}

/// An app icon button: accent color at rest, subtly lit when pressed.
/// For bitmap icons, transparent at rest with a subtle translucent highlight when pressed.
fn icon_style(entry: &'static Entry, status: button::Status) -> button::Style {
    if entry.icon.is_bitmap() {
        let bg = match status {
            button::Status::Pressed => Some(Color::from_rgba(1.0, 1.0, 1.0, 0.15).into()),
            _ => None,
        };
        return button::Style {
            background: bg,
            text_color: Color::WHITE,
            border: Border {
                radius: style::ICON_RADIUS.into(),
                ..Border::default()
            },
            shadow: Shadow::default(),
            snap: false,
        };
    }

    let base_color = entry.color();
    let bg = match status {
        button::Status::Pressed => {
            let (r, g, b) = entry.accent;
            Color::from_rgb(
                ((r as f32 * 1.35).min(255.0)) / 255.0,
                ((g as f32 * 1.35).min(255.0)) / 255.0,
                ((b as f32 * 1.35).min(255.0)) / 255.0,
            )
        }
        _ => base_color,
    };

    button::Style {
        background: Some(bg.into()),
        text_color: Color::WHITE,
        border: Border {
            radius: style::ICON_RADIUS.into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

#[cfg(feature = "desktop")]
fn render_bitmap_icon<'a, Message: 'a>(icon: BitmapIcon) -> Element<'a, Message> {
    let pixel_count = (icon.width as usize) * (icon.height as usize);
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for i in 0..pixel_count {
        let p565 = icon.rgb565[i];
        let r5 = (p565 >> 11) & 0x1F;
        let g6 = (p565 >> 5) & 0x3F;
        let b5 = p565 & 0x1F;

        let r = ((r5 as u32 * 255 + 15) / 31) as u8;
        let g = ((g6 as u32 * 255 + 31) / 63) as u8;
        let b = ((b5 as u32 * 255 + 15) / 31) as u8;
        let a = icon.alpha[i];

        rgba.extend_from_slice(&[r, g, b, a]);
    }
    let handle = iced::widget::image::Handle::from_rgba(
        icon.width as u32,
        icon.height as u32,
        rgba,
    );
    container(
        iced::widget::image(handle)
            .width(Length::Fixed(style::ICON))
            .height(Length::Fixed(style::ICON)),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

#[cfg(not(feature = "desktop"))]
struct BitmapIconWidget {
    icon: BitmapIcon,
    size: f32,
}

#[cfg(not(feature = "desktop"))]
impl<Message, Theme> iced::advanced::widget::Widget<Message, Theme, iced::Renderer>
    for BitmapIconWidget
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        _renderer: &iced::Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        iced::advanced::layout::Node::new(limits.resolve(
            Length::Fixed(self.size),
            Length::Fixed(self.size),
            Size::new(self.size, self.size),
        ))
    }

    fn draw(
        &self,
        _tree: &iced::advanced::widget::Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        renderer.draw_bitmap_565(
            layout.bounds(),
            self.icon.width,
            self.icon.height,
            self.icon.rgb565,
            self.icon.alpha,
        );
    }
}

#[cfg(not(feature = "desktop"))]
fn render_bitmap_icon<'a, Message: 'a>(icon: BitmapIcon) -> Element<'a, Message> {
    container(
        Element::new(BitmapIconWidget {
            icon,
            size: style::ICON,
        })
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}


/// Returns the current (formatted clock string, minute index in day).
fn current_time_info() -> (String, u32) {
    if let Ok(duration) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        let total_secs = duration.as_secs();
        // UTC+8 offset (China Standard Time / Beijing Time: 8 hours = 28,800 seconds)
        let local_secs = total_secs + 28800;
        let day_secs = local_secs % 86400;
        let hours = (day_secs / 3600) as u32;
        let minutes = ((day_secs % 3600) / 60) as u32;
        (format!("{:02}:{:02}", hours, minutes), hours * 60 + minutes)
    } else {
        ("00:00".to_string(), 0)
    }
}

#[derive(Clone)]
struct StatusSubscription {
    board: Arc<Board>,
}

impl std::hash::Hash for StatusSubscription {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        "app_launcher_status_subscription".hash(state);
    }
}

impl PartialEq for StatusSubscription {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.board, &other.board)
    }
}

impl Eq for StatusSubscription {}

fn status_stream(sub: &StatusSubscription) -> impl iced::futures::Stream<Item = Message> {
    let board = Arc::clone(&sub.board);
    let (mut tx, rx) = iced::futures::channel::mpsc::channel(16);

    // Send initial status on subscription creation
    let (clock, _) = current_time_info();
    let battery = board.power().battery_percent().unwrap_or(0);
    let charging = board.power().is_charging().unwrap_or(false);
    let wifi = board.wifi().status().signal_bars();
    let _ = tx.try_send(Message::Status(clock, battery, charging, wifi));

    // Reactive hardware event listener — zero new threads spawned in app-launcher!
    let tx_event = tx;
    let board_clone = Arc::clone(&board);
    board.on_event(move |event| {
        let mut tx = tx_event.clone();
        match event {
            pomelo_hal::SystemEvent::BatteryChanged {
                percent, charging, ..
            } => {
                let (clock, _) = current_time_info();
                let wifi = board_clone.wifi().status().signal_bars();
                let _ = tx.try_send(Message::Status(clock, *percent, *charging, wifi));
            }
            pomelo_hal::SystemEvent::WifiStatusChanged(wifi_status) => {
                let (clock, _) = current_time_info();
                let battery = board_clone.power().battery_percent().unwrap_or(0);
                let charging = board_clone.power().is_charging().unwrap_or(false);
                let _ = tx.try_send(Message::Status(
                    clock,
                    battery,
                    charging,
                    wifi_status.signal_bars(),
                ));
            }
            pomelo_hal::SystemEvent::InputAction(action) => {
                let msg = match action {
                    pomelo_hal::InputAction::Back => Message::Back,
                    pomelo_hal::InputAction::Exit => Message::Exit,
                };
                let _ = tx.try_send(msg);
            }
            pomelo_hal::SystemEvent::Tick => {
                // Once a second, and `try_send` rather than `send`: this runs on the pump thread,
                // and a queue that is momentarily full is a pulse nobody needed — the next one is a
                // second away, and blocking here would stall the hardware events behind it.
                let _ = tx.try_send(Message::Tick);
            }
        }
    });

    rx
}

#[cfg(test)]
mod tests {
    // The tests drive `update` by hand, and a test has no loop to return the `Task` it produces to.
    // What that task does — the settings app scrolling its own body — belongs to the loop.
    #![allow(unused_must_use)]

    /// Opens the switcher the way a finger does: down from the head of the panel, and let go of once
    /// it is far enough down to stay.
    ///
    /// A drag and not a field, because that is what it is now: the sheet follows the finger like
    /// everything else on this panel, and a test that set `recents` itself would be testing a `bool`
    /// rather than the gesture. The settle at the end is so that whatever comes next is not stepping
    /// into a transition still in flight.
    fn pull_the_switcher_down(launcher: &mut Launcher) {
        // Whatever was in flight is finished first: a drag is one gesture at a time, and a test that
        // wants the sheet is not testing what happens to a finger that arrives in the middle of an app
        // sliding off the panel.
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Pulled(Vector::new(0.0, 200.0)));
        launcher.update(Message::Released(Vector::new(0.0, 0.0)));
        launcher.update(Message::TransitionSettled);
    }
    use super::*;

    #[test]
    fn default_preferences_are_chinese_dark_standard() {
        let board = Arc::new(Board::simulated());
        let launcher = Launcher::new(board);

        assert_eq!(launcher.preferences().language, Language::Chinese);
        assert_eq!(launcher.preferences().theme, ThemeMode::Dark);
        assert_eq!(launcher.preferences().font_tier, FontSizeTier::Standard);
    }

    #[test]
    fn catalogue_entries_are_localized_in_both_languages() {
        for entry in CATALOGUE {
            assert_ne!(
                entry.localized_name(Language::Chinese),
                entry.localized_name(Language::English)
            );
            assert!(!entry.localized_name(Language::Chinese).is_empty());
            assert!(!entry.localized_name(Language::English).is_empty());
        }

        assert_eq!(CATALOGUE[MUSIC].localized_name(Language::Chinese), "音乐");
        assert_eq!(CATALOGUE[WEB_MANAGER].localized_name(Language::Chinese), "网页管理");
        assert_eq!(CATALOGUE[TERMINAL].localized_name(Language::Chinese), "终端");
        assert_eq!(CATALOGUE[SETTINGS].localized_name(Language::Chinese), "设置");
        assert_eq!(CATALOGUE[CALCULATOR].localized_name(Language::Chinese), "计算器");
    }

    /// The grid's two pages, in the order they are drawn: the four the box is for, then the one it
    /// is not.
    ///
    /// Written out rather than derived, because the order *is* the interface: a page and a position
    /// are what a finger learns, and a reshuffle that left this test green would be the one kind of
    /// change nobody would notice until they were holding the box.
    #[test]
    fn the_catalogue_is_two_pages_in_the_order_the_grid_shows_them() {
        let names: Vec<&str> = CATALOGUE.iter().map(|entry| entry.name).collect();

        assert_eq!(
            names,
            ["Music", "Web Manager", "Terminal", "Settings", "Calculator"]
        );
        assert_eq!(CATALOGUE.len(), 5, "and the five are all the tiles there are");
        assert_eq!(
            Launcher::new(Arc::new(Board::simulated())).pages(),
            2,
            "four on the first page leaves the fifth on the second"
        );
    }

    #[test]
    fn tile_label_truncation_and_single_line() {
        // The longest name the catalogue has fits the wider tile line width
        assert_eq!(
            style::truncate_label("Web Manager", style::LABEL_MAX_WIDTH, style::LABEL),
            "Web Manager"
        );
        assert_eq!(
            style::truncate_label("Terminal", style::LABEL_MAX_WIDTH, style::LABEL),
            "Terminal"
        );
        assert_eq!(
            style::truncate_label("终端", style::LABEL_MAX_WIDTH, style::LABEL),
            "终端"
        );

        // Strips any potential newline to guarantee single line
        assert_eq!(
            style::truncate_label("Web Manager\nsecond-line", style::LABEL_MAX_WIDTH, style::LABEL),
            "Web Manager"
        );

        // Very long name truncates and appends "..."
        let long_name = "SuperUltraLongApplicationNameThatExceedsWidth";
        let truncated = style::truncate_label(long_name, style::LABEL_MAX_WIDTH, style::LABEL);
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() < long_name.len());
        assert!(!truncated.contains('\n'));
        assert!(style::text_width(&truncated, style::LABEL) <= style::LABEL_MAX_WIDTH);

        // Very long Chinese name also truncates and appends "..."
        let long_chinese = "这是一个超长应用程序名称用于测试截断效果";
        let truncated_zh = style::truncate_label(long_chinese, style::LABEL_MAX_WIDTH, style::LABEL);
        assert!(truncated_zh.ends_with("..."));
        assert!(truncated_zh.chars().count() < long_chinese.chars().count());
        assert!(style::text_width(&truncated_zh, style::LABEL) <= style::LABEL_MAX_WIDTH);
    }

    #[test]
    fn settings_updates_sync_preferences_to_launcher() {
        let board = Arc::new(Board::simulated());
        let mut launcher = Launcher::new(board);

        // Switch language via Settings message
        launcher.update(Message::Settings(settings::Message::SetLanguage(Language::English)));
        assert_eq!(launcher.preferences().language, Language::English);
        assert_eq!(launcher.language(), Language::English);

        // Switch theme via Settings message
        launcher.update(Message::Settings(settings::Message::SetTheme(ThemeMode::Light)));
        assert_eq!(launcher.preferences().theme, ThemeMode::Light);
        assert_eq!(launcher.calculator().theme_mode(), ThemeMode::Light);
        assert_eq!(launcher.music().theme_mode(), ThemeMode::Light);
        assert_eq!(launcher.terminal().theme_mode(), ThemeMode::Light);

        // Cycle font tier via Settings message
        launcher.update(Message::Settings(settings::Message::CycleFontTier));
        assert_eq!(launcher.preferences().font_tier, FontSizeTier::Large);
        assert_eq!(launcher.preferences().font_tier.base_size(), 30.0);

        launcher.update(Message::Settings(settings::Message::CycleFontTier));
        assert_eq!(launcher.preferences().font_tier, FontSizeTier::ExtraSmall);
        assert_eq!(launcher.preferences().font_tier.base_size(), 18.0);

        launcher.update(Message::Settings(settings::Message::CycleFontTier));
        assert_eq!(launcher.preferences().font_tier, FontSizeTier::Small);
        assert_eq!(launcher.preferences().font_tier.base_size(), 20.0);

        launcher.update(Message::Settings(settings::Message::CycleFontTier));
        assert_eq!(launcher.preferences().font_tier, FontSizeTier::Standard);
        assert_eq!(launcher.preferences().font_tier.base_size(), 24.0);

        // Set preferences directly on launcher
        let custom_prefs = SystemPreferences::new(Language::Chinese, ThemeMode::Dark, FontSizeTier::Large);
        launcher.set_preferences(custom_prefs);
        assert_eq!(launcher.preferences(), custom_prefs);
        assert_eq!(launcher.settings().preferences(), custom_prefs);
        assert_eq!(launcher.calculator().preferences(), custom_prefs);
        assert_eq!(launcher.music().preferences(), custom_prefs);
        assert_eq!(launcher.terminal().preferences(), custom_prefs);
        assert_eq!(launcher.calculator().theme_mode(), ThemeMode::Dark);
        assert_eq!(launcher.music().theme_mode(), ThemeMode::Dark);
        assert_eq!(launcher.terminal().theme_mode(), ThemeMode::Dark);
    }

    #[test]
    fn back_navigates_subpages_before_returning_to_grid() {
        let board = Arc::new(Board::simulated());
        let mut launcher = Launcher::new(Arc::clone(&board));

        // 1. Back while on Grid stays on Grid
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);

        // 2. Single-page app (e.g. Calculator) exits directly to Grid
        launcher.update(Message::Open(CALCULATOR));
        assert_eq!(launcher.screen, Screen::App(CALCULATOR));
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);

        // 3. Multi-page app (Settings) navigates internal subpages first
        launcher.update(Message::Open(SETTINGS));
        assert_eq!(launcher.screen, Screen::App(SETTINGS));
        assert_eq!(launcher.settings().section(), settings::SettingsSection::Main);

        // Open Wifi subpage
        launcher.update(Message::Settings(settings::Message::Open(settings::SettingsSection::Wifi)));
        assert_eq!(launcher.settings().section(), settings::SettingsSection::Wifi);

        for _ in 0..3 {
            board.tick();
        }
        launcher.update(Message::Settings(settings::Message::WifiFrame(
            std::time::Instant::now() + std::time::Duration::from_secs(1),
        )));

        // Open password prompt dialog
        launcher.update(Message::Settings(settings::Message::WifiSelect(0)));
        assert!(launcher.settings().wifi().prompt().is_some());

        // Hardware Back (Message::Back) closes prompt, remains on Wifi subpage
        launcher.update(Message::Back);
        assert!(launcher.settings().wifi().prompt().is_none());
        assert_eq!(launcher.settings().section(), settings::SettingsSection::Wifi);
        assert_eq!(launcher.screen, Screen::App(SETTINGS));

        // Hardware Back (Message::Back) returns to Settings Main, remains in Settings
        launcher.update(Message::Back);
        assert_eq!(launcher.settings().section(), settings::SettingsSection::Main);
        assert_eq!(launcher.screen, Screen::App(SETTINGS));

        // Hardware Back from Main exits Settings to Grid
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);
    }

    /// The player is the second app here that navigates inside itself, and it takes the back press
    /// the same way the settings app does: from the track it steps to the list, and only from the
    /// list does the press leave the app.
    #[test]
    fn back_returns_the_music_player_to_its_list_before_the_grid() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        launcher.update(Message::Open(MUSIC));
        assert_eq!(launcher.screen, Screen::App(MUSIC));
        assert_eq!(
            launcher.music().page(),
            music_player::Page::Library,
            "the app opens on the list and plays nothing"
        );

        // What a tap on a row sends: the playing screen comes up.
        launcher.update(Message::Music(music_player::Message::Open(0)));
        assert_eq!(launcher.music().page(), music_player::Page::NowPlaying);

        launcher.update(Message::Back);
        assert_eq!(
            launcher.screen,
            Screen::App(MUSIC),
            "the player consumes the press that leaves its track"
        );
        assert_eq!(launcher.music().page(), music_player::Page::Library);

        launcher.update(Message::Back);
        assert_eq!(
            launcher.screen,
            Screen::Grid,
            "and the press from the list is the launcher's own"
        );
    }

    #[test]
    fn back_keeps_app_running_in_background_and_exit_kills_app() {
        let board = Arc::new(Board::simulated());
        let mut launcher = Launcher::new(Arc::clone(&board));

        // Initially no apps running
        assert!(launcher.running_apps().is_empty());

        // 1. Open Music: it is added to running_apps
        launcher.update(Message::Open(MUSIC));
        assert_eq!(launcher.screen, Screen::App(MUSIC));
        assert!(launcher.is_app_running(MUSIC));
        assert_eq!(launcher.running_apps(), &[MUSIC]);

        // 2. Press Back: returns to Grid, but app continues running in background
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);
        assert!(launcher.is_app_running(MUSIC));

        // 3. Open Calculator as well: both apps running in memory
        launcher.update(Message::Open(CALCULATOR));
        assert_eq!(launcher.screen, Screen::App(CALCULATOR));
        assert!(launcher.is_app_running(CALCULATOR));
        assert_eq!(launcher.running_apps(), &[MUSIC, CALCULATOR]);

        // 4. Press Exit in Calculator: kills Calculator and returns to Grid
        launcher.update(Message::Exit);
        assert_eq!(launcher.screen, Screen::Grid);
        assert!(!launcher.is_app_running(CALCULATOR));
        assert_eq!(launcher.running_apps(), &[MUSIC]);

        // 5. Press Exit on Grid: kills the last background app (Music)
        launcher.update(Message::Exit);
        assert!(!launcher.is_app_running(MUSIC));
        assert!(launcher.running_apps().is_empty());

        // 6. Test Settings reset on kill: navigate to Wifi subpage, kill app, next open is fresh
        launcher.update(Message::Open(SETTINGS));
        launcher.update(Message::Settings(settings::Message::Open(settings::SettingsSection::Wifi)));
        assert_eq!(launcher.settings().section(), settings::SettingsSection::Wifi);

        launcher.update(Message::Exit);
        assert_eq!(launcher.screen, Screen::Grid);
        assert!(!launcher.is_app_running(SETTINGS));

        launcher.update(Message::Open(SETTINGS));
        assert_eq!(launcher.settings().section(), settings::SettingsSection::Main);
    }

    #[test]
    fn status_updates_clock_battery_and_settings() {
        let board = Arc::new(Board::simulated());
        let mut launcher = Launcher::new(Arc::clone(&board));

        launcher.update(Message::Status("14:30".to_string(), 85, true, 3));
        assert_eq!(launcher.clock, "14:30");
        assert_eq!(launcher.battery, 85);
        assert!(launcher.charging);
        assert_eq!(launcher.wifi, 3);
        assert_eq!(launcher.settings().battery().percent, 85);
        assert!(launcher.settings().battery().charging);
    }

    /// The swipe up from the foot of the panel: home, with the app put aside rather than closed.
    ///
    /// Its own message and not `Message::Back` is the point of the test. Back asks the app first, so
    /// on a settings sub-page it turns the page; a swipe up says which of the two things the finger
    /// meant, and the app comes back from the desktop on the page it was on.
    #[test]
    fn a_swipe_up_goes_home_and_leaves_the_app_running() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        launcher.update(Message::Open(SETTINGS));
        launcher.update(Message::Settings(settings::Message::Open(
            settings::SettingsSection::Battery,
        )));
        assert_eq!(launcher.screen, Screen::App(SETTINGS));

        launcher.update(Message::Home);

        assert_eq!(launcher.screen, Screen::Grid, "the desktop is back");
        assert!(
            launcher.is_app_running(SETTINGS),
            "and the app is put aside, not closed"
        );
        assert_eq!(
            launcher.settings().section(),
            settings::SettingsSection::Battery,
            "with the page it was on still up"
        );

        // On the desktop there is nowhere further to go: the grid is what home is.
        launcher.update(Message::Home);
        assert_eq!(launcher.screen, Screen::Grid);
    }

    /// An app is drawn over the desktop while it moves, and the desktop is the screen from the frame
    /// the gesture is recognised on.
    ///
    /// The design in one test: the screen changing and the app still being drawn are two different
    /// events, and what ends the second is the widget reporting that the app has arrived or gone —
    /// not the gesture, and not the launcher.
    #[test]
    fn an_app_is_drawn_over_the_desktop_while_it_moves() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        // 1. Opened from the desktop: an arrival, over a desktop that is still there. The app *is*
        //    the screen already — an app that is arriving is an app that is up — and what the
        //    transition says is that a second screen is being drawn underneath it.
        launcher.update(Message::Open(SETTINGS));
        assert_eq!(launcher.screen, Screen::App(SETTINGS));
        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(SETTINGS),
                motion: pomelo_widgets::Motion::Open,
                start: 0.0,
                progress: pomelo_widgets::Progress::To(1.0),
            }),
            "an app opened from the desktop arrives over it"
        );

        // The end of it: the app is simply up, with nothing in flight.
        launcher.update(Message::TransitionSettled);
        assert!(launcher.transition.is_none());
        assert_eq!(launcher.screen, Screen::App(SETTINGS));

        // 2. A tile tapped while an app is up — the switcher's cards do this — is not an arrival:
        //    the desktop is what an app is over, and two apps are never over each other.
        launcher.update(Message::Open(MUSIC));
        assert_eq!(launcher.screen, Screen::App(MUSIC));
        assert!(
            launcher.transition.is_none(),
            "one app over another does not slide"
        );

        // 3. Home, from the app: the desktop is the screen at once, and the app is what is still
        //    being drawn out of the state this launcher kept for it.
        launcher.update(Message::Home);
        assert_eq!(launcher.screen, Screen::Grid, "the gesture decides the screen");
        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(MUSIC),
                motion: pomelo_widgets::Motion::Home,
                start: 0.0,
                progress: pomelo_widgets::Progress::To(1.0),
            }),
        );
        assert!(
            launcher.is_app_running(MUSIC),
            "put aside rather than closed: the state the slide is drawn from is this launcher's"
        );

        // And the view builds both screens in one frame, which is the whole of what a transition is.
        let _ = launcher.view();

        launcher.update(Message::TransitionSettled);
        assert!(launcher.transition.is_none());

        // 4. An app stopped while it is still moving stops being drawn: there is nothing left to
        //    slide. The switcher's cross is where this happens.
        launcher.update(Message::Open(MUSIC));
        launcher.update(Message::Home);
        assert!(launcher.transition.is_some());

        launcher.update(Message::Exit);
        assert!(
            launcher.transition.is_none(),
            "a killed app is not on its way anywhere"
        );
        assert!(!launcher.is_app_running(MUSIC));
    }

    /// A finger takes the app with it, and what happens when it lets go depends on how far and how
    /// fast — the two halves of a phone's dismissal gesture, and the difference between an app that
    /// followed the hand and one that waited for it.
    #[test]
    fn a_dragged_app_follows_the_finger_and_is_let_go_of() {
        use pomelo_widgets::{Motion, Progress};

        let mut launcher = Launcher::new(Arc::new(Board::simulated()));
        launcher.update(Message::Open(SETTINGS));
        launcher.update(Message::TransitionSettled);
        assert_eq!(launcher.screen, Screen::App(SETTINGS));

        // A finger down on the left edge and 118 px to the right of where the drag was recognised:
        // the app follows it, and nothing at all has been decided yet — it is still the screen, it is
        // still running, and it is where the finger has put it.
        launcher.update(Message::Pulled(Vector::new(style::SLOP + 118.0, 4.0)));

        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(SETTINGS),
                motion: Motion::Back,
                start: 0.0,
                progress: Progress::At(118.0 / SCREEN as f32),
            }),
            "drawn where the finger has taken it, and not where an animation would"
        );
        assert_eq!(
            launcher.screen,
            Screen::App(SETTINGS),
            "a dragged app is still the app: nothing is decided until the finger leaves"
        );
        assert!(launcher.is_app_running(SETTINGS));

        // The finger goes back to where it came down, and the app comes back with it.
        launcher.update(Message::Pulled(Vector::new(style::SLOP, 0.0)));
        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(SETTINGS),
                motion: Motion::Back,
                start: 0.0,
                progress: Progress::At(0.0),
            }),
        );

        // Letting go there sends it back to nothing: the app does not move, which is drawn as a
        // transition that ends where it already is.
        launcher.update(Message::Released(Vector::new(0.0, 0.0)));

        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(SETTINGS),
                motion: Motion::Back,
                start: 0.0,
                progress: Progress::To(0.0),
            }),
            "coming back rather than going away"
        );
        assert_eq!(launcher.screen, Screen::App(SETTINGS));
        assert!(launcher.is_app_running(SETTINGS));

        // And now a flick: a short pull and a fast one. The app goes, and the desktop is the screen
        // from this frame — what is left is drawing the app sliding off a panel that has left it.
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Pulled(Vector::new(style::SLOP + 20.0, 0.0)));
        launcher.update(Message::Released(Vector::new(
            style::TRANSITION_FLING + 100.0,
            0.0,
        )));

        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(SETTINGS),
                motion: Motion::Back,
                start: 0.0,
                progress: Progress::To(1.0),
            }),
        );
        assert_eq!(launcher.screen, Screen::Grid, "the gesture has decided");
        assert!(
            launcher.is_app_running(SETTINGS),
            "and the app was put aside rather than closed"
        );

        // A finger coming *down* the panel is not about the app at all: down from the head of the
        // panel is the switcher, and the app it comes down over is drawn exactly where it was.
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Open(MUSIC));
        launcher.update(Message::TransitionSettled);

        assert_eq!(launcher.screen, Screen::App(MUSIC));

        launcher.update(Message::Pulled(Vector::new(0.0, 200.0)));

        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::Switcher,
                motion: Motion::Down,
                start: 0.0,
                progress: Progress::At((200.0 - style::SLOP) / SCREEN as f32),
            }),
            "down from the head of the panel brings the sheet, not the app"
        );
        assert_eq!(launcher.screen, Screen::App(MUSIC));

        launcher.update(Message::Released(Vector::new(0.0, 0.0)));

        assert!(launcher.recents, "and letting go of it leaves the sheet down");
        assert_eq!(launcher.screen, Screen::App(MUSIC));

        // With the sheet down it is the layer in front, and it moves the way it came: up. The app
        // behind it does not move at all, and a flick upwards is what puts the sheet away.
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Pulled(Vector::new(0.0, -200.0)));

        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::Switcher,
                motion: Motion::Down,
                start: 1.0,
                progress: Progress::At(1.0 - (200.0 - style::SLOP) / SCREEN as f32),
            }),
            "a finger on the sheet takes the sheet"
        );
        assert_eq!(
            launcher.screen,
            Screen::App(MUSIC),
            "and never the app behind it"
        );

        launcher.update(Message::Released(Vector::new(0.0, -600.0)));

        assert!(!launcher.recents, "an upward flick puts the sheet away");
        assert_eq!(launcher.screen, Screen::App(MUSIC));
    }

    /// Every edge gesture is pinned to the edge it starts at.
    ///
    /// The wiring, and not the dragging: a finger's path through the widget tree is the panel
    /// tests' business. What matters here — and what would still build if it went missing — is that
    /// each swipe is tied to a band of the panel. Without the pins the layer over the whole screen
    /// would take every drag on it, and a page that scrolls is a page of drags.
    #[test]
    fn the_gestures_are_pinned_to_the_edges_they_start_at() {
        use pomelo_widgets::{Edge, SwipeDirection};

        let launcher = Launcher::new(Arc::new(Board::simulated()));
        let detector = launcher.edge_gestures(Space::new().into());

        assert_eq!(
            detector.origin(SwipeDirection::Up),
            Some((Edge::Bottom, style::EDGE_ZONE)),
            "a swipe up is a swipe up from the foot of the panel"
        );

        // The back gesture starts at the edge opposite the way it travels: a rightward drag from
        // the left side, a leftward one from the right.
        assert_eq!(
            detector.origin(SwipeDirection::Right),
            Some((Edge::Left, style::EDGE_ZONE))
        );
        assert_eq!(
            detector.origin(SwipeDirection::Left),
            Some((Edge::Right, style::EDGE_ZONE))
        );

        // And the task switcher's drag is claimed only from the head of the panel — which is the
        // pin that earns its keep, because downwards is what every list on this board does.
        assert_eq!(
            detector.origin(SwipeDirection::Down),
            Some((Edge::Top, style::EDGE_ZONE)),
            "a downward drag is the switcher's only if it began at the top"
        );
    }

    /// The top edge brings the switcher down, and it lists what is running — the newest first.
    #[test]
    fn the_top_edge_lists_what_is_running_newest_first() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        for index in [TERMINAL, MUSIC] {
            launcher.update(Message::Open(index));
            launcher.update(Message::Back);
        }

        assert!(
            !launcher.recents,
            "the sheet is not down until the edge is dragged"
        );

        pull_the_switcher_down(&mut launcher);

        assert!(launcher.recents);
        assert_eq!(
            launcher.recent_apps(),
            vec![MUSIC, TERMINAL],
            "the one opened last is listed first"
        );

        // The same app opened again is the newest, wherever it was in the list before.
        launcher.update(Message::RecentsClose);
        launcher.update(Message::Open(TERMINAL));
        launcher.update(Message::Back);

        assert_eq!(launcher.recent_apps(), vec![TERMINAL, MUSIC]);
    }

    /// A tap on the wash puts the sheet away, and stops nothing: it is a layer, not a decision.
    #[test]
    fn putting_the_sheet_away_stops_nothing() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        launcher.update(Message::Open(MUSIC));
        launcher.update(Message::Back);
        pull_the_switcher_down(&mut launcher);
        launcher.update(Message::RecentsClose);

        assert!(!launcher.recents);
        assert!(launcher.is_app_running(MUSIC), "put away, not killed");
    }

    /// The cross stops one app; the clear-all stops every one of them, and frees what they held.
    ///
    /// "Freed" is the half a test can see: a kill that only forgot the index would leave the player
    /// holding the audio device and the terminal holding its transcript, and both would go on
    /// costing the board memory behind a sheet that claimed they were gone.
    #[test]
    fn the_sheet_stops_one_app_or_all_of_them() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        for index in [TERMINAL, MUSIC, SETTINGS] {
            launcher.update(Message::Open(index));
            launcher.update(Message::Back);
        }

        pull_the_switcher_down(&mut launcher);
        launcher.update(Message::RecentsKill(MUSIC));

        assert!(!launcher.is_app_running(MUSIC), "the cross stopped that one");
        assert!(
            launcher.is_app_running(TERMINAL) && launcher.is_app_running(SETTINGS),
            "and left the others alone"
        );
        assert!(
            launcher.recents,
            "and the sheet stayed down, with two cards left"
        );

        launcher.update(Message::RecentsClearAll);

        assert!(launcher.running_apps().is_empty());
        assert!(
            launcher.terminal.is_none() && launcher.music.is_none() && launcher.settings.is_none(),
            "each one was killed, which is what lets it release what it holds"
        );
        assert!(
            launcher.recents,
            "an empty sheet is a sheet that says it is empty"
        );

        // And a clear-all over an empty list is nothing at all, rather than a panic.
        launcher.update(Message::RecentsClearAll);

        assert!(launcher.running_apps().is_empty());
    }

    /// The sheet is a layer, so back and the swipe up are about *it* while it is down.
    #[test]
    fn back_puts_the_sheet_away_before_it_reaches_the_app() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        launcher.update(Message::Open(TERMINAL));
        pull_the_switcher_down(&mut launcher);
        launcher.update(Message::Back);

        assert!(!launcher.recents, "the sheet is what back closed");
        assert_eq!(
            launcher.screen,
            Screen::App(TERMINAL),
            "and the app underneath is still up"
        );

        pull_the_switcher_down(&mut launcher);
        // The sheet is what a gesture is about while it is down, and it is the layer in front: a drag
        // up from the foot of the panel takes *it* away, and the app underneath is exactly where it
        // was — which is the whole difference between a layer and a screen.
        launcher.update(Message::Pulled(Vector::new(0.0, -200.0)));
        launcher.update(Message::Released(Vector::new(0.0, -600.0)));

        assert!(!launcher.recents, "a drag up closes it as well");
        assert_eq!(
            launcher.screen,
            Screen::App(TERMINAL),
            "and the app underneath is still up, because the sheet is what was in front"
        );
    }

    #[test]
    fn page_changed_updates_launcher_page() {
        let board = Arc::new(Board::simulated());
        let mut launcher = Launcher::new(board);
        assert_eq!(launcher.page, 0);

        launcher.update(Message::PageChanged(1));
        assert_eq!(launcher.page, 1);
    }

    #[test]
    fn grid_view_builds_and_all_pages_render_without_panics() {
        let board = Arc::new(Board::simulated());
        let launcher = Launcher::new(board);
        let _view = launcher.view();

        for p in 0..launcher.pages() {
            let _page = launcher.page(p);
        }
    }

    #[test]
    fn apps_are_lazily_loaded_on_demand_and_freed_on_kill() {
        let board = Arc::new(Board::simulated());
        let mut launcher = Launcher::new(board);

        // At boot, all sub-apps are None (0 boot CPU time / 0 heap allocations for apps)
        assert!(launcher.calculator.is_none());
        assert!(launcher.settings.is_none());
        assert!(launcher.music.is_none());
        assert!(launcher.terminal.is_none());
        assert!(launcher.web_manager.is_none());

        // Opening Calculator instantiates only Calculator
        launcher.update(Message::Open(CALCULATOR));
        assert!(launcher.calculator.is_some());
        assert!(launcher.settings.is_none());
        assert!(launcher.music.is_none());
        assert!(launcher.terminal.is_none());
        assert!(launcher.web_manager.is_none());

        // Backgrounding Calculator (Back) preserves instance
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);
        assert!(launcher.calculator.is_some());

        // Exiting / killing Calculator frees its heap memory (reverts to None)
        launcher.update(Message::Exit);
        assert!(launcher.calculator.is_none());
    }
}


