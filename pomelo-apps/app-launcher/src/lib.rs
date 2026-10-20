//! The launcher, built from iced widgets — **a standard iced program**.
//!
//! Deliberately free of raster images: iced's `image` feature pulls the `image` crate into the
//! firmware, and the wallpaper and icons we already bake are RGB565 blits that `pomelo-gfx` does
//! natively. So the wallpaper is a colour — black, so that the bezel's edge is not drawn on the screen —
//! and every picture on the screen is a glyph out of `pomelo_material_symbols`: the grid's in white on a
//! saturated accent square (`apps.rs`), the status bar's on its black band. One font of the whole Material
//! Symbols catalogue, which costs nothing beyond the text this launcher draws anyway. See
//! [`style::WALLPAPER_DARK`].
//!
//! # Hosting an app is merging its subscription
//!
//! The shell's apps are hosted as **widgets**: this launcher calls their `view` and `update` itself,
//! so their state is its state. What a widget cannot carry is an app's *subscriptions* — the one that
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
//! A page is [`PER_PAGE`] apps, four to a row, so the sixteen the catalogue has are two pages:
//! eight and eight.
//!
//! # The grid is the shell plus what is installed
//!
//! The screen's size is not the only thing this launcher is told: the tile list is too. The catalogue
//! is the shell's half — the apps compiled into this image — and the tiles after it are the packages
//! the catalog found on the two volumes. A package's tile is its name, its accent and the first
//! letter of its name, because a package is not compiled into anything and its bitmap is not baked;
//! pressing it reads the package, builds a VM and hands the page to `app-host`, which replays the
//! display list the application writes and delivers the finger to it. The store is what changes that
//! list, and a page that has installed or removed one is what makes the grid rescan —
//! [`Launcher::sync_hosted`] is where the indices are moved.
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
use std::time::Instant;

use iced::theme::Palette;
use iced::widget::{button, column, container, stack, text, Column, Row, Space};
use iced::{
    Alignment, Border, Color, Element, Length, Point, Rectangle, Shadow, Size, Subscription, Task,
    Theme, Vector,
};

use app_host::ui::Fonts;
use app_host::{Catalog, HostedApp, Installed};
use pomelo_hal::Board;
use settings::Settings;
use store::Store;
use updater::Updater;
use terminal::Terminal;

pub use apps::{Entry, CATALOGUE};
pub use pomelo_material_symbols::Icon;
pub use pomelo_widgets::{
    icon_or, AppIcon, BitmapIcon, Language, SystemPreferences, ThemeMode, FONT_BASE,
};
pub use style::{
    DOT_REST, DOT_UP, LABEL, PER_PAGE, SCREEN, STATUS_BG, STATUS_BG_DARK, STATUS_BG_LIGHT,
    STATUS_HEIGHT, STATUS_INSET, WALLPAPER_DARK, WALLPAPER_LIGHT,
};

///
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

///
///
/// The list shrinks as applications leave the image, and it is four long now: the clock, the
/// calculator and ten more are published as packages, and an application that can be installed is
/// one the image should not also carry — see the crate's module docs on what the shell keeps.
///
/// The store is first: an image that carries no applications is a box whose first question is where
/// they come from, and the tile that answers it is the one a thumb reaches without looking.
pub const STORE: usize = 0;
pub const SETTINGS: usize = 1;
pub const TERMINAL: usize = 2;
/// The updater is the last tile on the grid: it is the one that is on its way out, and the store now
/// holds the position it had — see the catalogue's own docs for both.
pub const UPDATER: usize = 3;

///
/// No longer `Eq`: the terminal's own `Message` carries a `Size`, and a size is a pair of floats.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Open(usize),
    ///
    HostedInput(usize, app_abi::Event),
    HostedFrame,
    PageChanged(usize),
    Back,
    ///
    Home,
    Exit,
    ///
    RecentsClose,
    RecentsKill(usize),
    RecentsClearAll,
    ///
    TransitionSettled,
    ///
    Pulled {
        /// Where the finger came down, on the panel.
        start: Point,
        /// How far it has travelled since.
        delta: Vector,
    },
    ///
    Released(Vector),
    ///
    Status(String, u8, bool, u8),
    ///
    Tick,
    Resized(Size),
    Settings(settings::Message),
    Store(store::Message),
    Updater(updater::Message),
    Terminal(terminal::Message),
}

///
///
///
/// Which is why a layer is remembered by name rather than by element: an app's state never left this
/// launcher — the same reason a backgrounded app comes back on the page it was on — and the view
/// builds whatever it draws out of that. See [`Launcher::view`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Transition {
    /// What is moving over the desktop.
    layer: Layer,
    motion: pomelo_widgets::Motion,
    ///
    /// An app rests on the panel; the switcher rests *off* it or *over* it, depending on whether it
    /// was up when the finger arrived. It is where a release falls back to, and what tells a gesture
    /// that meant it from one that changed its mind. See [`Launcher::release_layer`].
    start: f32,
    ///
    /// Kept here rather than asked back out of the widget, because the widget decides nothing and is
    /// only ever told where to draw. One answer to "where is it" is worth more than a widget that can
    /// be asked, and it is what makes the finger and the animation unable to disagree.
    progress: pomelo_widgets::Progress,
}

/// The two things that are drawn *over* the desktop, and can be taken off it by a finger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layer {
    App(usize),
    Switcher,
}

impl Layer {
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

///
struct Hosted {
    installed: Installed,
    /// The running application, once its tile has been pressed.
    app: Option<HostedApp>,
    /// Why the package could not even be read, when it could not. A package that will not parse is
    /// a tile that says so rather than a tile that does nothing.
    error: Option<String>,
}

/// The launcher.
pub struct Launcher {
    screen: Screen,
    /// Every installed package, in the catalog's order: these are the tiles after the shell's.
    hosted: Vec<Hosted>,
    /// When this launcher started. The clock an application's frames and events are stamped with.
    started: Instant,
    settings: Option<Settings>,
    /// The store. Plain `store` and not `store_app`, for the clock's reason: nothing else in the
    /// launcher is called that.
    store: Option<Store>,
    /// The updater. Plain `updater` and not `updater_app`, for the clock's reason: nothing else in
    /// the launcher is called that.
    updater: Option<Updater>,
    terminal: Option<Terminal>,
    /// The file manager. Plain `files` and not `files_app`, for the clock's reason: there is no
    /// status bar reading of that name for a suffix to be told apart from.
    /// The power log. Plain `power_log` and not `power_log_app`, for the same reason as the one above:
    /// nothing else in the launcher is called that.
    /// The channel map. Plain `wifi_map` and not `wifi_map_app`, for the power log's reason: the
    /// launcher has no status bar reading called that, only a count of bars.
    /// The weather page. Plain `weather` and not `weather_app`, for the wifi map's reason: nothing
    /// else in the launcher is called that.
    /// The games app. Plain `games` and not `games_app`, for the weather page's reason.
    /// The cloud page. Plain `cloud` and not `cloud_app`, for the games app's reason.
    /// Apps currently running in memory (foreground or background).
    running_apps: Vec<usize>,
    board: Arc<Board>,
    /// The screen's size, as the platform last reported it. The grid's pages and the pager's
    /// thresholds are laid out for it, and it is what the apps whose layout depends on the size are
    /// told — see [`Launcher::hand_over_size`]. Everything else here fills whatever it is given.
    size: Size,
    /// Which page of the grid is currently up.
    page: usize,
    ///
    /// A `bool` and not a `Screen`, because it is not one: the sheet is a layer over whatever is on
    /// screen, and the screen under it is still what is running. What it lists is `running_apps`,
    /// which is the same list the status bar's row of icons is drawn from.
    recents: bool,
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

