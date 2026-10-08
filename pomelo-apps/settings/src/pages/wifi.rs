//! The Wi-Fi page: radio switch, scan results, connection card, and password prompt.

use std::sync::Arc;
use std::time::Duration;

use iced::time::Instant;
use iced::widget::{button, container, opaque, stack, text, Column, Row, Space};
use iced::{border::Radius, Alignment, Border, Length, Padding};
use pomelo_hal::wifi_credentials::WifiCredentials;
use pomelo_hal::{ApInfo, Board, ScanState, WifiState, WifiStatus};
use pomelo_material_symbols::Icon;
use pomelo_widgets::touch_keyboard;
use pomelo_widgets::touch_keyboard::{KeyAction, KeyboardMode};
use pomelo_widgets::{Language, SystemPreferences, ThemeMode};

use crate::i18n::{Key, LanguageExt as _};
use crate::pages::card::{Header, Tile};
use crate::pages::common::{
    action_row, body, capsule_button, card_surface, confirm_style, dismiss_style, notice_card, page,
    row_style, separator, switch_row, text_button, toggle, UI,
};
use crate::style;
use crate::{Message, SettingsSection};

/// How often an open Wi-Fi page reads the radio.
///
/// The page is told about every frame while it is up (see `Settings::subscription`) and reads the
/// board this often. A read is a mutex and a few atomics; four times a second is quicker than a
/// finger and far slower than the board's own loop.
const READ_EVERY: Duration = Duration::from_millis(250);

/// The Wi-Fi page: its state and its rules, next to the widgets that draw it.
///
/// This is the only page with a conversation of its own — the radio is asked, waited on, and asked
/// again — so it is the only page that owns state. The board it talks to lives here rather than on
/// `Settings`, which is what the rest of the app needs none of.
pub struct Wifi {
    /// The radio. The one hardware this page talks to, and the reason it holds anything at all.
    board: Arc<Board>,
    /// Whether the radio is on. Mirrors the board; the page's switch is what changes it.
    enabled: bool,
    /// What the last completed scan found, best signal first.
    access_points: Vec<ApInfo>,
    /// Where the radio is: idle, scanning, or finished.
    scan: ScanState,
    /// The connection as of the last read.
    status: WifiStatus,
    /// The prompt is open for this entry of `access_points`.
    prompt: Option<usize>,
    /// What has been typed into the prompt.
    password: String,
    /// Which page the prompt's keyboard is showing.
    keyboard: KeyboardMode,
    /// Whether the prompt draws what has been typed in the clear.
    revealed: bool,
    /// A connect has been asked for and has not come back yet.
    pending: bool,
    /// What that connect asked for, held until the radio answers.
    ///
    /// The password lives here and not in the HAL because the file is the app's: the app asked for
    /// the connection, so the app is what knows to write it when one comes up.
    attempt: Option<WifiCredentials>,
    /// The last attempt failed, so the prompt says so.
    failed: bool,
    /// When the radio was last read. `None` until the next frame after a scan was asked for.
    read_at: Option<Instant>,
    /// Whether the finished scan's results have been taken.
    ///
    /// `ScanState::Done` stays `Done` until the next scan starts, so without this the page would
    /// rebuild the list four times a second for as long as it is open.
    taken: bool,
}

impl Wifi {
    /// The page before the radio has said anything.
    pub(crate) fn new(board: Arc<Board>) -> Self {
        Self {
            board,
            enabled: false,
            access_points: Vec::new(),
            scan: ScanState::Idle,
            status: WifiStatus::default(),
            prompt: None,
            password: String::new(),
            keyboard: KeyboardMode::Lower,
            revealed: false,
            pending: false,
            attempt: None,
            failed: false,
            read_at: None,
            taken: false,
        }
    }

    // =========================================================================
    // What the host reads
    // =========================================================================

    /// Whether the radio is on.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// What the last scan found, best signal first.
    pub fn access_points(&self) -> &[ApInfo] {
        &self.access_points
    }

    /// The connection as of the last read.
    pub fn status(&self) -> &WifiStatus {
        &self.status
    }

