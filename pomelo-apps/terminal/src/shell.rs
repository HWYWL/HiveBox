//! The terminal's shell: the model, the commands and the storage helpers, with **no UI**.
//!
//! Everything here is the part of the terminal that is not drawing: [`TerminalModel`] and its
//! history, the [`commands`](crate::commands) registry, the storage helpers ([`get_storage_root`],
//! [`resolve_path`], [`TerminalModel::short_cwd`]) and the `VS_*` colour roles. None of it depends
//! on a framework or a font, which is why it is a module and not a pile of app code.
//!
//! Two decisions follow from that, and the reason is the same for both:
//!
//! * **Text measurement.** The measurement is a parameter, not a dependency: [`wrap_line`] takes
//!   a `&dyn Fn(&str) -> f32` and the UI supplies it (this app measures with cosmic-text via
//!   `style::measure`). The model therefore stores **raw text** — one [`HistoryEntry`] per push,
//!   exactly as the command wrote it, newlines included — and the UI wraps it when it draws.
//! * **Scrolling.** `TerminalModel` used to own a framework `ScrollController`. That is a framework
//!   type, so it is gone; what is left is [`TerminalModel::auto_scroll_to_bottom`], a plain `bool`
//!   the UI reads and acts on, which the app turns into an anchored `scrollable`.
//!
//! The colours are [`Rgb`] triples for the same reason: a UI has a colour type and this module
//! does not want one.

use pomelo_widgets::{FontSizeTier, SystemPreferences};

/// The font size tiers obtained from SystemPreferences: [18.0, 20.0, 24.0, 30.0].
pub const FONT_SIZES: [f32; 4] = SystemPreferences::font_sizes();
pub const FONT_EXTRA_SMALL: f32 = FontSizeTier::ExtraSmall.base_size(); // 18.0 px (Compact tier)
pub const FONT_SMALL: f32 = FontSizeTier::Small.base_size();            // 20.0 px (Small tier)
pub const FONT_STANDARD: f32 = FontSizeTier::Standard.base_size();      // 24.0 px (Standard tier)
pub const FONT_LARGE: f32 = FontSizeTier::Large.base_size();            // 30.0 px (Large tier)

/// The terminal's character cell, sourced from SystemPreferences (no custom font sizes).
pub const TERM_FONT_SIZE: f32 = FONT_SMALL;
/// One line cell's fixed height (ascent + descent + gap), matching the standard system tier.
pub const TERM_LINE_HEIGHT: f32 = FONT_STANDARD;

/// The default terminal username.
pub const DEFAULT_USER: &str = "pomelo";

// =============================================================================
// Terminal colours
// =============================================================================
// Pure black background for the terminal, to contrast against the keyboard's #1C1C1E. The values
// are the original terminal's; each UI converts them to its own colour type.

/// A colour, as the model's palette states it. No UI type crosses this crate's boundary, so the
/// roles are plain triples and the conversion is each app's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

pub const VS_TEXT_FG: Rgb = rgb(212, 212, 212); // #D4D4D4 default text foreground
pub const VS_PROMPT_USER: Rgb = rgb(35, 209, 139); // #23D18B ANSI bright green (user/host)
pub const VS_PROMPT_DIR: Rgb = rgb(86, 156, 214); // #569CD6 ANSI bright blue (directory)
pub const VS_PROMPT_PUNCT: Rgb = rgb(133, 133, 133); // #858585 colon / separator
pub const VS_PROMPT_SYM: Rgb = rgb(204, 204, 204); // #CCCCCC prompt symbol ($)
pub const VS_COMMAND_TEXT: Rgb = rgb(255, 255, 255); // #FFFFFF active command text

// =============================================================================
// The model
// =============================================================================

/// The keyboard's mode and what a key press means — re-exported from the shared widget crate.
///
/// They are re-exported rather than defined here because the settings app draws the same keyboard
/// for a Wi-Fi password, so the table, the geometry and the two enums live in `pomelo-widgets`.
/// The names stay reachable as `terminal::KeyAction` on purpose: `terminal`'s public surface is
/// what a host typing into the terminal names (`app-launcher`'s tests do exactly that).
pub use pomelo_widgets::touch_keyboard::{KeyAction, KeyboardMode};