        // What an interrupted install left behind is nobody's: a `.part` file in `.staging` and a
        // package in `.trash` are both work that was in the middle of being undone when the box
        // stopped, and neither is ever resumed. This is the boot-time half of the store's promise
        // that a failed install leaves nothing — the store's own failures clean up as they happen,
        // and this is the one that did not get the chance.
        app_host::manager::clean_up(&app_host::Root::standard());

        Self {
            screen: Screen::Grid,
            hosted: scan_hosted(),
            started: Instant::now(),
            settings: None,
            store: None,
            updater: None,
            terminal: None,
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
        if let Some(app) = &mut self.settings {
            app.set_preferences(prefs);
        }
        if let Some(app) = &mut self.store {
            app.set_preferences(prefs);
        }
        if let Some(app) = &mut self.updater {
            app.set_preferences(prefs);
        }
        if let Some(app) = &mut self.terminal {
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

    ///
    /// It asks the repository what it has on the way in, for the updater's reason: the page's whole
    /// subject is what there is to install. The installed list does not need the network, so the
    /// first thing the page shows is true whether or not the request comes back.
    pub fn get_or_create_store(&mut self) -> &mut Store {
        if self.store.is_none() {
            let mut app = Store::new(Arc::clone(&self.board));
            app.set_preferences(self.preferences);
            self.store = Some(app);
        }
        self.store.as_mut().unwrap()
    }

    ///
    /// It asks the repository what it has on the way in, for the weather page's reason one step
    /// further on: a page whose whole subject is "is there a newer one" should open on that question
    /// having been *asked*, rather than on an empty row waiting for a finger. The request is
    /// asynchronous and frame-driven — see the app's own docs — so this app, like the two before it,
    /// is parked in [`Launcher::subscription`] and poked by [`Message::Tick`].
    pub fn get_or_create_updater(&mut self) -> &mut Updater {
        if self.updater.is_none() {
            let mut app = Updater::new(Arc::clone(&self.board));
            app.set_preferences(self.preferences);
            self.updater = Some(app);
        }
        self.updater.as_mut().unwrap()
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

    pub fn get_or_create_terminal(&mut self) -> &mut Terminal {
        if self.terminal.is_none() {
            // The board is handed over here and nowhere else: the terminal is hosted by the launcher, so
            // this is the point at which it can be given the same board the monitor reaches machines
            // through. `terminal`'s own window is the case with none, and shows only the local shell —
            // see [`Terminal::with_board`].
            let mut app = Terminal::new().with_board(Arc::clone(&self.board));
            app.set_preferences(self.preferences);
            app.update(terminal::Message::Resized(self.size));
            self.terminal = Some(app);
        }
        self.terminal.as_mut().unwrap()
    }

    ///

    ///

    ///

    ///

    ///

    ///
    ///

    /// The hosted apps, for the host and the tests.
    pub fn settings(&mut self) -> &Settings {
        self.get_or_create_settings()
    }

    pub fn terminal(&mut self) -> &Terminal {
        self.get_or_create_terminal()
    }

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
            SETTINGS => self.settings = None,
            STORE => self.store = None,
            UPDATER => self.updater = None,
            // An installed package: the VM goes and the package stays, because killing an
            // application is not uninstalling it. Its tile keeps its place on the grid.
            _ => {
                if let Some(at) = self.hosted_at(index) {
                    self.hosted[at].app = None;
                    self.hosted[at].error = None;
                }
            }
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

    /// How many tiles the grid has: the shell's, then the installed packages'.
    pub fn tile_count(&self) -> usize {
        CATALOGUE.len() + self.hosted.len()
    }

    /// The installed package a combined index names, if it names one.
    fn hosted_at(&self, index: usize) -> Option<usize> {
        index
            .checked_sub(CATALOGUE.len())
            .filter(|&at| at < self.hosted.len())
    }

    /// The frame clock, in milliseconds since this launcher started.
    fn now_ms(&self) -> i64 {
        self.started.elapsed().as_millis() as i64
    }

    ///
    /// The package is read here rather than at boot, because a tile is a name and an accent and a
    /// module is a module. What the read costs is paid once per open, by the person waiting for it —
    /// which is the trade the plan made when it said "instantiate on open, destroy on exit".
    fn open_hosted(&mut self, at: usize) {
        let now = self.now_ms();

        let Some(hosted) = self.hosted.get_mut(at) else {
            return;
        };

        if hosted
            .app
            .as_ref()
            .map(HostedApp::is_running)
            .unwrap_or(false)
        {
            return;
        }

        let package = match app_host::Package::read(&hosted.installed.package_path()) {
            Ok(package) => package,
            Err(error) => {
                hosted.app = None;
                hosted.error = Some(error.to_string());
                return;
            }
        };

        let data = hosted.installed.dir.join(app_host::catalog::DATA_DIR);
        // The board's services and not the desktop's even on a PC: the simulator implements the
        // same traits, so an application asks the same questions against the same types wherever it
        // runs. What it gains on the panel is a real radio, speaker and network; what it gains on a
        // PC is that its preferences go through the same storage call the panel needs.
        let services = app_host::services::BoardServices::new(Arc::clone(&self.board), data)
            .for_app(hosted.installed.meta.id.clone());
        let size = (SCREEN as f32, SCREEN as f32);
        let dir = hosted.installed.dir.clone();

        hosted.error = None;
        hosted.app = Some(HostedApp::open(
            &package,
            dir,
            Box::new(services),
            size,
            1.0,
            now,
        ));
    }

    /// Sends one touch to the installed application on `index`.
    fn hosted_event(&mut self, index: usize, event: app_abi::Event) {
        let Some(at) = self.hosted_at(index) else {
            return;
        };
        let Some(hosted) = self.hosted.get_mut(at) else {
            return;
        };
        let Some(app) = hosted.app.as_mut() else {
            return;
        };

        if !app.event(event) {
            if let Some(error) = app.error() {
                hosted.error = Some(error.to_string());
            }
        }
    }

    ///
    /// The exit bit and the failure sentence are read after the frame rather than inside it: both
    /// are the application's answers to having been asked, and both end in the same place — the page
    /// stops asking, and the desktop is what is left.
    fn hosted_frame(&mut self) {
        let Screen::App(index) = self.screen else {
            return;
        };
        let Some(at) = self.hosted_at(index) else {
            return;
        };

        let now = self.now_ms();

        let (failed, exited) = {
            let Some(hosted) = self.hosted.get_mut(at) else {
                return;
            };
            let Some(app) = hosted.app.as_mut() else {
                return;
            };

            let failed = match app.frame(now) {
                Some(_) => None,
                None => app.error().map(str::to_string),
            };

            (failed, app.exited())
        };

        if let Some(error) = failed {
            self.hosted[at].error = Some(error);
        }

        if exited {
            self.hosted[at].app = None;
            self.running_apps.retain(|&running| running != index);

            if self.screen == Screen::App(index) {
                self.screen = Screen::Grid;
            }
        }
    }

    ///
    /// A package installed or removed while the store was open shifts every tile after it, and
    /// `running_apps` and the screen are indices into that list. Matching by id — not by position —
    /// is what makes the shift survivable: an application that is still installed keeps its running
    /// VM wherever it now sits, and one that was removed takes its VM with it.
    pub fn sync_hosted(&mut self) {
        let catalog = Catalog::standard();
        let before: Vec<String> = self
            .hosted
            .iter()
            .map(|hosted| hosted.installed.meta.id.clone())
            .collect();

        let mut after: Vec<Hosted> = Vec::with_capacity(catalog.apps().len());

        for installed in catalog.apps() {
            let app = self
                .hosted
                .iter_mut()
                .find(|hosted| {
                    hosted.installed.meta.id == installed.meta.id
                        && hosted.installed.dir == installed.dir
                })
                .and_then(|hosted| hosted.app.take());

            after.push(Hosted {
                installed: installed.clone(),
                app,
                error: None,
            });
        }

        let unchanged = before.len() == after.len()
            && before
                .iter()
                .zip(&after)
                .all(|(id, hosted)| id == &hosted.installed.meta.id);

        if unchanged {
            return;
        }

        let shell = CATALOGUE.len();
        let remap = |index: usize| -> Option<usize> {
            if index < shell {
                return Some(index);
            }

            let id = before.get(index - shell)?;
            after
                .iter()
                .position(|hosted| &hosted.installed.meta.id == id)
                .map(|at| shell + at)
        };

        let screen = match self.screen {
            Screen::App(index) => remap(index).map(Screen::App),
            other => Some(other),
        };
        let running: Vec<usize> = self.running_apps.iter().filter_map(|&i| remap(i)).collect();

        self.hosted = after;
        self.running_apps = running;
        self.screen = screen.unwrap_or(Screen::Grid);
        self.transition = None;

        if self.page >= self.pages() {
            self.page = self.pages().saturating_sub(1);
        }
    }

    /// The screen one installed application draws, or the sentence saying why it cannot.
    fn hosted_screen(&self, index: usize) -> Element<'_, Message> {
        let Some(hosted) = self.hosted_at(index).and_then(|at| self.hosted.get(at)) else {
            return self.launcher();
        };

        let fonts = Fonts {
            icons: pomelo_material_symbols::font(),
            ..Fonts::default()
        };

        if let Some(app) = &hosted.app {
            if app.is_running() {
                return app.view(fonts, move |event| Message::HostedInput(index, event));
            }
        }

        let sentence = hosted
            .error
            .as_deref()
            .or_else(|| hosted.app.as_ref().and_then(HostedApp::error))
            .unwrap_or("");

        let name = match self.preferences.language {
            Language::Chinese => &hosted.installed.meta.name_zh,
            Language::English => &hosted.installed.meta.name,
        };

        let ink = if self.preferences.theme.is_light() {
            Color::from_rgb8(17, 24, 39)
        } else {
            Color::WHITE
        };

        let back = match self.preferences.language {
            Language::Chinese => "返回桌面",
            Language::English => "Back to desktop",
        };

        container(
            Column::with_children(vec![
                text(name.clone()).size(26).color(ink).into(),
                text(sentence.to_string())
                    .size(15)
                    .color(Color::from_rgb8(150, 156, 168))
                    .into(),
                button(text(back).size(16))
                    .padding([10, 22])
                    .on_press(Message::Back)
                    .into(),
            ])
            .spacing(18)
            .align_x(Alignment::Center),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    }

    ///
    /// The package's own bitmap is not drawn here yet — see `apps.rs` on why the shell's icons are
    /// baked and this one is a letter — but the accent and the name come from the package, so a
    /// tile is still the application's own colour and its own word.
    fn hosted_tile(&self, index: usize) -> Element<'_, Message> {
        let Some(hosted) = self.hosted_at(index).and_then(|at| self.hosted.get(at)) else {
            return self.placeholder_tile();
        };

        let meta = &hosted.installed.meta;
        let accent = Color::from_rgb8(meta.accent.0, meta.accent.1, meta.accent.2);

        let name = match self.preferences.language {
            Language::Chinese => &meta.name_zh,
            Language::English => &meta.name,
        };

        // The package's own glyph, which is the picture this application's tile wore when it was
        // compiled into the image: `meta.glyph` is the name the packer wrote, and `icon_or` is the
        // one place a name becomes a picture. The fallback is the generic mark rather than the first
        // character of the name — a tile is found by its colour and its shape, and a letter is
        // neither: two applications whose names start the same way drew the same tile.
        let icon = icon_or(&meta.glyph);

        let icon_button = button(
            container(
                text(icon.glyph())
                    .size(style::GLYPH)
                    .font(pomelo_material_symbols::font())
                    .color(Color::WHITE),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
        .width(Length::Fixed(style::ICON))
        .height(Length::Fixed(style::ICON))
        .padding(0)
        .on_press(Message::Open(index))
        .style(move |_theme, status| accent_icon_style(accent, status));

        let label_display = style::truncate_label(name, style::LABEL_MAX_WIDTH, style::LABEL);
        let label_color = if self.preferences.theme.is_light() {
            Color::from_rgb8(17, 24, 39)
        } else {
            Color::WHITE
        };

        let contents = column![
            icon_button,
            text(label_display)
                .size(style::LABEL)
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

    /// One running application, for the switcher: the shell's own name and accent, or the package's.
    fn card(&self, index: usize) -> Option<recents::Card> {
        if let Some(entry) = CATALOGUE.get(index) {
            return Some(recents::Card {
                name: entry.localized_name(self.preferences.language).to_string(),
                accent: entry.accent,
                glyph: entry.icon.as_glyph().unwrap_or(Icon::APPS),
            });
        }

        let meta = &self
            .hosted_at(index)
            .and_then(|at| self.hosted.get(at))?
            .installed
            .meta;

        let name = match self.preferences.language {
            Language::Chinese => &meta.name_zh,
            Language::English => &meta.name,
        };

        Some(recents::Card {
            name: name.clone(),
            accent: meta.accent,
            glyph: Icon::APPS,
        })
    }

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
                let tile: Element<'_, Message> = if index < self.tile_count() {
                    self.tile(index)
                } else {
                    self.placeholder_tile()
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

    /// How many pages the grid makes: the shell's tiles, then the installed packages'.
    fn pages(&self) -> usize {
        self.tile_count().div_ceil(style::PER_PAGE)
    }

    fn terminal_screen(&self) -> Element<'_, Message> {
        if let Some(app) = &self.terminal {
            app.view().map(Message::Terminal)
        } else {
            Space::new().into()
        }
    }

    /// The cloud page. One page and no tabs, so the launcher's own back gesture is the way out, as it
    /// is for the pages beside it.

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

    /// The updater. One page and no tabs, so the launcher's own back gesture is the way out, as it is
    /// for the pages beside it. The one hosted page that can end with the box rebooting into a new
    /// image rather than with the app closing.
    fn updater_screen(&self) -> Element<'_, Message> {
        if let Some(app) = &self.updater {
            app.view().map(Message::Updater)
        } else {
            Space::new().into()
        }
    }

    /// The store. One page and no tabs, so the launcher's own back gesture is the way out, exactly
    /// as it is for the updater's.
    fn store_screen(&self) -> Element<'_, Message> {
        if let Some(app) = &self.store {
            app.view().map(Message::Store)
        } else {
            Space::new().into()
        }
    }

    ///
    /// Only the app's application icon is tappable. Surrounding spaces and margins allow swipe
    /// gestures to pass through cleanly to [`pomelo_widgets::pager`].
    fn tile(&self, index: usize) -> Element<'_, Message> {
        // A tile the catalogue does not have is an installed package: the grid is the shell's list
        // with the catalog's appended, and this is the one place that boundary is read.
        let Some(entry) = CATALOGUE.get(index) else {
            return self.hosted_tile(index);
        };

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
            .filter_map(|&index| match CATALOGUE.get(index) {
                Some(entry) => Some(entry.icon.as_glyph().unwrap_or(Icon::APPS)),
                // An installed package has no shell glyph to wear, and the generic mark is the
                // honest one: the row says *an application is running*, not which picture it has.
                None => self.hosted_at(index).map(|_| Icon::APPS),
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

        if self.running_apps.contains(&SETTINGS) {
            if let Some(settings) = &self.settings {
                subs.push(settings.subscription().map(Message::Settings));
            }
        }

        // The channel map's subscription is the terminal's case with a clock in it: it asks the platform
        // for frames *only while a scan is in flight* — see `WifiMap::waiting` — so a page showing a
        // finished scan costs nothing, and a window's sixty frames a second are what carries a scan from
        // "running" to "drawn" in the thirty milliseconds it takes rather than in the second the
        // launcher's own beat would take. On the board there are no frames and the beat is all there is,
        // which is why [`Message::Tick`] is forwarded to it below as well.

        // The weather page's subscription is the channel map's, letter for letter: frames only while
        // its request is in flight — see `Weather::waiting` — so a page showing a finished forecast
        // costs nothing, and the same beat below is what ends the wait on a board with no frames.

        // The games app's subscription is asked for only while a snake is open — see
        // `Games::subscription` — because that is the only thing in it that is not a desktop's arrow
        // key: a snake moves on its own, and the frames it moves on are this platform's. The other
        // game subscribes to nothing a board can hear, so a page showing 2048 costs what it always
        // did. This is the one forwarding here that is not a wait ending: a snake is a clock.

        // An installed application's clock, and the only one it has: a sandbox may not make a timer,
        // so the frames of the window are what its animation steps on and what its redraw requests
        // are answered by. Nothing is subscribed for a page that is not one — which is every page
        // the shell draws — and nothing for a package that is not running.
        if let Screen::App(index) = self.screen {
            let running = self
                .hosted_at(index)
                .and_then(|at| self.hosted.get(at))
                .and_then(|hosted| hosted.app.as_ref())
                .map(HostedApp::is_running)
                .unwrap_or(false);

            if running {
                subs.push(iced::window::frames().map(|_| Message::HostedFrame));
            }
        }

        // The store's subscription is the updater's, letter for letter: frames only while something
        // is out — a check, or the download of a package — so a page sitting on a list costs
        // nothing, and the beat below is what ends the wait on a board with no frames.
        if let (Screen::App(STORE), Some(app)) = (self.screen, &self.store) {
            subs.push(app.subscription().map(Message::Store));
        }

        // The updater's subscription is the weather page's, letter for letter: frames only while a check
        // is in flight — see `Updater::waiting` — so a page sitting on a version number costs nothing,
        // and the beat below is what ends the wait on a board with no frames. The two are the same case
        // for the same reason: what both are waiting on is a request whose answer arrives in a second,
        // where the beat arrives in sixteen.
        if let (Screen::App(UPDATER), Some(app)) = (self.screen, &self.updater) {
            subs.push(app.subscription().map(Message::Updater));
        }

        // The cloud page's subscription is the updater's, letter for letter: frames only while a
        // request is in flight — see `CloudMonitor::waiting` — so a page sitting on a list of machines
        // costs nothing, and the beat above is what ends the wait on a board with no frames. The one
        // thing this page waits on that the others do not is a *power command*: it is the same kind of
        // wait, and it is ended by the same beat.

        Subscription::batch(subs)
    }

    /// The theme: the palette and the background the platform paints behind the tree.
    pub fn theme(&self) -> Theme {
        match self.preferences.theme {
            ThemeMode::Dark => Theme::custom(
                "PomeloDark",
                Palette {
                    // Black, so that the wallpaper and the bezel are the same nothing — see
                    // [`style::WALLPAPER_DARK`], which is where that argument is.
                    background: of(style::WALLPAPER_DARK),
                    ..Palette::DARK
                },
            ),
            ThemeMode::Light => Theme::custom(
                "PomeloLight",
                Palette {
                    background: of(style::WALLPAPER_LIGHT),
                    ..Palette::LIGHT
                },
            ),
        }
    }

    ///
    /// The only work that comes back is the settings app's: it scrolls its own body when the page
    /// changes, and a widget operation has to travel as a [`Task`] from whoever owns the loop.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // An installed application's back is the application's: a sandboxed page has no other way to
        // hear it, and the press it may not want is still answered by the launcher's own gestures —
        // the edge swipes — and by a page that is not running at all, which this press takes back to
        // the desktop. The one press this does *not* take is the hardware key on a live page, which
        // is the application's to interpret: it may have a sub-page to leave.
        if matches!(message, Message::Back) {
            if let Screen::App(index) = self.screen {
                let live = self
                    .hosted_at(index)
                    .and_then(|at| self.hosted.get(at))
                    .and_then(|hosted| hosted.app.as_ref())
                    .map(HostedApp::is_running)
                    .unwrap_or(false);

                if live {
                    self.hosted_event(index, app_host::hosted::back_event());
                    return Task::none();
                }
            }
        }

        match message {
            Message::HostedFrame => self.hosted_frame(),
            Message::HostedInput(index, event) => self.hosted_event(index, event),
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
                    SETTINGS => {
                        self.get_or_create_settings();
                    }
                    STORE => {
                        self.get_or_create_store();
                    }
                    UPDATER => {
                        self.get_or_create_updater();
                    }
                    // An installed package: no `Option` field of its own, because the catalog may
                    // hold a number of them the shell cannot name at compile time.
                    _ => {
                        if let Some(at) = self.hosted_at(index) {
                            self.open_hosted(at);
                        }
                    }
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
            Message::Pulled { start, delta } => self.pull_layer(start, delta),
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

                // The task viewer is the other way round, and the launcher is the one that knows:
                // reading the task list holds the scheduler still for a moment, and a page nobody is
                // on does not need that — while the app cannot tell whether it is the page being
                // looked at. So the question is asked here, and only while it is the screen.

                // The NAS monitor is asked the same way and for a sharper version of the same reason:
                // a look at it is a network round trip to another machine, and a page nobody is on has
                // no business opening sessions to a NAS that nobody is asking about.

                // The power log is asked the fourth way, which is the clock's reason with a longer
                // horizon on it: it is a curve, so the gauge is worth reading while somebody is looking
                // at it — and worth writing down once an hour for as long as they do. Its own
                // subscription is a window's frames and this board has none, so the beat comes from
                // here, and the app throws away everything faster than a minute for the curve and an
                // hour for the log.

                // The channel map is asked the fifth way, and its reason is the first one in this list
                // again: a scan is a wait, and this is the beat that ends it on a board whose host
                // serves no frames. One a second is slow for something that is over in thirty
                // milliseconds, and it is the whole of what this page needs: what it draws is a
                // finished scan, and a press of its own button asks for the next one.

                // The weather page is asked the sixth way, and its reason is the fifth one again: a
                // request is a wait, and this is the beat that ends it on a board whose host serves no
                // frames. One a second is the whole of what the page needs — it draws an answer, and a
                // press of its own button asks for the next one.

                // The games app is asked the seventh way, and it is the odd one here: it is the only
                // app this launcher hands a beat to that has a clock of its own. A snake steps about
                // ten times a second and this beat is once, so the real clock is the frames the app
                // subscribes to — see [`Launcher::subscription`], which forwards them while it is the
                // screen. This is the floor under that: on a host that serves no frames at all, a
                // snake still moves, slowly, rather than sitting still until somebody steers it.

                // The updater is asked the eighth way, and the weather page's reason applies to it
                // twice over: both of its rows are waits — a repository's list, and then a download
                // that takes minutes — and this is the beat that ends them on a board whose host
                // serves no frames. It is asked only while it *is* the screen, which is the one place
                // it differs from the two pages whose requests are over in a moment: a download left
                // running behind another app is a download nobody can watch or stop.
                if let (Screen::App(UPDATER), Some(app)) = (self.screen, &mut self.updater) {
                    app.update(updater::Message::Tick(Instant::now()));
                }

                // The cloud page is asked the ninth way, and its reason is the sixth one again: a
                // request is a wait, and this is the beat that ends it on a board whose host serves no
                // frames. One a second is the whole of what the page needs — it draws an answer, and a
                // press of its own button asks for the next one. A power command asks for the list
                // again a few of these later, which is the same wait a second time rather than a clock
                // of the page's own — see `CloudMonitor::tick`.

                // The store is asked the tenth way, and its reason is the updater's: a check is a
                // wait and a download is a longer one. It is asked only while it *is* the screen,
                // for the updater's own reason: a download that is behind another app is a download
                // nobody can watch or stop.
                if let (Screen::App(STORE), Some(app)) = (self.screen, &mut self.store) {
                    app.update(store::Message::Tick(Instant::now()));
                }

                // An installed application on screen is asked for a frame by the beat too. One a
                // second is slow for an animation and exactly right for a page whose number changes
                // once a second, and it is the floor under a board whose host serves no frames at
                // all — the same floor the pages above it stand on.
                if let Screen::App(index) = self.screen {
                    if self.hosted_at(index).is_some() {
                        self.hosted_frame();
                    }
                }
            }
            Message::Resized(size) => {
                self.size = size;
                self.hand_over_size();
            }
            Message::Terminal(message) => self.get_or_create_terminal().update(message),
            Message::Store(message) => {
                // The store installs into the same directories the grid is built from, so a page
                // that has just changed them is the one thing that makes the grid rescan. Nothing
                // else moves a tile while the launcher is up.
                self.get_or_create_store().update(message);

                if self.get_or_create_store().take_changed() {
                    self.sync_hosted();
                }
            }
            Message::Updater(message) => self.get_or_create_updater().update(message),
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

    ///
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

    ///
    ///
    ///
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

    ///
    ///
    /// Which layer that is, and which way it is going, come off the drag itself — see
    /// [`Launcher::layer_for`]. This is the arithmetic: how far along its own axis the finger has taken
    /// it, from where the drag was recognised.
    fn pull_layer(&mut self, start: Point, delta: Vector) {
        use pomelo_widgets::{Motion, Progress};

        // A layer that is already on its way is not this finger's to take: what sent it owns it until
        // it has arrived, and a second answer mid-animation would be a second animation.
        if matches!(self.transition, Some(transition) if matches!(transition.progress, Progress::To(_)))
        {
            return;
        }

        // The layer a finger has hold of: the one it already had, or — on the first frame of a drag —
        // whichever layer this gesture is about. A layer that changed the edge it was leaving by
        // halfway through would be a layer that jumped.
        let held = self
            .transition
            .map(|transition| (transition.layer, transition.motion))
            .or_else(|| self.layer_for(start, delta));

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

    ///
    ///
    /// The switcher comes first, and not by accident: it is over everything while it is down, so a
    /// finger on it is talking to it and not to the app behind it.
    fn layer_for(&self, start: Point, delta: Vector) -> Option<(Layer, pomelo_widgets::Motion)> {
        use pomelo_widgets::{Edge, Motion};

        let panel = Rectangle::new(Point::ORIGIN, self.size);
        let from = |edge: Edge| edge.contains(start, panel, style::EDGE_ZONE);

        if self.recents {
            // The sheet is in front, and it only moves the way it came: up, out of the foot of the
            // panel. A finger going down the panel with it down is pushing it the way it is already
            // clamped, and a finger that came down anywhere else is on its way to something else —
            // neither is a way to put it away. Those are a tap on the wash and the back key.
            return (from(Edge::Bottom) && delta.y < 0.0).then_some((Layer::Switcher, Motion::Down));
        }

        // Down from the head of the panel: the sheet comes down over whatever is up.
        if from(Edge::Top) && delta.y > 0.0 {
            return Some((Layer::Switcher, Motion::Down));
        }

        // Otherwise it is the app in front, if there is one: a finger from the left edge going right
        // takes it off to the right, and one from the foot of the panel going up puts it aside.
        let Screen::App(app) = self.screen else {
            return None;
        };

        if from(Edge::Left) && delta.x > 0.0 {
            Some((Layer::App(app), Motion::Back))
        } else if from(Edge::Bottom) && delta.y < 0.0 {
            Some((Layer::App(app), Motion::Home))
        } else {
            None
        }
    }

    ///
    ///
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

        // What the decision is about is what the *finger* did, and the sign of it is which way the
        // finger pushed: a sheet pulled down and a sheet pushed up are one motion with two signs.
        let (pushed, span, speed) = match transition.motion {
            Motion::Back => (pulled - transition.start, self.size.width, velocity.x),
            Motion::Home => (
                pulled - transition.start,
                self.size.height,
                -velocity.y,
            ),
            Motion::Down => (
                pulled - transition.start,
                self.size.height,
                velocity.y,
            ),
            // An arriving app is never held: what sends it is a tap, which is `Progress::To` from the
            // start and never reaches this.
            Motion::Open => return,
        };

        // The finger's own travel, and not the layer's: the layer's position is measured from where the
        // drag was recognised (`pull_layer`), so the slop the finger spent getting there is added back
        // here. See [`style::TRANSITION_COMMIT`] for why that distinction is the whole point of the
        // number.
        let travelled = pushed.abs() * span + style::SLOP;

        let meant_it = travelled >= style::TRANSITION_COMMIT
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

    ///
    /// Reversed, because `running_apps` is oldest-first: an app enters the list when it is opened
    /// and moves to the back of it whenever it is opened again (see [`Message::Open`]), so its last
    /// entry is the one a person was most recently looking at. The status bar draws the same list
    /// from the other end, which is why the two agree about which app is newest without either of
    /// them storing a second order.
    fn recent_apps(&self) -> Vec<usize> {
        self.running_apps.iter().rev().copied().collect()
    }

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
            SETTINGS => self.settings_screen(),
            STORE => self.store_screen(),
            UPDATER => self.updater_screen(),
            // An installed package: a page the shell knows nothing about beyond the index it was
            // given. Anything else — an index the catalog has lost since — is the desktop, which is
            // the one screen that is always there.
            _ if self.hosted_at(app).is_some() => self.hosted_screen(app),
            _ => self.launcher(),
        }
    }

    ///
    /// The one place that question is answered, because two things ask it — the view, and a transition
    /// whose layer is the switcher, which needs whatever the sheet has come down over.
    fn current_screen(&self) -> Element<'_, Message> {
        match self.screen {
            Screen::Grid => self.launcher(),
            Screen::App(index) => self.app_screen(index),
        }
    }

    ///
    /// A method because two places draw it — the view, over a screen that is not moving, and a
    /// transition, over a screen the sheet has brought itself down over — and a sheet built twice would
    /// be two answers to what is running.
    fn switcher(&self) -> Element<'_, Message> {
        recents::view(
            &self.recent_apps(),
            self.preferences.language,
            self.preferences.theme,
            &|index| self.card(index),
        )
    }

    ///
    ///
    ///
    ///
    ///
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
            //
            // The start is recovered from the frame it is reported in — where the finger is, less how
            // far it has travelled — because the band it came down in is what says which gesture this
            // is, and `PanUpdateDetails` carries the two numbers that add up to it. See
            // [`Launcher::layer_for`].
            .on_pan_update(|details| Message::Pulled {
                start: details.point - details.total_delta,
                delta: details.total_delta,
            })
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

///
/// The metrics tables are plain `const`s of bytes so that they stay readable in a diff; this is the one
/// place that turns them into the type the renderer wants.
fn of((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb8(r, g, b)
}

/// `colour` with `lift` of the way to white mixed into it: a highlight that cannot clip.
fn lift(colour: Color, lift: f32) -> Color {
    Color {
        r: colour.r + (1.0 - colour.r) * lift,
        g: colour.g + (1.0 - colour.g) * lift,
        b: colour.b + (1.0 - colour.b) * lift,
        a: colour.a,
    }
}

///
/// A boot scans and stops there. Reading a module here would be paying for every application at
/// start-up to draw a tile that is a name, an accent and a letter.
fn scan_hosted() -> Vec<Hosted> {
    Catalog::standard()
        .apps()
        .iter()
        .cloned()
        .map(|installed| Hosted {
            installed,
            app: None,
            error: None,
        })
        .collect()
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

    accent_icon_style(entry.color(), status)
}

///
/// The same rule either way: the accent at rest, and a white wash over it when pressed — not a
/// brighter copy, because the accents are saturated and multiplying one clips the channels it is
/// strongest in. See [`style::TILE_PRESS_LIFT`].
fn accent_icon_style(accent: Color, status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Pressed => lift(accent, style::TILE_PRESS_LIFT),
        _ => accent,
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
        launcher.update(pulled_from_top(200.0));
        launcher.update(Message::Released(Vector::new(0.0, 0.0)));
        launcher.update(Message::TransitionSettled);
    }

    ///
    /// The start is what these carry and not just the distance, because the edge a finger came down
    /// beside is what says which gesture it is making — see [`Launcher::layer_for`]. Written down once
    /// here rather than in each test so that the geometry is in one place and the tests read as the
    /// gestures they are.
    fn pulled_from_left(dx: f32) -> Message {
        Message::Pulled {
            start: Point::new(style::EDGE_ZONE / 2.0, SCREEN as f32 / 2.0),
            delta: Vector::new(dx, 0.0),
        }
    }

    /// And from the foot of the panel: how far it has gone upwards.
    fn pulled_from_bottom(up: f32) -> Message {
        Message::Pulled {
            start: Point::new(SCREEN as f32 / 2.0, SCREEN as f32 - style::EDGE_ZONE / 2.0),
            delta: Vector::new(0.0, -up),
        }
    }

    /// And from the head of the panel: how far it has gone downwards.
    fn pulled_from_top(down: f32) -> Message {
        Message::Pulled {
            start: Point::new(SCREEN as f32 / 2.0, style::EDGE_ZONE / 2.0),
            delta: Vector::new(0.0, down),
        }
    }
    use super::*;

    #[test]
    fn default_preferences_are_chinese_and_dark_at_one_font_size() {
        let board = Arc::new(Board::simulated());
        let launcher = Launcher::new(board);

        assert_eq!(launcher.preferences().language, Language::Chinese);
        assert_eq!(launcher.preferences().theme, ThemeMode::Dark);
        // One size, and it is the size the shipped baked table was cut at.
        assert_eq!(launcher.preferences().font_size(), FONT_BASE);
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

        assert_eq!(CATALOGUE[TERMINAL].localized_name(Language::Chinese), "终端");
        assert_eq!(CATALOGUE[SETTINGS].localized_name(Language::Chinese), "设置");
        assert_eq!(
            CATALOGUE[STORE].localized_name(Language::Chinese),
            "应用商店"
        );
        assert_eq!(CATALOGUE[STORE].name, "App Store");
        assert_eq!(CATALOGUE[UPDATER].localized_name(Language::Chinese), "更新");
        assert_eq!(CATALOGUE[UPDATER].name, "Update");
    }

    ///
    /// Written out rather than derived, because the order *is* the interface: a page and a position
    /// are what a finger learns, and a reshuffle that left this test green would be the one kind of
    /// change nobody would notice until they were holding the box. The page boundary is part of that:
    /// four to a row and two rows is what the first page holds, and what falls past the eighth tile
    /// opens the second — which is where the two rows of the second page are the two rows of it.
    #[test]
    fn the_catalogue_is_one_page_in_the_order_the_grid_shows_them() {
        let names: Vec<&str> = CATALOGUE.iter().map(|entry| entry.name).collect();

        assert_eq!(
            names,
            [
                "App Store",
                "Settings",
                "Terminal",
                "Update"
            ]
        );
        assert_eq!(CATALOGUE.len(), 4, "and the four are all the tiles there are");
        assert_eq!(
            Launcher::new(Arc::new(Board::simulated())).pages(),
            1,
            "four on one page"
        );
    }

    ///
    /// Both halves are the reason the accents and the press were changed together: an accent dim enough to
    /// survive a multiplication is an accent that has stopped being the thing a tile is recognised by.
    #[test]
    fn the_accents_are_vivid_and_a_press_never_clips_them() {
        for entry in CATALOGUE {
            let (r, g, b) = entry.accent;
            let brightest = r.max(g).max(b);

            // 130 sits between the two catalogues this file has had: the near-black shades it carried
            // before topped out at 78, and the only tile under this line today is the settings app's grey
            // — deliberately a grey, the way a phone draws its settings, and nowhere near a shadow.
            assert!(
                brightest >= 130,
                "{entry:?}: a tile has to be a colour, not a shadow"
            );

            let base = entry.color();
            let lit = lift(base, style::TILE_PRESS_LIFT);

            for (was, is) in [(base.r, lit.r), (base.g, lit.g), (base.b, lit.b)] {
                assert!(is > was, "{entry:?}: the press did not brighten it");
                assert!(is <= 1.0, "{entry:?}: the press clipped it to grey");
            }
        }
    }

    #[test]
    fn tile_label_truncation_and_single_line() {
        // A quarter of the panel is 113 px of label, so the names that fit at the font the interface
        // draws at are the short ones: what the catalogue's longest name does now is get cut, which is
        // [`style::LABEL_MAX_WIDTH`]'s own example.
        let longest = style::truncate_label("Web Manager", style::LABEL_MAX_WIDTH, style::LABEL);
        assert!(
            longest.ends_with("..."),
            "{longest}: the longest name does not fit a quarter of the panel"
        );
        assert!(style::text_width(&longest, style::LABEL) <= style::LABEL_MAX_WIDTH);

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
            style::truncate_label("Terminal\nsecond-line", style::LABEL_MAX_WIDTH, style::LABEL),
            "Terminal"
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

    ///
    /// The truncation above is what the label *does*, and for the English names it is what it is for:
    /// "Web Manager" is 151 px against the 113 a quarter of the panel leaves and is meant to be cut.
    /// The Chinese names are not: four characters are 96 px and fit, five are 120 px and do not, and a
    /// five-character name is therefore a name the grid draws as `任务查…` — which two of them were, for
    /// as long as the columns were wide enough that nobody had looked. A name added to the catalogue
    /// has to be counted, and this is where it is counted.
    #[test]
    fn every_chinese_name_fits_a_tile_label() {
        for entry in CATALOGUE {
            let name = entry.localized_name(Language::Chinese);
            assert_eq!(
                style::truncate_label(name, style::LABEL_MAX_WIDTH, style::LABEL),
                name,
                "{name} is wider than a tile's label and is drawn cut"
            );
        }
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
        assert_eq!(launcher.terminal().theme_mode(), ThemeMode::Light);
        assert_eq!(launcher.terminal().theme_mode(), ThemeMode::Light);

        // Set preferences directly on launcher
        let custom_prefs = SystemPreferences::new(Language::Chinese, ThemeMode::Dark);
        launcher.set_preferences(custom_prefs);
        assert_eq!(launcher.preferences(), custom_prefs);
        assert_eq!(launcher.settings().preferences(), custom_prefs);
        assert_eq!(launcher.terminal().preferences(), custom_prefs);
        assert_eq!(launcher.terminal().preferences(), custom_prefs);
        assert_eq!(launcher.terminal().theme_mode(), ThemeMode::Dark);
        assert_eq!(launcher.terminal().theme_mode(), ThemeMode::Dark);
    }

    #[test]
    fn back_navigates_subpages_before_returning_to_grid() {
        let board = Arc::new(Board::simulated());
        let mut launcher = Launcher::new(Arc::clone(&board));

        // 1. Back while on Grid stays on Grid
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);

        // 2. Single-page app (e.g. Files) exits directly to Grid
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

    #[test]
    fn back_keeps_app_running_in_background_and_exit_kills_app() {
        let board = Arc::new(Board::simulated());
        let mut launcher = Launcher::new(Arc::clone(&board));

        // Initially no apps running
        assert!(launcher.running_apps().is_empty());

        // 1. Open Music: it is added to running_apps
        launcher.update(Message::Open(TERMINAL));
        launcher.update(Message::TransitionSettled);
        assert!(launcher.is_app_running(TERMINAL));
        assert_eq!(launcher.running_apps(), &[TERMINAL]);

        // 2. Press Back: returns to Grid, but app continues running in background
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);
        assert!(launcher.is_app_running(TERMINAL));

        // 3. Open the file manager as well: both apps running in memory
        launcher.update(Message::Open(SETTINGS));
        assert!(launcher.is_app_running(SETTINGS));
        assert_eq!(launcher.running_apps(), &[TERMINAL, SETTINGS]);

        // 4. Press Exit in the file manager: kills it and returns to Grid
        launcher.update(Message::Exit);
        assert_eq!(launcher.screen, Screen::Grid);
        assert!(!launcher.is_app_running(SETTINGS));
        assert_eq!(launcher.running_apps(), &[TERMINAL]);

        // 5. Press Exit on Grid: kills the last background app (Music)
        launcher.update(Message::Exit);
        assert!(!launcher.is_app_running(TERMINAL));
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
                layer: Layer::App(SETTINGS),
                motion: pomelo_widgets::Motion::Home,
                start: 0.0,
                progress: pomelo_widgets::Progress::To(1.0),
            }),
        );
        assert!(
            launcher.is_app_running(SETTINGS),
            "put aside rather than closed: the state the slide is drawn from is this launcher's"
        );

