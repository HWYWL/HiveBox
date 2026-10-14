//! The terminal, built from iced widgets -- **a standard iced program**.
//!
//! The shell -- the model, the commands, the storage helpers -- is [`shell`], which the original
//! terminal used too, because two shells that disagree with each other is a bug waiting for a user.
//! What is here is the face: the VS Code transcript, the active input line with its cursor, and the
//! iOS on-screen keyboard — which `pomelo-widgets` draws, because the settings app draws the same
//! one for a Wi-Fi password.
//!
//! The input line is drawn and driven by this app on purpose. `iced::widget::text_input` assumes a
//! physical keyboard and a focus system; on a panel with neither, the app owns the string (in
//! [`shell`]) and the keys write to it, exactly as the original did.
//!
//! # The size comes from the platform
//!
//! The keyboard band and the wrapping width are derived from how big the screen is, so the app has
//! to be told. It is told the way a real terminal is: [`Message::Resized`], from
//! [`Terminal::subscription`], which subscribes to `iced::window::resize_events()`. On a desktop
//! that is the window being resized; on the panel the platform sends the same message with the
//! panel's size (the launcher does it when it opens the terminal). Between the two the app starts
//! at [`SCREEN`], the design size, so it draws something sensible even if nobody ever tells it.
//!
//! # Two shells
//!
//! The shell above is this app's own, and a board is not needed for it. Since the board can reach a NAS
//! there is a second one: a session on a machine across the network, which is [`remote`] and which the
//! app is *pointed at* rather than starting in — [`Terminal::connect`], from the NAS monitor's page,
//! through the launcher. Same app, same keyboard, same frame, because the frame around a shell is the
//! frame around a shell; what is different is that the page is the machine's and arrives as bytes.

mod commands;
mod remote;
mod screen;
mod shell;

pub mod style;

use std::sync::Arc;

use iced::theme::Palette;
use iced::time::Instant;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{button, container, text, Column, Row, Scrollable, Space};
use iced::{Alignment, Color, Element, Length, Padding, Size, Subscription, Theme};
use pomelo_hal::{Board, NasCredentials, NasFault, NasShellStatus};
use pomelo_widgets::preferences::{SystemPreferences, ThemeMode};
use pomelo_widgets::touch_keyboard::KeyboardMode;

use crate::remote::Remote;
use crate::shell::{HistoryEntry, Rgb, TerminalModel};

pub use shell::KeyAction;
pub use style::SCREEN;

/// An element this app builds. Everything is owned, so the helpers do not borrow the app.
type El = Element<'static, Message>;

/// What the terminal reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// A key of the on-screen keyboard was pressed.
    Key(KeyAction),
    /// The screen changed size: a window on a desktop, the panel on the board.
    Resized(Size),
    /// A frame has been drawn, and its instant is what the cursor's blink is measured against.
    ///
    /// Only a page with something of its own to move asks for these — a session that may have said
    /// something, and a cursor that comes and goes — so see [`Terminal::subscription`] for when one
    /// is asked for.
    Tick(Instant),
    /// Opens the session again on the machine it was opened on.
    Reconnect,
    /// Leaves the session and goes back to the shell in this app.
    Leave,
}

/// The terminal.
pub struct Terminal {
    model: TerminalModel,
    /// The board the app can reach machines through, when it has been given one: `None` on a desktop
    /// window started on its own, and on a host that wired no NAS. A terminal with no board is a
    /// terminal with no machines to reach, which is the state this app's own shell needs nothing for.
    board: Option<Arc<Board>>,
    /// The session on a machine, when somebody has pointed the app at one — see
    /// [`Terminal::connect`]. While it is here it is what the page shows, and the shell above keeps
    /// its own transcript until somebody leaves and comes back to it.
    session: Option<Remote>,
    /// Which face of the keyboard is showing. The app's and not the model's, because both shells type
    /// on the same keyboard; the model is still told, so that it stays consistent with itself.
    keyboard_mode: KeyboardMode,
    /// Whether the cursor is drawn this half of its blink. Flipped by [`Terminal::blink`], and it
    /// starts showing, because the line somebody has just been shown is one they can type at.
    cursor_showing: bool,
    /// When the blink last changed its mind, which is what the next change is measured from.
    ///
    /// A time and not a count of frames: what wakes this app up is a frame, and frames are not evenly
    /// spaced — a desktop window sends sixty a second, a board's loop one per turn of whatever it is
    /// doing — so counting them would blink at whatever speed the screen happened to be running at.
    cursor_flipped: Instant,
    /// The screen the app is drawing into. The keyboard's geometry and the wrapping width are
    /// derived from it, as the original's `MediaQuery` was. It starts at the design size and is
    /// replaced by whoever tells the app the truth -- see the module documentation.
    size: Size,
    preferences: SystemPreferences,
}