/// One line of the transcript.
#[derive(Debug, Clone)]
pub enum HistoryEntry {
    /// A command that was run, with the directory it was run in.
    Prompt { cwd: String, cmd: String },
    /// An output line.
    Output(String),
}

/// Wraps `text` to `max_width` using `measure` to find how wide a string is.
///
/// `measure` is the parameter that keeps this crate free of a font: the original passed its
/// baked-font metrics, the iced one its own estimate. The algorithm is the original
/// one — whole lines that fit are kept, otherwise split on spaces, and a single word longer than
/// the line is split character by character.
pub fn wrap_line(text: &str, max_width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut result = Vec::new();

    for raw_line in text.lines() {
        if raw_line.is_empty() {
            result.push(String::new());
            continue;
        }

        // Fast path: if the entire line fits, push it directly.
        if measure(raw_line) <= max_width {
            result.push(raw_line.to_string());
            continue;
        }

        // Split by words with whitespace preserved.
        let words: Vec<&str> = raw_line.split_inclusive(' ').collect();
        let mut current_line = String::new();

        for word in words {
            let candidate = if current_line.is_empty() {
                word.to_string()
            } else {
                format!("{}{}", current_line, word)
            };

            if measure(&candidate) <= max_width {
                current_line = candidate;
            } else {
                if !current_line.is_empty() {
                    result.push(current_line.trim_end().to_string());
                    current_line = String::new();
                }

                // A single word wider than the line is split character by character.
                if measure(word) > max_width {
                    let mut chunk = String::new();
                    for ch in word.chars() {
                        let next_chunk = format!("{}{}", chunk, ch);
                        if measure(&next_chunk) > max_width {
                            result.push(chunk);
                            chunk = ch.to_string();
                        } else {
                            chunk = next_chunk;
                        }
                    }
                    if !chunk.is_empty() {
                        current_line = chunk;
                    }
                } else {
                    current_line = word.to_string();
                }
            }
        }

        if !current_line.is_empty() {
            result.push(current_line.trim_end().to_string());
        }
    }

    if result.is_empty() {
        result.push(String::new());
    }
    result
}

/// The canonical storage root on ESP32, or a sensible fallback on a host/simulator.
pub fn get_storage_root() -> &'static str {
    #[cfg(target_os = "espidf")]
    {
        "/internal"
    }
    #[cfg(not(target_os = "espidf"))]
    {
        "./internal"
    }
}

/// Resolves a command argument relative to the current working directory or the storage root.
pub fn resolve_path(cwd: &str, arg: &str) -> String {
    let raw = arg.trim();

    let path_str = if raw.is_empty() {
        if cwd.is_empty() {
            get_storage_root().to_string()
        } else {
            cwd.to_string()
        }
    } else if raw == "~" || raw == "~/" {
        get_storage_root().to_string()
    } else if let Some(sub) = raw.strip_prefix("~/") {
        let root = get_storage_root();
        if root == "." {
            sub.to_string()
        } else {
            format!("{}/{}", root.trim_end_matches('/'), sub)
        }
    } else if raw.starts_with('/') {
        raw.to_string()
    } else {
        // Relative to cwd.
        if cwd == "." || cwd.is_empty() {
            raw.to_string()
        } else {
            format!("{}/{}", cwd.trim_end_matches('/'), raw)
        }
    };

    // On a desktop simulator: if /internal or /storage is accessed but does not exist on the host, map it.
    #[cfg(not(target_os = "espidf"))]
    {
        if path_str == "/internal" && !std::path::Path::new("/internal").exists() {
            return if std::path::Path::new("./internal").exists() {
                "./internal".to_string()
            } else {
                ".".to_string()
            };
        }
        if path_str.starts_with("/internal/") && !std::path::Path::new("/internal").exists() {
            let sub = &path_str["/internal/".len()..];
            return if std::path::Path::new("./internal").exists() {
                format!("./internal/{}", sub)
            } else {
                format!("./{}", sub)
            };
        }
        if path_str == "/storage" && !std::path::Path::new("/storage").exists() {
            return "./storage".to_string();
        }
        if path_str.starts_with("/storage/") && !std::path::Path::new("/storage").exists() {
            return format!(".{}", path_str);
        }
    }

    path_str
}