    /// The network the password prompt is open for, if it is open.
    pub fn prompt(&self) -> Option<&ApInfo> {
        self.prompt
            .and_then(|index| self.access_points.get(index))
    }

    /// Whether a prompt is open at all.
    pub(crate) fn has_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    /// What has been typed into the prompt.
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Whether the prompt is drawing the password in the clear.
    pub fn revealed(&self) -> bool {
        self.revealed
    }

    /// Whether this page is waiting on the radio for something only the radio can say.
    ///
    /// A scan is in flight, or a finished scan's results have not been taken yet, or a connection
    /// is being waited for. Nothing else needs the loop: the page redraws when a finger touches it.
    pub(crate) fn is_waiting(&self, section: SettingsSection) -> bool {
        if section != SettingsSection::Wifi || !self.enabled {
            return false;
        }

        let waiting_for_results = self.scan == ScanState::Done && !self.taken;

        // `ScanState::Error` is deliberately not "waiting": nothing will arrive to stop it, and a
        // subscription that never ends is the bug this method exists to prevent.
        self.scan == ScanState::Scanning || waiting_for_results || self.pending
    }

    // =========================================================================
    // The rules: one method per message, and every one of them asks the board
    // rather than guessing what it said.
    // =========================================================================

    /// Flips the radio's switch, and asks for a scan when it goes on.
    pub(crate) fn toggle(&mut self) {
        let on = !self.enabled;

        if self.board.wifi().set_enabled(on).is_err() {
            return;
        }

        self.enabled = on;
        self.remember_switch();

        if on {
            self.start_scan();
        } else {
            // Nothing to list and nothing to connect to: the page says so.
            self.access_points.clear();
            self.scan = ScanState::Idle;
            self.taken = false;
            self.cancel_prompt();
            self.read();
        }
    }

    /// Writes the switch down, if there is a network to write it beside.
    ///
    /// Nothing is written without one: `ssid` has no default, and a file that names no network is
    /// not a Wi-Fi file. A board that has never been on a network has no file for a switch to live
    /// in, and the first thing that makes one is the first connection that comes up.
    fn remember_switch(&self) {
        let mut wifi = self.board.wifi();

        let Some(mut saved) = wifi.saved() else {
            return;
        };

        if saved.enabled == self.enabled {
            return;
        }

        saved.enabled = self.enabled;

        if let Err(error) = wifi.remember(&saved) {
            eprintln!(
                "[wifi] the radio is {}, but the file says otherwise: {error}",
                self.enabled
            );
        }
    }

    /// Asks the radio for a scan.
    pub(crate) fn start_scan(&mut self) {
        if !self.enabled {
            return;
        }

        if self.board.wifi().scan_start().is_err() {
            self.scan = ScanState::Error;
            return;
        }

        self.scan = ScanState::Scanning;
        self.taken = false;
        self.read_at = None;
    }

    /// Reads the radio once, copying what it says into the page's state.
    pub(crate) fn read(&mut self) {
        let wifi = self.board.wifi();

        self.enabled = wifi.is_enabled();
        self.scan = wifi.scan_state();

        // `Done` stays `Done` until the next scan starts, so the list is taken once rather than
        // rebuilt four times a second for as long as the page is open.
        if self.scan == ScanState::Done && !self.taken {
            if let Ok(found) = wifi.scan_results() {
                let mut found = found;
                // Best signal first: the network a finger is looking for is at the top.
                found.sort_by_key(|ap| std::cmp::Reverse(ap.rssi));
                self.access_points = found;
                self.taken = true;
            }
        }

        self.status = wifi.status();
    }