impl Terminal {
    pub fn new() -> Self {
        Self {
            model: TerminalModel::new(),
            board: None,
            session: None,
            keyboard_mode: KeyboardMode::Lower,
            cursor_showing: true,
            cursor_flipped: Instant::now(),
            size: Size::new(SCREEN as f32, SCREEN as f32),
            preferences: SystemPreferences::default(),
        }
    }

    /// The same terminal, with a board to reach machines through.
    ///
    /// A builder rather than an argument to [`Terminal::new`], because a terminal is useful without
    /// one: the local shell is the app's own and a desktop window that was never given a board still
    /// has it. What a board is *for* here is one thing, and it is [`Terminal::connect`].
    pub fn with_board(mut self, board: Arc<Board>) -> Self {
        self.board = Some(board);
        self
    }

    /// Opens a session on `device` and shows it.
    ///
    /// This is the whole of the remote mode's entry: the app is pointed at a machine and the page
    /// becomes that machine's. Nothing is closed first — a session replaces a session, and the backend
    /// drops the old one when the new one opens — and a `connect` with no board does nothing, because
    /// there is nothing to open a session through.
    pub fn connect(&mut self, device: &NasCredentials) {
        let Some(board) = &self.board else {
            return;
        };

        let (columns, rows) = self.grid();
        let mut nas = board.nas();
        self.session = Some(Remote::open(&mut **nas, device, columns, rows));
    }

    /// The session, when there is one. Read by the host and by the tests.
    pub fn session(&self) -> Option<&Remote> {
        self.session.as_ref()
    }

    /// Leaves the session and goes back to the shell in this app.
    ///
    /// The machine is told, and not just forgotten: the session on the other end is a login, and one
    /// left to time out on a server is somebody else's process for as long as that takes. The page goes
    /// with the session — the transcript of *this* app's shell is still there and is what comes back.
    pub fn leave(&mut self) {
        if self.session.take().is_some() {
            if let Some(board) = &self.board {
                let _ = board.nas().shell_close();
            }
        }
    }

    /// Opens the session again on the machine it was opened on.
    ///
    /// The machine is looked up in the board's list rather than remembered here, for the reason
    /// [`Remote`] does not keep the password: what is remembered is what it takes to name the machine
    /// on a page, and the credentials that reach it are the backend's to hold. A machine that has been
    /// dropped from the list since is one this cannot reconnect to, and nothing happens — the page it
    /// was on is left as it is, because throwing away a scrollback somebody was reading to say "no" is
    /// worse than a key that does nothing.
    fn reconnect(&mut self) {
        let (Some(board), Some(host)) = (
            &self.board,
            self.session
                .as_ref()
                .map(|session| session.host().to_string()),
        ) else {
            return;
        };

        let device = board
            .nas()
            .devices()
            .into_iter()
            .find(|device| device.host == host);

        if let Some(device) = device {
            self.connect(&device);
        }
    }

    /// Returns the system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// The current theme mode.
    pub fn theme_mode(&self) -> ThemeMode {
        self.preferences.theme
    }

    /// Sets the theme mode.
    pub fn set_theme_mode(&mut self, theme: ThemeMode) {
        self.preferences.theme = theme;
    }