/// The terminal's state: the transcript, the line being typed, the working directory.
///
/// Deliberately plain data now — see the crate docs for what moved out (the scroll controller) and
/// why (raw text, not wrapped lines).
#[derive(Clone)]
pub struct TerminalModel {
    pub current_input: String,
    pub keyboard_mode: KeyboardMode,
    pub cwd: String,
    pub history: Vec<HistoryEntry>,
    /// Set whenever the transcript changes; each UI consumes it (sets it back to `false`) when it
    /// has acted on it, so a touch drag is not overridden afterwards.
    /// The width the UI had to draw text in — the model stored it before wrapping moved to the
    /// UI, and nothing reads it now.
    pub auto_scroll_to_bottom: bool,
}

impl Default for TerminalModel {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalModel {
    pub fn new() -> Self {
        let initial_cwd = get_storage_root().to_string();

        let mut state = Self {
            current_input: String::new(),
            keyboard_mode: KeyboardMode::Lower,
            cwd: initial_cwd,
            history: Vec::new(),
            auto_scroll_to_bottom: true,
        };

        state.push_output("Pomelo UI System [Version 0.2.0]");
        state.push_output("Type 'help' for available commands.");
        state
    }

    /// Appends an output entry. The text is stored raw (newlines and all); each UI wraps it when
    /// it draws, with its own measurement.
    pub fn push_output(&mut self, text: &str) {
        self.history.push(HistoryEntry::Output(text.to_string()));
        self.auto_scroll_to_bottom = true;
    }

    /// Applies a key to the model: the one place that knows how typing and running commands work.
    pub fn handle_key_action(&mut self, action: KeyAction) {
        self.auto_scroll_to_bottom = true;
        match action {
            KeyAction::Char(ch) => {
                self.current_input.push(ch);
            }
            KeyAction::Space => {
                self.current_input.push(' ');
            }
            KeyAction::Backspace => {
                self.current_input.pop();
            }
            KeyAction::Enter => {
                let cmd = self.current_input.trim().to_string();
                if !cmd.is_empty() {
                    self.history.push(HistoryEntry::Prompt {
                        cwd: self.short_cwd(),
                        cmd: cmd.clone(),
                    });
                    self.execute_command(&cmd);
                    self.current_input.clear();
                }
            }
            KeyAction::SwitchMode(mode) => {
                self.keyboard_mode = mode;
            }
        }
    }

    /// The working directory as the prompt shows it: `/internal` at the storage root.
    pub fn short_cwd(&self) -> String {
        self.cwd.clone()
    }

    /// The line being typed. For the host and for tests.
    pub fn input(&self) -> &str {
        &self.current_input
    }

    /// The current working directory. For the host and for tests.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// The transcript as plain text lines: a prompt renders as `pomelo:cwd$ cmd`, an output/error
    /// entry as its text (split on newlines). For a host that displays the terminal elsewhere, and
    /// for tests. Not wrapped — wrapping is the UI's, and needs its measurement.
    pub fn history_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for entry in &self.history {
            match entry {
                HistoryEntry::Prompt { cwd, cmd } => {
                    lines.push(format!("{DEFAULT_USER}:{cwd}$ {cmd}"));
                }
                HistoryEntry::Output(text) => {
                    lines.extend(text.lines().map(str::to_string));
                }
            }
        }
        lines
    }

    fn execute_command(&mut self, cmd: &str) {
        crate::commands::get_registry().dispatch(self, cmd);

        // Keep up to 64 lines of backlog for rich scrolling.
        if self.history.len() > 64 {
            let overflow = self.history.len() - 64;
            self.history.drain(0..overflow);
        }
        self.auto_scroll_to_bottom = true;
    }
}