    /// A frame arrived while the page is open: read the radio, a few times a second.
    pub(crate) fn poll(&mut self, now: Instant) {
        let due = match self.read_at {
            Some(last) => now.duration_since(last) >= READ_EVERY,
            None => true,
        };

        if !due {
            return;
        }

        self.read_at = Some(now);
        self.read();

        // A prompt that was waiting on the radio: the connection either came up, or it did not.
        if self.pending {
            eprintln!("[wifi] waiting — {}", self.answer());

            match self.status.state {
                WifiState::Connected => self.connected(),
                WifiState::Disconnected => {
                    // The driver clears the status when it gives up, so `status.ssid` is empty by the
                    // time this line runs and naming it that way printed `""`. The name comes from
                    // the list the attempt was made from — and while `pending` is set, the prompt is
                    // what says which entry that was.
                    let ssid = self
                        .prompt
                        .and_then(|index| self.access_points.get(index))
                        .map(|ap| ap.ssid.clone());

                    eprintln!("[wifi] gave up waiting for {ssid:?} — {}", self.answer());
                    self.pending = false;
                    self.attempt = None;
                    self.failed = true;
                }
                _ => {}
            }
        }
    }

    /// A network was pressed: connect to it, or ask for its password first.
    pub(crate) fn select(&mut self, index: usize) {
        let Some(ap) = self.access_points.get(index) else {
            return;
        };

        let (ssid, secure) = (ap.ssid.clone(), ap.secure);

        if !secure {
            self.request_connect(&ssid, "");
            return;
        }

        // A prompt for a network starts empty, whatever was typed for the last one — and hidden
        // again: a password left in the clear is not a state to inherit.
        self.password.clear();
        self.keyboard = KeyboardMode::Lower;
        self.revealed = false;
        self.failed = false;
        self.pending = false;
        self.attempt = None;
        self.prompt = Some(index);
    }

    /// A key of the prompt's keyboard.
    pub(crate) fn key(&mut self, action: KeyAction) {
        match action {
            KeyAction::Char(character) => self.password.push(character),
            KeyAction::Space => self.password.push(' '),
            KeyAction::Backspace => {
                self.password.pop();
            }
            KeyAction::SwitchMode(mode) => self.keyboard = mode,
            KeyAction::Enter => self.connect(),
        }
    }

    /// The prompt's connect button, and its return key.
    pub(crate) fn connect(&mut self) {
        let Some(index) = self.prompt else {
            return;
        };
        let Some(ssid) = self.access_points.get(index).map(|ap| ap.ssid.clone()) else {
            return;
        };

        let password = self.password.clone();

        self.request_connect(&ssid, &password);
    }

    /// Asks the radio to connect, and says what happened.
    fn request_connect(&mut self, ssid: &str, password: &str) {
        self.failed = false;

        // The length and not the password. The password itself was printed while the authmode
        // threshold was being chased — the question then was "did the box hold what was typed", and
        // a length cannot answer it. That question is answered, and what is left is the only secret
        // this app ever holds; an `eprintln!` on the board goes out on the UART, and a log that
        // travels ends up in a bug report.
        eprintln!(
            "[wifi] connect({ssid:?}, {} chars, prompt={})",
            password.chars().count(),
            self.prompt.is_some()
        );

        // The radio's answer is taken as a value: the borrow that asks for it lasts as long as the
        // call, and `read` below needs the board back.
        let asked = self.board.wifi().connect(ssid, password);

        if let Err(error) = asked {
            // Refused before it tried: no password on a locked network, or a network it does not
            // know. A prompt that is open is the thing that can say so; a tap on an open network
            // has nothing on screen to explain, so the list stands as it was.
            eprintln!("[wifi] refused before trying: {error:?}");
            self.read();
            eprintln!("[wifi] after the refusal — {}", self.answer());
            self.failed = self.prompt.is_some();
            return;
        }

        // Held until the radio answers, and written only if it answers well. Asked for by hand, so
        // it is wanted: connecting to a network once and then not offering it at boot would be a
        // strange thing to have remembered.
        self.attempt = Some(WifiCredentials {
            enabled: true,
            ssid: ssid.to_string(),
            password: password.to_string(),
            autoconnect: true,
        });

        self.pending = self.prompt.is_some();
        self.read();
        eprintln!(
            "[wifi] asked, pending={} — {}",
            self.pending,
            self.answer()
        );

        if self.pending && self.status.state == WifiState::Connected {
            // A board that connects in one step (the simulator does) is done in this message.
            self.connected();
        }
    }