    /// The app's subscriptions: the size of the screen, and — while there is a cursor to blink — a
    /// clock.
    ///
    /// The local shell used to say it needed no clock, and that was the truth for as long as a cursor
    /// was a block that sat still: the page changed when it was typed into and when the screen changed
    /// shape, and there was nothing to wake up for. The cursor is that nothing. It goes and comes back
    /// on half a second of its own — see [`Terminal::blink`] — and the only way to be drawing when it
    /// does is to be woken up. Both shells have one, because a line being typed at has a cursor under
    /// it wherever that line is.
    ///
    /// A session needs the same clock for a second reason. Output arrives when the *machine* decides
    /// to send it — a `docker pull` finishing, a `tail -f`, a `wall` from somebody else — so a page
    /// that only redrew on a key press would sit there looking finished while bytes waited in the
    /// backend.
    ///
    /// What stops the clock is a page with nothing moving on it, and there is exactly one: a session
    /// that has hung up or been lost is a page to *read*, with no prompt on it and so no cursor under
    /// one. That is the same shape as the wifi page waiting for a scan and the games app asking for
    /// frames, and for the same reason — a frame is not free, and an app with nothing left to draw
    /// should not be asking for one.
    ///
    /// A frame is what a blink is drawn on, and the bill for one is paid only while the terminal is the
    /// screen: the launcher subscribes to this only while it is, and a page with nothing moving on it is
    /// not. A host that sent no frames at all would leave the cursor in whichever half of its blink it
    /// was in, because the instant a frame carries is what the blink is measured against and this stack
    /// has no other clock in it.
    pub fn subscription(&self) -> Subscription<Message> {
        let size = iced::window::resize_events().map(|(_window, size)| Message::Resized(size));

        if self.wants_a_clock() {
            Subscription::batch([size, iced::window::frames().map(Message::Tick)])
        } else {
            size
        }
    }

    /// The screen this app is laying out for.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Whether the app is asking to be woken up between messages.
    ///
    /// True while there is a cursor on the page and false otherwise, and named as a question rather
    /// than as a subscription because that is the *decision*: a clock is what a page needs when
    /// something on it changes on nobody's message, and a blinking cursor is such a thing wherever it
    /// is drawn — at the end of a session's prompt, and at the end of this app's own. What a clock is
    /// also needed for is a session's output, which arrives when the machine decides to send it. The
    /// subscription above asks for one when this says so, and a test can read this without a
    /// subscription's `Debug`, which is a bare `Subscription` and tells nothing.
    ///
    /// A session that has hung up is the one page that asks for nothing: there is no prompt on it and
    /// no cursor under one, and a machine that has said goodbye is not about to say anything either.
    pub fn wants_a_clock(&self) -> bool {
        match &self.session {
            Some(session) => session.is_live(),
            None => true,
        }
    }

    /// Whether the cursor is drawn this half of its blink.
    ///
    /// Read by the tests, which is the only place a blink can be watched without watching a screen.
    pub fn cursor_showing(&self) -> bool {
        self.cursor_showing
    }

    /// The transcript as plain text lines: a prompt renders as `rust:cwd$ cmd`, an output line as
    /// its text. Read by the host and by the tests.
    ///
    /// Of whichever shell is showing, which is what makes it worth asking a *host*: a session's page is
    /// the machine's text, and the app's own shell is this app's.
    pub fn history_lines(&self) -> Vec<String> {
        match &self.session {
            Some(session) => session.screen().lines(),
            None => self.model.history_lines(),
        }
    }

    /// The line being typed, wherever it is being typed. Read by the host and by the tests.
    pub fn current_input(&self) -> &str {
        match &self.session {
            Some(session) => session.input(),
            None => self.model.input(),
        }
    }

    /// The current working directory. Read by the host and by the tests.
    ///
    /// This app's own shell's, always: a session's directory is the *machine's*, and this app cannot see
    /// it — a terminal does not parse what a shell at the other end does with a `cd`, and the prompt on
    /// the page is the only place it shows. What a host wants of a session is
    /// [`history_lines`][Terminal::history_lines].
    pub fn cwd(&self) -> &str {
        self.model.cwd()
    }