        // And the view builds both screens in one frame, which is the whole of what a transition is.
        let _ = launcher.view();

        launcher.update(Message::TransitionSettled);
        assert!(launcher.transition.is_none());
        launcher.update(Message::Open(SETTINGS));
        launcher.update(Message::TransitionSettled);

        // 4. An app stopped while it is still moving stops being drawn: there is nothing left to
        //    slide. The switcher's cross is where this happens.
        launcher.update(Message::Home);
        assert!(launcher.transition.is_some());

        launcher.update(Message::Exit);
        assert!(
            launcher.transition.is_none(),
            "a killed app is not on its way anywhere"
        );
        assert!(!launcher.is_app_running(TERMINAL));
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
        launcher.update(Message::TransitionSettled);
        assert_eq!(launcher.screen, Screen::App(SETTINGS));

        // A finger down on the left edge and 118 px to the right of where the drag was recognised:
        // the app follows it, and nothing at all has been decided yet — it is still the screen, it is
        // still running, and it is where the finger has put it.
        launcher.update(pulled_from_left(style::SLOP + 118.0));

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
        launcher.update(pulled_from_left(style::SLOP));
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
        launcher.update(pulled_from_left(style::SLOP + 20.0));
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
        launcher.update(Message::TransitionSettled);

        launcher.update(pulled_from_top(200.0));

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