    /// The radio's answer, as one line for the log.
    fn answer(&self) -> String {
        let status = &self.status;
        format!(
            "state={:?} ssid={:?} ip={:?} gateway={:?}",
            status.state, status.ssid, status.ip, status.gateway
        )
    }

    /// The connection came up: the prompt has nothing left to ask.
    fn connected(&mut self) {
        self.pending = false;
        self.failed = false;
        self.prompt = None;
        self.password.clear();
        self.keyboard = KeyboardMode::Lower;
        self.revealed = false;

        self.read();

        // The one moment the file is written: the radio said the password worked, so the password is
        // worth keeping. Written when it was *typed* instead, a mistyped one would destroy the one
        // that worked — which is what the NVS copy this replaces did.
        if let Some(credentials) = self.attempt.take() {
            if let Err(error) = self.board.wifi().remember(&credentials) {
                eprintln!("[wifi] connected, but the file could not be written: {error}");
            }
        }

        eprintln!("[wifi] connected — {}", self.answer());
    }

    /// Closes the prompt without connecting.
    pub(crate) fn cancel_prompt(&mut self) {
        self.prompt = None;
        self.password.clear();
        self.keyboard = KeyboardMode::Lower;
        self.revealed = false;
        self.failed = false;
        self.pending = false;
        self.attempt = None;
    }

    /// Leaves the network we are on.
    pub(crate) fn disconnect(&mut self) {
        let _ = self.board.wifi().disconnect();

        self.read();
    }

    /// Draws what has been typed in the clear, or as dots again.
    pub(crate) fn reveal(&mut self) {
        self.revealed = !self.revealed;
    }
}

/// The Wi-Fi page: the radio's switch, the scan, what it found, and the connection.
///
/// The password prompt is *not* part of the body: it is a `stack` over the whole page, because a
/// prompt is a modal — the list behind it must not take the finger. See [`password_prompt`].
pub(crate) fn wifi_page<'a>(preferences: SystemPreferences, on: bool, wifi: &Wifi) -> UI<'a> {
    let language = preferences.language;
    let theme = preferences.theme;
    // Built here and not twice: the page has two exits — radio off, and radio on — and both of them
    // are the same page with a different body under the same head.
    let head = Header::section(
        Icon::WIFI,
        SettingsSection::Wifi,
        language.text(Key::Wifi),
    )
    .view(theme);
    let mut parts: Vec<UI<'a>> = vec![switch_row(
        language.text(Key::Wifi),
        toggle(on, Message::ToggleWifi, theme),
        theme,
    )];

    if !on {
        parts.push(notice_card(language.text(Key::WifiOff), theme));

        return page(head, body(parts));
    }

    // What the radio is doing, and the way to ask for another one. A text button and not a card: a
    // card would give "scan again" the weight of the rows above and below it, and it is not a
    // setting — it is a request.
    let scanning = wifi.scan == ScanState::Scanning;
    parts.push(text_button(
        language.text(if scanning {
            Key::Scanning
        } else {
            Key::ScanAgain
        }),
        if scanning { None } else { Some(Message::WifiScan) },
        theme,
    ));

    parts.push(network_list(language, wifi, theme));

    if wifi.status.state == WifiState::Connected {
        parts.push(connection_card(language, &wifi.status, theme));
    }

    let pg = page(head, body(parts));

    // The prompt is not part of the page; it is a layer *over* it. One `Stack` for the two of them,
    // and the prompt's layer on top — see `password_prompt`.
    match password_prompt(preferences, wifi) {
        Some(prompt) => stack![pg, prompt].into(),
        None => pg,
    }
}

/// The networks the last scan found, best signal first — or a line saying there are none.
fn network_list<'a>(language: Language, wifi: &Wifi, theme: ThemeMode) -> UI<'a> {
    if wifi.access_points.is_empty() {
        return notice_card(language.text(Key::NoNetworks), theme);
    }

    let connected = if wifi.status.state == WifiState::Connected {
        Some(wifi.status.ssid.as_str())
    } else {
        None
    };

    let last = wifi.access_points.len() - 1;
    let mut rows: Vec<UI<'a>> = Vec::new();

    for (index, ap) in wifi.access_points.iter().enumerate() {
        rows.push(network_row(
            language,
            ap,
            connected == Some(ap.ssid.as_str()),
            index,
            theme,
        ));

        if index < last {
            rows.push(separator(theme));
        }
    }

    card_surface(Column::with_children(rows).width(Length::Fill), theme)
}