    /// The keyboard band's height: about 48% of the screen, clamped to 180..240 px.
    ///
    /// The formula is the keyboard crate's, so the band a password prompt gets in the settings app
    /// is the same share of the same screen.
    pub fn keyboard_height(&self) -> f32 {
        style::keyboard_height(self.size.height)
    }

    /// One line of the transcript, in its fixed 24 px cell.
    fn cell(child: El) -> El {
        container(child)
            .width(Length::Fill)
            .height(Length::Fixed(style::LINE_HEIGHT))
            .align_y(Alignment::Center)
            .into()
    }

    /// The `pomelo:/internal$ ` prompt's coloured parts, without what follows.
    fn prompt_prefix(short_cwd: String, theme_mode: ThemeMode) -> Vec<El> {
        vec![
            label(shell::DEFAULT_USER, style::prompt_user_for(theme_mode)),
            label(":", style::prompt_punct_for(theme_mode)),
            label(short_cwd, style::prompt_dir_for(theme_mode)),
            label("$ ", style::prompt_sym_for(theme_mode)),
        ]
    }

    /// The visible tail of the input line and the cursor after it.
    fn input_tail(&self) -> El {
        let theme_mode = self.theme_mode();
        let short_cwd = self.model.short_cwd();
        let prompt_w = style::measure(&format!("{}:{short_cwd}$ ", shell::DEFAULT_USER));
        let max_text_w = self.text_width();
        let avail = (max_text_w - prompt_w - style::CURSOR_WIDTH - 8.0).max(40.0);
        let visible = visible_tail(self.model.input(), avail);

        let mut parts = Self::prompt_prefix(short_cwd, theme_mode);
        parts.push(
            Row::with_children(vec![
                label(visible, style::command_text_for(theme_mode)),
                self.cursor(),
            ])
            .spacing(2.0)
            .align_y(Alignment::Center)
            .into(),
        );
        Row::with_children(parts).align_y(Alignment::Center).into()
    }

    /// The cursor: a bar standing where the next character typed will land, which is where it is drawn
    /// on the half of its blink that shows and nowhere at all on the other half.
    ///
    /// Shared by both shells, because a cursor is a cursor: the one the app draws at the end of its own
    /// prompt is the same bar the app draws at the end of a session's prompt, and the only reason it is
    /// a method rather than a line in `input_tail` is that there are now two lines that need it.
    ///
    /// The half that shows nothing is drawn as a *transparent* bar and not as no bar at all, so that the
    /// row it sits in measures the same whether it is there or not: take the two pixels out and the line
    /// being typed jerks sideways twice a second, which is worse than no blink.
    fn cursor(&self) -> El {
        let cursor_color = if self.cursor_showing {
            style::command_text_for(self.theme_mode())
        } else {
            Color::TRANSPARENT
        };

        container(
            container(Space::new())
                .width(Length::Fixed(style::CURSOR_WIDTH))
                .height(Length::Fixed(style::CURSOR_HEIGHT))
                .style(move |_theme| container::Style {
                    background: Some(cursor_color.into()),
                    ..container::Style::default()
                }),
        )
        .padding(Padding {
            top: style::CURSOR_OFFSET_Y,
            bottom: 0.0,
            left: 0.0,
            right: 0.0,
        })
        .into()
    }

    /// The width the transcript wraps into: the screen minus the safe inset.
    fn text_width(&self) -> f32 {
        let pad = style::PAGE_PADDING.min(self.size.width * 0.05);
        (self.size.width - pad * 2.0).max(180.0)
    }

    /// The transcript: every history line, then the active input line.
    ///
    /// Wrapping is this UI's job — `shell::wrap_line` takes the measurement as a parameter — so
    /// one raw output entry becomes as many 24 px cells as it needs.
    fn transcript(&self) -> Vec<El> {
        let theme_mode = self.theme_mode();
        let max_text_w = self.text_width();
        let mut lines = Vec::with_capacity(self.model.history.len() + 1);

        for entry in &self.model.history {
            match entry {
                HistoryEntry::Prompt { cwd, cmd } => {
                    let mut parts = Self::prompt_prefix(cwd.clone(), theme_mode);
                    parts.push(label(cmd.clone(), style::command_text_for(theme_mode)));
                    lines.push(Self::cell(
                        Row::with_children(parts).align_y(Alignment::Center).into(),
                    ));
                }
                HistoryEntry::Output(text) => {
                    let text_color = style::text_fg_for(theme_mode);
                    for line in shell::wrap_line(text, max_text_w, &style::measure) {
                        lines.push(Self::cell(label(line, text_color)));
                    }
                }
            }
        }

        lines.push(Self::cell(self.input_tail()));
        lines
    }