        launcher.update(Message::Released(Vector::new(0.0, 0.0)));

        assert!(launcher.recents, "and letting go of it leaves the sheet down");

        // With the sheet down it is the layer in front, and it moves the way it came: up. The app
        // behind it does not move at all, and a flick upwards is what puts the sheet away.
        launcher.update(Message::TransitionSettled);
        launcher.update(pulled_from_bottom(200.0));

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
            Screen::Grid,
            "and never the app behind it"
        );

        launcher.update(Message::Released(Vector::new(0.0, -600.0)));

        assert!(!launcher.recents, "an upward flick puts the sheet away");
    }

    ///
    /// Both halves of that are about how hard a gesture is to make. The band a finger comes down in
    /// decides which gesture it is — a thumb that begins a swipe up from the foot of the panel with a
    /// sideways twitch is still swiping up, and the twitch is most of the movement here — and 96 px of
    /// travel is enough to mean it, which is the distance an edge swipe needed before any of this
    /// followed a finger.
    #[test]
    fn a_gesture_is_judged_by_the_edge_it_came_from() {
        use pomelo_widgets::{Motion, Progress};

        let mut launcher = Launcher::new(Arc::new(Board::simulated()));
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Open(TERMINAL));
        launcher.update(Message::TransitionSettled);

        // A thumb down in the last 24 px of the panel, going up and a little sideways.
        launcher.update(Message::Pulled {
            start: Point::new(SCREEN as f32 / 2.0, SCREEN as f32 - style::EDGE_ZONE / 2.0),
            delta: Vector::new(30.0, -50.0),
        });

        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(TERMINAL),
                motion: Motion::Home,
                start: 0.0,
                progress: Progress::At((50.0 - style::SLOP) / SCREEN as f32),
            }),
            "up from the foot of the panel puts the app aside, whatever the wrist did first"
        );

        // Let go of short of the threshold and the app comes back, which is where it stays.
        launcher.update(Message::Released(Vector::new(0.0, 0.0)));
        launcher.update(Message::TransitionSettled);

        // Then 118 px out of the left edge: past the point of no return on distance alone, with no help
        // from speed and without crossing a third of the panel.
        launcher.update(pulled_from_left(style::SLOP + 118.0));
        launcher.update(Message::Released(Vector::new(0.0, 0.0)));

        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(TERMINAL),
                motion: Motion::Back,
                start: 0.0,
                progress: Progress::To(1.0),
            }),
            "96 px of travel is what an edge swipe used to take"
        );
        assert_eq!(launcher.screen, Screen::Grid, "so the app leaves");

        // And a finger that came down at the *head* of the panel going up takes nothing with it: that
        // edge is where the switcher comes down from, and it does not come up.
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Pulled {
            start: Point::new(SCREEN as f32 / 2.0, style::EDGE_ZONE / 2.0),
            delta: Vector::new(0.0, -50.0),
        });

        assert_eq!(
            launcher.transition, None,
            "no layer moves for a finger going the wrong way out of an edge"
        );

        // And — the case this was reported for — a finger that comes down *inside* the band but not
        launcher.update(Message::Open(TERMINAL));
        launcher.update(Message::TransitionSettled);
        // glued to the edge is making the gesture just the same. 48 px of a 480 px panel is under 4 mm
        // of board, and a thumb goes where it goes.
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Pulled {
            start: Point::new(SCREEN as f32 / 2.0, SCREEN as f32 - style::EDGE_ZONE + 4.0),
            delta: Vector::new(6.0, -120.0),
        });

        assert!(
            matches!(
                launcher.transition,
                Some(Transition {
                    layer: Layer::App(TERMINAL),
                    motion: Motion::Home,
                    ..
                })
            ),
            "a finger 44 px up from the foot of the panel is still swiping up from it"
        );

        // And one that came down outside the band is not: a band is a band, and this is what it costs.
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Pulled {
            start: Point::new(SCREEN as f32 / 2.0, SCREEN as f32 - style::EDGE_ZONE - 12.0),
            delta: Vector::new(6.0, -120.0),
        });

        assert_eq!(
            launcher.transition, None,
            "a finger that came down above the band is scrolling the page, not leaving it"
        );

        // And a short pull that *leaves* fast is a flick: 40 px is under the distance and 700 px/s is
        // far over the speed. This is the case that was reported as "it moves and then goes back" —
        // the finger had travelled far enough for the layer to be visibly on its way, the hand paused
        // first, and an average over the whole drag read as a crawl.
        launcher.update(pulled_from_bottom(40.0));
        launcher.update(Message::Released(Vector::new(0.0, -700.0)));

        assert_eq!(
            launcher.transition,
            Some(Transition {
                layer: Layer::App(TERMINAL),
                motion: Motion::Home,
                start: 0.0,
                progress: Progress::To(1.0),
            }),
            "40 px of travel is not far enough on its own, but 700 px/s is"
        );
    }

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

        for index in [STORE, TERMINAL] {
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
            vec![TERMINAL, STORE],
            "the one opened last is listed first"
        );

        // The same app opened again is the newest, wherever it was in the list before.
        launcher.update(Message::RecentsClose);
        launcher.update(Message::Open(TERMINAL));
        launcher.update(Message::TransitionSettled);
        launcher.update(Message::Back);

        assert_eq!(launcher.recent_apps(), vec![TERMINAL, STORE]);
    }

    /// A tap on the wash puts the sheet away, and stops nothing: it is a layer, not a decision.
    #[test]
    fn putting_the_sheet_away_stops_nothing() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));
        launcher.update(Message::Open(TERMINAL));
        launcher.update(Message::TransitionSettled);

        launcher.update(Message::Back);
        pull_the_switcher_down(&mut launcher);
        launcher.update(Message::RecentsClose);

        assert!(!launcher.recents);
        assert!(launcher.is_app_running(TERMINAL), "put away, not killed");
    }

    ///
    /// "Freed" is the half a test can see: a kill that only forgot the index would leave the player
    /// holding the audio device and the terminal holding its transcript, and both would go on
    /// costing the board memory behind a sheet that claimed they were gone.
    #[test]
    fn the_sheet_stops_one_app_or_all_of_them() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        for index in [TERMINAL, STORE, SETTINGS] {
            launcher.update(Message::Open(index));
            launcher.update(Message::Back);
        }

        pull_the_switcher_down(&mut launcher);
        launcher.update(Message::RecentsKill(TERMINAL));

        assert!(!launcher.is_app_running(TERMINAL), "the cross stopped that one");
        assert!(
            launcher.is_app_running(STORE) && launcher.is_app_running(SETTINGS),
            "and left the others alone"
        );
        assert!(
            launcher.recents,
            "and the sheet stayed down, with two cards left"
        );

        launcher.update(Message::RecentsClearAll);

        assert!(launcher.running_apps().is_empty());
        assert!(
            launcher.terminal.is_none() && launcher.store.is_none() && launcher.settings.is_none(),
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
        launcher.update(Message::TransitionSettled);
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
        launcher.update(pulled_from_bottom(200.0));
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
        assert!(launcher.settings.is_none());
        assert!(launcher.store.is_none());
        assert!(launcher.terminal.is_none());
        assert!(launcher.settings.is_none());

        // Opening the settings instantiates only the settings
        launcher.update(Message::Open(SETTINGS));
        assert!(launcher.settings.is_some());
        assert!(launcher.terminal.is_none());
        assert!(launcher.store.is_none());
        assert!(launcher.updater.is_none());

        // Backgrounding the settings (Back) preserves instance
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);
        assert!(launcher.settings.is_some());

        // Exiting / killing the settings frees its heap memory (reverts to None)
        launcher.update(Message::Exit);
    }

    /// An installed package is a tile after the shell's and a page of its own — and a package that
    /// cannot be read is a page that says so, because a tile that does nothing is not an answer.
    #[test]
    fn an_installed_package_is_a_tile_after_the_shell_and_a_page_of_its_own() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));
        assert_eq!(launcher.tile_count(), 4, "the shell alone");
        assert_eq!(launcher.pages(), 1, "four on one page");

        let meta = app_host::Meta::parse(
            r#"{"id":"notes","name":"Notes","name_zh":"笔记","version":"v1.0.0","memory_kb":64}"#,
        )
        .expect("a package that parses");

        launcher.hosted.push(Hosted {
            installed: Installed {
                meta,
                source: app_host::Source::Internal,
                // Nothing is at this path, which is what a package directory that is gone looks
                // like to the launcher: the read fails, and the page says why.
                dir: std::path::PathBuf::from("/nonexistent/hivebox-apps/notes"),
                icon: None,
                disabled: false,
            },
            app: None,
            error: None,
        });

        assert_eq!(launcher.tile_count(), 5);
        assert_eq!(launcher.pages(), 1, "five on one page");
        assert_eq!(
            launcher.card(4).expect("a card").name,
            "笔记",
            "the sheet says what the package calls itself, in the panel's language"
        );

        launcher.update(Message::Open(4));
        assert_eq!(launcher.screen, Screen::App(4));
        assert!(launcher.hosted[0].error.is_some(), "the read failed");

        // The page is the sentence, not a panic and not a blank screen.
        let _ = launcher.app_screen(4);

        // A page that is not running gives the back press back to the launcher.
        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);
    }

    /// The store lives the same four steps as the pages before it — built when its tile is tapped,
    /// kept while backgrounded, freed on exit — and its own messages reach it and only it.
    #[test]
    fn the_store_is_loaded_on_demand_freed_on_kill_and_routed_to() {
        let mut launcher = Launcher::new(Arc::new(Board::simulated()));

        assert!(launcher.store.is_none());

        launcher.update(Message::Open(STORE));
        assert!(launcher.store.is_some());
        assert_eq!(launcher.screen, Screen::App(STORE));

        launcher.update(Message::Back);
        assert_eq!(launcher.screen, Screen::Grid);
        assert!(launcher.store.is_some(), "backgrounding keeps it");

        launcher.update(Message::Exit);
        assert!(launcher.store.is_none(), "killing frees it");

        // A message from the page builds it again if it was not up, and the tab it asks for is the
        // page's own state: nothing here decides what a page shows.
        launcher.update(Message::Store(store::Message::Section(
            store::Section::Updates,
        )));
        assert!(launcher.store.is_some());
        assert_eq!(
            launcher.get_or_create_store().section(),
            store::Section::Updates
        );
    }
}