/// One network: its name, whether it is locked, whether we are on it, and its signal.
///
/// The lock is a *word* and not a padlock, for the same reason the keyboard's shift key says
/// `shift`: the font is a Chinese and Latin subset with no padlock in it. That is no longer the
/// whole story — `pomelo-material-symbols` is an icon font and `Icon::LOCK` is one call site away
/// — but swapping it is a change to this list's layout, not to this sentence.
fn network_row<'a>(
    language: Language,
    ap: &ApInfo,
    connected: bool,
    index: usize,
    theme: ThemeMode,
) -> UI<'a> {
    let mut right: Vec<UI<'a>> = Vec::new();

    if ap.secure {
        right.push(
            text(language.text(Key::Secured))
                .size(style::VALUE_FONT)
                .color(style::notice_for(theme))
                .into(),
        );
        right.push(Space::new().width(Length::Fixed(style::SIGNAL_GAP)).into());
    }

    if connected {
        right.push(
            text(language.text(Key::Connected))
                .size(style::VALUE_FONT)
                .color(style::accent())
                .into(),
        );
        right.push(Space::new().width(Length::Fixed(style::SIGNAL_GAP)).into());
    }

    right.push(signal_bars(ap.signal_bars(), theme));

    let contents = Row::with_children(vec![
        text(ap.ssid.clone())
            .size(style::LABEL_FONT)
            .color(style::label_for(theme))
            .into(),
        Space::new().width(Length::Fill).into(),
        Row::with_children(right)
            .align_y(Alignment::Center)
            .into(),
    ])
    .align_y(Alignment::Center)
    .width(Length::Fill);

    button(
        container(contents)
            .width(Length::Fill)
            .padding([style::ROW_PADDING_V, style::ROW_PADDING_H]),
    )
    .width(Length::Fill)
    .padding(0)
    .style(move |_theme, status| row_style(theme, status))
    .on_press(Message::WifiSelect(index))
    .into()
}