    /// The machine's page, as the cells it is drawn in.
    ///
    /// Drawn as the machine left it: one character per cell, runs of one colour merged, a line the
    /// machine left empty kept empty. What this app adds is the *end* of it — where the cursor is, and
    /// what has been typed since — because that end is this side of the wire: the machine has not heard
    /// the line yet.
    fn session_page(&self, session: &Remote) -> Vec<El> {
        let theme_mode = self.theme_mode();
        let max_text_w = self.text_width();
        let default = style::text_fg_for(theme_mode);
        let page = session.screen().runs();
        // Only a session that is up has a prompt to type at. A page that has been lost, or has said
        // goodbye, is a page to *read*, and a cursor blinking at the end of it would be a cursor in a
        // terminal that is not there any more.
        let typing = session.status() == NasShellStatus::Ready;
        let mut lines: Vec<El> = Vec::with_capacity(page.len());

        for (index, runs) in page.iter().enumerate() {
            let at_the_prompt = typing && index + 1 == page.len();
            // Room is kept on the line the input will be typed at, so that the machine's own line and
            // this app's do not push each other off the screen. Capped at a share of the width: a line
            // being typed shows its tail rather than its head, which is the trade a terminal makes —
            // the prompt it is being typed at stays visible.
            let reserved = if at_the_prompt {
                (style::measure(session.input()) + style::CURSOR_WIDTH + 8.0).min(max_text_w * 0.6)
            } else {
                0.0
            };

            let pieces = wrap_runs(runs, max_text_w - reserved, &style::measure);
            let count = pieces.len();

            for (piece_index, run) in pieces.into_iter().enumerate() {
                let width: f32 = run.iter().map(|(text, _)| style::measure(text)).sum();
                let mut parts: Vec<El> = run
                    .into_iter()
                    .map(|(text, color)| label(text, color.map_or(default, style::color)))
                    .collect();

                if at_the_prompt && piece_index + 1 == count {
                    let room = (max_text_w - width - style::CURSOR_WIDTH - 8.0).max(40.0);
                    parts.push(
                        Row::with_children(vec![
                            label(
                                visible_tail(session.input(), room),
                                style::command_text_for(theme_mode),
                            ),
                            self.cursor(),
                        ])
                        .spacing(2.0)
                        .align_y(Alignment::Center)
                        .into(),
                    );
                }

                // No spacing between the runs of a line: one line of `ls` is as many runs as it has
                // colours, and a gap between each of them would space the machine's own output out.
                lines.push(Self::cell(
                    Row::with_children(parts).align_y(Alignment::Center).into(),
                ));
            }
        }

        lines
    }

    /// What is said under the page, when there is something to say about the session itself.
    ///
    /// Nothing while a session is up: the page is the machine's, and a terminal that narrated its own
    /// health would be covering the thing somebody is reading. Everything else is a state somebody has
    /// to act on — a login in flight, a machine that is off, a shell somebody left — and it comes with
    /// the one or two things they can do about it.
    fn status_line(&self, session: &Remote) -> Option<El> {
        let theme_mode = self.theme_mode();
        let (words, color) = match session.status() {
            NasShellStatus::Ready => return None,
            NasShellStatus::Opening => (
                format!("Logging in to {}@{}…", session.user(), session.host()),
                style::muted_for(theme_mode),
            ),
            NasShellStatus::Closed => (
                format!("The session on {} ended.", session.host()),
                style::muted_for(theme_mode),
            ),
            NasShellStatus::Lost(fault) => (
                fault_words(fault, session.host()),
                style::error_for(theme_mode),
            ),
        };

        let mut actions: Vec<El> = Vec::new();
        // Not while a login is in flight: there is nothing to do about one but wait, and the backend
        // resolves it on its own. Not after a board has said it has no SSH client either — asking again
        // cannot grow one, and a button that cannot work is worse than no button.
        if !session.is_live() && session.status() != NasShellStatus::Lost(NasFault::Unsupported) {
            actions.push(text_button("Reconnect", Message::Reconnect, color));
        }
        if !session.is_live() {
            actions.push(text_button("Leave", Message::Leave, color));
        }

        // The words get a line of their own and the actions the line below, right-aligned. Not a row of
        // words-then-buttons, which is what a wide window wants and what a 480 px panel cannot have: a
        // sentence about a machine plus two buttons is wider than the screen, and iced lays a row that
        // does not fit out past the edge, where a finger cannot reach it.
        let mut lines: Vec<El> = vec![label(words, color)];
        if !actions.is_empty() {
            actions.insert(0, Space::new().width(Length::Fill).into());
            lines.push(
                Row::with_children(actions)
                    .spacing(8.0)
                    .align_y(Alignment::Center)
                    .into(),
            );
        }

        Some(
            container(Column::with_children(lines).spacing(2.0).width(Length::Fill))
                .width(Length::Fill)
                .padding(Padding {
                    top: 4.0,
                    right: style::PAGE_PADDING.min(self.size.width * 0.05),
                    bottom: 6.0,
                    left: style::PAGE_PADDING.min(self.size.width * 0.05),
                })
                .into(),
        )
    }