/// Four bars, the lit ones as tall as the signal — the same scale the launcher's status bar draws.
fn signal_bars<'a>(bars: u8, theme: ThemeMode) -> UI<'a> {
    let count = style::SIGNAL_BARS;
    let heights = style::SIGNAL_BAR_H - style::SIGNAL_BAR_MIN_H;

    let bars = (0..count).map(move |index| {
        let height =
            style::SIGNAL_BAR_MIN_H + heights * f32::from(index + 1) / f32::from(count);
        let lit = index < bars;

        container(Space::new())
            .width(Length::Fixed(style::SIGNAL_BAR_W))
            .height(Length::Fixed(height))
            .style(move |_theme| container::Style {
                background: Some(
                    if lit {
                        style::signal_on_for(theme)
                    } else {
                        style::signal_off_for(theme)
                    }
                    .into(),
                ),
                border: Border {
                    radius: 1.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into()
    });

    Row::with_children(bars)
        .spacing(style::SIGNAL_BAR_GAP)
        .align_y(Alignment::End)
        .into()
}

/// What is known about the connection: the network, the numbers that came with it, and the way out.
pub(crate) fn connection_card<'a>(
    language: Language,
    status: &WifiStatus,
    theme: ThemeMode,
) -> UI<'a> {
    let mut children = crate::pages::common::detail_rows(
        vec![
            (language.text(Key::Network), status.ssid.clone()),
            (language.text(Key::Signal), format!("{} dBm", status.rssi)),
            (language.text(Key::IpAddress), status.ip.clone()),
            (language.text(Key::Gateway), status.gateway.clone()),
            (language.text(Key::SubnetMask), status.netmask.clone()),
        ],
        theme,
    );

    children.push(separator(theme));
    children.push(action_row(
        language.text(Key::Disconnect),
        Some(Message::WifiDisconnect),
        theme,
    ));

    card_surface(Column::with_children(children).width(Length::Fill), theme)
}

/// The password prompt, when one is open: the network, what has been typed, and the keyboard.
///
/// It is the whole screen — a dimmed backdrop that takes the finger, with the card on it — because
/// that is what makes the list behind it unreachable. `None` when no prompt is open, which is the
/// page as it stands.
/// The password prompt, when one is open: the sheet at the foot of the panel, and the dimming that
/// keeps the page behind it out of reach.
///
/// `None` when no prompt is open, which is the page as it stands.
///
/// One layer: the dimming and the sheet are one `container`, because the rectangle that covers the
/// page is the same rectangle the sheet stands in.
fn password_prompt<'a>(preferences: SystemPreferences, wifi: &Wifi) -> Option<UI<'a>> {
    let ap = wifi.prompt.and_then(|index| wifi.access_points.get(index))?;
    let language = preferences.language;
    let theme = preferences.theme;

    // The two answers, at the head of the sheet, one at each end. Glyphs and not words: the shape is
    // the capsule the page's back button is, a cross and a tick need no translating, and the pair of
    // them says which is which without a sentence.
    let answers = Row::with_children(vec![
        capsule_button(Icon::CLOSE, Some(Message::WifiCancel), dismiss_style, theme),
        Space::new().width(Length::Fill).into(),
        capsule_button(
            Icon::CHECK,
            if wifi.pending {
                None
            } else {
                Some(Message::WifiConnect)
            },
            confirm_style,
            theme,
        ),
    ])
    .width(Length::Fill);

    let mut children: Vec<UI<'a>> = vec![
        answers.into(),
        Space::new().height(Length::Fixed(style::PROMPT_GAP)).into(),
        text(ap.ssid.clone())
            .size(style::NAV_FONT)
            .color(style::label_for(theme))
            .into(),
        Space::new()
            .height(Length::Fixed(style::PROMPT_GAP))
            .into(),
        // One row and not two: a tile, the same row every list in this app is built from, with the
        // label on the left and the field on the right. `Tile::field` and not `Tile::action`,
        // because there is nothing behind this row to open and nothing for a press to do.
        Tile::field(language.text(Key::Password))
            .secondary(password_line(&wifi.password, wifi.revealed, theme))
            .view(theme),
    ];

    if wifi.failed {
        children.push(
            Space::new()
                .height(Length::Fixed(style::PROMPT_GAP))
                .into(),
        );
        children.push(
            text(language.text(Key::ConnectFailed))
                .size(style::VALUE_FONT)
                .color(style::error())
                .into(),
        );
    }

    // No gap above the button: `text_button` carries `ROW_PADDING_V` over its own label, and the
    // tile above carries as much under it, so a `Space` here would be a third helping of the same
    // 16 pixels — and the keys at the foot of the sheet are what pays for it.
    children.push(text_button(
        language.text(if wifi.revealed { Key::Hide } else { Key::Show }),
        Some(Message::WifiReveal),
        theme,
    ));

    // The question, with the panel's margins on both sides. It takes whatever the band under it does
    // not, and its bottom edge is where the keys begin — the keys have no margin of their own, so the
    // width and every pixel of slack above them belong to them.
    //
    // `Fill`, so the keys sit on the panel's floor and stay exactly `keyboard_height()` tall whatever
    // the question above them says.
    //
    // Nothing *below* the question, either: the text button at its foot carries `ROW_PADDING_V` of
    // its own for the finger to land on, and a second margin under that is 16 px of nothing between
    // it and the keys.
    let half = container(Column::with_children(children).width(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: style::PROMPT_PADDING,
            bottom: 0.0,
            left: style::PROMPT_PADDING,
            right: style::PROMPT_PADDING,
        });

    // The band's own height and not the sheet's leftovers. The formula is the keyboard crate's, so
    // this is the same keyboard the terminal draws at the same size on the same panel — see
    // `style::keyboard_height`. A `Fill` here would make the keys whatever the question left them,
    // which is a different keyboard on every password.
    let keys = container(touch_keyboard::band_with_theme(
        wifi.keyboard,
        Message::WifiKey,
        theme,
    ))
    .width(Length::Fill)
    .height(Length::Fixed(style::keyboard_height()));

    let sheet = container(
        Column::with_children(vec![half.into(), keys.into()])
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(style::card_for(theme).into()),
        border: Border {
            color: style::card_border_for(theme),
            width: style::CARD_BORDER_WIDTH,
            // Rounded at the top only: the other three edges are the panel's own, so there is no
            // corner there to round.
            radius: Radius {
                top_left: style::PROMPT_RADIUS,
                top_right: style::PROMPT_RADIUS,
                ..Radius::default()
            },
        },
        ..container::Style::default()
    });

    // The dimming, and the whole of the modal. No margin on the sides or the floor: the sheet is as
    // wide as the panel and stands on it. The only gap is the one at the top, and the page showing
    // through it is what says this is a layer *over* the page rather than the page.
    //
    // `opaque` is what takes the finger. It captures any press inside its bounds, and its bounds are
    // the whole panel, so the list behind — a screen of buttons — never sees it.
    Some(opaque(
        container(sheet)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding {
                top: style::PROMPT_TOP_GAP,
                bottom: 0.0,
                left: 0.0,
                right: 0.0,
            })
            .style(|_theme| container::Style {
                background: Some(style::backdrop().into()),
                ..container::Style::default()
            }),
    ))
}