    /// The transcript viewport, scrolled to the bottom.
    fn viewport(&self, content: Vec<El>) -> El {
        let pad = style::PAGE_PADDING.min(self.size.width * 0.05);
        let content = Column::with_children(content).width(Length::Fill);

        let scroller = Scrollable::new(content)
            .direction(Direction::Vertical(Scrollbar::hidden()))
            .anchor_bottom()
            .width(Length::Fill)
            .height(Length::Fill);

        container(scroller)
            .padding(Padding {
                top: 20.0,
                right: pad,
                bottom: 4.0,
                left: pad,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// The on-screen keyboard, anchored to the bottom.
    ///
    /// The band is the shared keyboard's: the same four rows, keys and palette the settings app
    /// draws for a Wi-Fi password. What is the terminal's own here is the string the keys write
    /// into (`Message::Key` → the model), the mode it remembers, and the *height* — a keyboard
    /// fills the box it is given, so how tall that box is, is stated here on the layout and not
    /// passed into the widget.
    fn keyboard(&self) -> El {
        container(pomelo_widgets::touch_keyboard::band_with_theme(
            self.keyboard_mode,
            Message::Key,
            self.theme_mode(),
        ))
        .height(Length::Fixed(self.keyboard_height()))
        .into()
    }

    /// How many cells the page has, as the machine is told when a session opens.
    ///
    /// An estimate, and it has to be: this app draws in a proportional font, so a cell is not a fixed
    /// width and `style::measure` measures text rather than grid positions. See [`crate::screen`], which
    /// drops cursor addressing for the same reason. What the number is *for* is a shell at the other end
    /// wrapping its output, and a shell that wraps a little early is a page with room to spare, while one
    /// that wraps late is a page that scrolls sideways. So the cell is taken generously and the count
    /// comes out low.
    ///
    /// Asked once, when the session opens: the backend has no way to be told a new size, so a window
    /// resized mid-session keeps the columns it was opened with until it is reconnected.
    fn grid(&self) -> (u16, u16) {
        const CELL: f32 = style::FONT_SIZE * 0.62;

        let transcript = (self.size.height - self.keyboard_height() - 24.0).max(style::LINE_HEIGHT);
        let columns = (self.text_width() / CELL).floor().clamp(20.0, 500.0) as u16;
        let rows = (transcript / (style::FONT_SIZE * 1.4)).floor().clamp(2.0, 500.0) as u16;

        (columns, rows)
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Terminal {
    /// The theme: the palette and the background the platform paints behind the tree.
    pub fn theme(&self) -> Theme {
        // A solid background, not a wallpaper primitive -- see the launcher's theme for the
        // measurement that made this the rule: the compositor paints the background over the
        // damage rectangle only, while a full-screen primitive costs the whole screen every frame.
        let theme_mode = self.theme_mode();
        if theme_mode.is_light() {
            Theme::custom(
                "PomeloLight",
                Palette {
                    background: style::background_for(theme_mode),
                    ..Palette::LIGHT
                },
            )
        } else {
            Theme::custom(
                "Pomelo",
                Palette {
                    background: style::background_for(theme_mode),
                    ..Palette::DARK
                },
            )
        }
    }

    /// Reacts to one message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Key(action) => self.key(action),
            Message::Resized(size) => self.size = size,
            Message::Tick(at) => {
                self.blink(at);
                self.poll();
            }
            Message::Reconnect => self.reconnect(),
            Message::Leave => self.leave(),
        }
    }

    /// Types one key at whichever shell is showing.
    ///
    /// The keyboard's mode is *this* app's, because there is one keyboard and both shells type on it.
    /// The model is told as well, so that it stays consistent with itself for anything that asks it —
    /// and nothing does during a session, because nothing but the page is on screen.
    fn key(&mut self, action: KeyAction) {
        if let KeyAction::SwitchMode(mode) = action {
            self.keyboard_mode = mode;
        }

        match (&mut self.session, &self.board) {
            (Some(session), Some(board)) => {
                let mut nas = board.nas();
                session.key(&mut **nas, action);
            }
            // A session with no board cannot happen — a board is what opens one — but if it somehow did,
            // the honest thing is to drop the key rather than to type it into the app's own shell, which
            // is not what is on the page.
            (Some(_), None) => {}
            (None, _) => self.model.handle_key_action(action),
        }
    }

    /// Takes whatever the machine has said, if there is a session that is still worth asking.
    ///
    /// Not asked once the session is over. A backend keeps the last word on a session that was lost or
    /// that said goodbye, so asking again would be harmless — but it would also be this app keeping a
    /// conversation going with a machine it had finished with, once per frame, for as long as the page
    /// stayed open.
    fn poll(&mut self) {
        let (Some(session), Some(board)) = (&mut self.session, &self.board) else {
            return;
        };

        if session.is_live() {
            let mut nas = board.nas();
            session.poll(&mut **nas);
        }
    }

    /// Flips the cursor's showing half once half a second has gone by since the last time it did.
    ///
    /// Measured against the frame's own instant rather than counted from one frame to the next, because
    /// frames are what this app is woken up by and they arrive at whatever rate the host has: sixty a
    /// second in a desktop window, one per turn of a board's loop, and as many as it likes in between.
    /// A counter would have the blink run at the speed of the screen; an instant has it run at the speed
    /// of the clock, and a host that wakes the app up rarely gets a cursor that is on for longer rather
    /// than one that never blinks at all.
    ///
    /// Frames that arrive inside the half are dropped rather than accumulated, so the blink does not
    /// drift with how many of them there were — and a host that stops sending frames (a page nobody is
    /// looking at) does not come back to a blink with catching up to do.
    fn blink(&mut self, at: Instant) {
        if at.duration_since(self.cursor_flipped) >= style::CURSOR_BLINK {
            self.cursor_flipped = at;
            self.cursor_showing = !self.cursor_showing;
        }
    }

    /// Describes the interface for the current state.
    ///
    /// The parameter is the elided lifetime, not [`El`]: everything below is owned, so the element
    /// could be `'static`, but iced asks for `for<'a> fn(&'a State) -> Element<'a, _>` and a
    /// `'static` return does not satisfy that bound. It coerces here instead.
    pub fn view(&self) -> Element<'_, Message> {
        let bg = style::background_for(self.theme_mode());

        // The page is one of two, and which one is not a mode the app is in: it is whether there is a
        // session. Nothing else in the frame changes with it — the same keyboard, the same band at the
        // bottom, the same window around the same transcript.
        let (page, status) = match &self.session {
            Some(session) => (self.session_page(session), self.status_line(session)),
            None => (self.transcript(), None),
        };

        let mut parts = vec![self.viewport(page)];
        parts.extend(status);
        parts.push(self.keyboard());

        container(Column::with_children(parts))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(bg.into()),
                ..container::Style::default()
            })
            .into()
    }
}