/// What has been typed: dots and a caret, or the password itself once it is revealed.
fn password_line<'a>(password: &str, revealed: bool, theme: ThemeMode) -> UI<'a> {
    if revealed {
        return Row::with_children(vec![
            text(password.to_string())
                .size(style::DETAIL_FONT)
                .color(style::label_for(theme))
                .into(),
            Space::new().width(Length::Fixed(style::CARET_GAP)).into(),
            caret(),
        ])
        .align_y(Alignment::Center)
        .height(Length::Fixed(style::LINE_H))
        .into();
    }

    let dot_color = style::label_for(theme);
    let dots: Vec<UI<'a>> = password
        .chars()
        .map(move |_| {
            container(Space::new())
                .width(Length::Fixed(style::PASSWORD_DOT))
                .height(Length::Fixed(style::PASSWORD_DOT))
                .style(move |_theme| container::Style {
                    background: Some(dot_color.into()),
                    border: Border {
                        radius: (style::PASSWORD_DOT / 2.0).into(),
                        ..Border::default()
                    },
                    ..container::Style::default()
                })
                .into()
        })
        .collect();

    // The dots in a row of their own, so the caret can stand closer to the last one than two dots
    // stand to each other.
    let dots = Row::with_children(dots)
        .spacing(style::PASSWORD_DOT_GAP)
        .align_y(Alignment::Center);

    Row::with_children(vec![
        dots.into(),
        Space::new().width(Length::Fixed(style::CARET_GAP)).into(),
        caret(),
    ])
    .align_y(Alignment::Center)
    .height(Length::Fixed(style::LINE_H))
    .into()
}

/// The caret that says where the next character goes.
///
/// Painted, not typed: the font has no `•` and no block, so what marks the cursor is a rectangle —
/// the same choice the terminal makes for its input line.
///
/// It hangs from the foot of the line rather than sitting in its middle. A line box carries an
/// ascender and a descender and the letters only use the part between them, so a caret centred in the
/// box floats above the letters it is meant to stand among. The `CARET_DROP` under it is that
/// descender, given back.
fn caret<'a>() -> UI<'a> {
    let bar = container(Space::new())
        .width(Length::Fixed(style::CARET_W))
        .height(Length::Fixed(style::CARET_H))
        .style(|_theme| container::Style {
            background: Some(style::accent().into()),
            ..container::Style::default()
        });

    // A `Column` and not a container with a bottom-aligned child: the row the caret goes into is
    // `LINE_H` tall, the bar is at the column's foot, and the column's own bottom margin is what
    // holds the bar up off the line's floor.
    Column::with_children(vec![
        Space::new().height(Length::Fill).into(),
        bar.into(),
        Space::new().height(Length::Fixed(style::CARET_DROP)).into(),
    ])
    .height(Length::Fill)
    .into()
}