/// One coloured label of the prompt or the transcript.
fn label(content: impl Into<String>, color: Color) -> El {
    text(content.into())
        .size(style::FONT_SIZE)
        .color(color)
        .into()
}

/// One line of a session's page, broken into pieces that fit the page's width.
///
/// Broken *anywhere* rather than at spaces, and that is the difference between this and the wrapped
/// prompt above. The machine already wrapped its output at the column count it was given; anything left
/// over is this app's estimate coming out short, and a piece that got re-flowed at its spaces would put
/// words on lines the machine did not, shifting a table or a progress bar out of the shape it drew. A
/// terminal continues a line where it ran out of room, and so does this.
///
/// A run's colour is carried into every piece it is broken into, so a cell of `ls --color` keeps its
/// colour across the break. An empty line comes back as one empty piece and not as nothing: the page has
/// to keep the line to draw a cursor on it.
fn wrap_runs(
    runs: &[(String, Option<Rgb>)],
    max_width: f32,
    measure: &dyn Fn(&str) -> f32,
) -> Vec<Vec<(String, Option<Rgb>)>> {
    let mut pieces: Vec<Vec<(String, Option<Rgb>)>> = vec![Vec::new()];
    let mut room = max_width;

    for (text, color) in runs {
        for ch in text.chars() {
            let cell = measure(&ch.to_string());

            if room < cell && !pieces.last().expect("a piece is open").is_empty() {
                pieces.push(Vec::new());
                room = max_width;
            }

            room -= cell;

            // The same colour as the cell before it: one run rather than two, which is what the page's
            // `runs()` did for the machine's own output.
            let piece = pieces.last_mut().expect("a piece is open");
            if let Some((run, run_color)) = piece.last_mut() {
                if *run_color == *color {
                    run.push(ch);
                    continue;
                }
            }

            piece.push((ch.to_string(), *color));
        }
    }

    pieces
}

/// A flat button that is only words: what a status line offers.
///
/// No background and no border, because it sits in a line of words rather than on a page of its own —
/// the colour of the words it is next to is the colour it is drawn in, so a fault's buttons are the same
/// red the fault is, and a session that ended quietly offers its buttons in the grey it ended in.
fn text_button(words: &str, message: Message, color: Color) -> El {
    button(text(words.to_string()).size(style::FONT_SIZE).color(color))
        .on_press(message)
        .padding(Padding {
            top: 2.0,
            right: 6.0,
            bottom: 2.0,
            left: 6.0,
        })
        .style(move |_theme, _status| button::Style {
            background: None,
            text_color: color,
            ..button::Style::default()
        })
        .into()
}

/// What went wrong with a machine, in words, with the machine named.
///
/// The app's words and not the backend's: the backend has [`NasFault`], which is a small closed list
/// because a screen has to have a sentence for every entry in it — the same reason the NAS monitor's page
/// words its faults here rather than there.
fn fault_words(fault: NasFault, host: &str) -> String {
    match fault {
        NasFault::Unreachable => format!("{host} did not answer."),
        NasFault::Authentication => format!("{host} would not take the account."),
        NasFault::HostKeyChanged => {
            format!("{host} is not the machine this board was told to expect.")
        }
        NasFault::NoNetwork => String::from("There is no network to reach it on."),
        NasFault::Protocol => {
            format!("{host} and this board could not agree on how to speak.")
        }
        NasFault::Unsupported => {
            String::from("This board has no SSH client, so it cannot open a session.")
        }
    }
}

/// The visible tail of a line: a terminal shows the end of what is being typed.
fn visible_tail(text: &str, max_width: f32) -> String {
    if style::measure(text) <= max_width {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    for start in 0..chars.len() {
        let candidate: String = chars[start..].iter().collect();
        if style::measure(&candidate) <= max_width {
            return candidate;
        }
    }
    String::new()
}
