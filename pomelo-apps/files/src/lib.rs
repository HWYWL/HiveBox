//! The file manager: the board's built-in volume and the card in the slot.
//!
//! # Two volumes, two halves of a filesystem
//!
//! The page browses wherever a filesystem is mounted, and this board has at most two: the built-in
//! partition, which is always there, and a card in the slot, which is there when it is there. The
//! volumes come from [`StorageBackend::volumes`], and a card is looked for again when the page opens
//! and when a finger asks it to — there is no card-detect pin, so a probe is the only way to know.
//!
//! Reading the filesystem — listing a directory, `stat`ing a name, reading a file — is `std::fs`'s,
//! over the VFS, as it is for the terminal and the music player. Writing is not: `std::fs` cannot
//! create a *file* on this board, so deleting, renaming and pasting go through [`ops`] and the
//! backend's `fopen`. [`ops`] makes that argument at length.
//!
//! # What a finger does
//!
//! One tap selects a row; a second tap on the same row goes into it when it is a directory. That is
//! the shape a touch screen wants — there is no double-click to lean on — and it is what makes the
//! four actions possible at all: a rename needs a row that is selected and *not* entered.
//!
//! [`StorageBackend::volumes`]: pomelo_hal::StorageBackend::volumes

pub mod entry;
pub mod ops;
pub mod path;
pub mod style;
mod ui;
mod words;

use std::sync::Arc;

use iced::{Subscription, Theme};

use pomelo_hal::{Board, VolumeInfo};
use pomelo_widgets::preferences::SystemPreferences;
use pomelo_widgets::touch_keyboard::{KeyAction, KeyboardMode};

use entry::Entry;

/// What the page reacts to.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Open the volume at this index: the built-in one, or the card.
    Volume(usize),
    /// Have the board look for a card again, and re-read the volume list.
    Refresh,
    /// A finger on a row: select it, or go into it when it was already the selected one.
    Open(usize),
    /// Go up one directory.
    Up,
    /// Ask to make a new folder, which opens the name prompt.
    NewFolder,
    /// Ask to rename the selected entry, which opens the prompt on its name.
    Rename,
    /// Ask to delete the selected entry, which opens the confirmation.
    Delete,
    /// Keep the selected entry for a paste.
    Copy,
    /// Copy what has been kept into the directory that is up.
    Paste,
    /// A key of the prompt's keyboard: a character, a backspace, a space, return, or a page change.
    Key(KeyAction),
    /// The prompt's button: do the thing it is asking about.
    Confirm,
    /// Put the prompt away and do nothing.
    Cancel,
}

/// What the prompt over the list is asking about.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asking {
    /// Make a directory with the typed name, under the directory that is up.
    NewFolder,
    /// Rename this path to the typed name.
    Rename(String),
    /// Remove this path — and `true` when it is a directory, so the removal takes the tree — once the
    /// person says so.
    Delete(String, bool),
}

/// The box over the list: a name to type, or a question to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Prompt {
    asking: Asking,
    text: String,
    /// Which page the keyboard under a name is on: the letters, the capitals, or one of the two rows
    /// of signs. The prompt's and not the app's, because a keyboard exists only while a name is being
    /// asked for — and every prompt that asks for one starts on the letters.
    keyboard: KeyboardMode,
}

/// The app.
pub struct Files {
    board: Arc<Board>,
    preferences: SystemPreferences,
    /// Every mounted volume, in the order the board reports them: the built-in one first.
    volumes: Vec<VolumeInfo>,
    /// Which of [`Files::volumes`] the page is looking at.
    current: usize,
    /// The directory that is up, as an absolute path inside the current volume.
    cwd: String,
    /// What that directory held, the last time it was listed.
    entries: Vec<Entry>,
    /// The row a finger has picked out, if any.
    selected: Option<usize>,
    /// The path a paste would copy from.
    clipboard: Option<String>,
    /// The prompt that is up, if any.
    prompt: Option<Prompt>,
    /// The last failure, in the words of whatever refused.
    fault: Option<String>,
}

impl Files {
    /// A file manager on this board, with the volumes looked at once.
    ///
    /// The probe ends by listing the directory that is up, so the first frame has the contents of the
    /// built-in volume rather than an empty page waiting for a second.
    pub fn new(board: Arc<Board>) -> Self {
        let mut files = Self {
            board,
            preferences: SystemPreferences::default(),
            volumes: Vec::new(),
            current: 0,
            cwd: String::new(),
            entries: Vec::new(),
            selected: None,
            clipboard: None,
            prompt: None,
            fault: None,
        };

        files.probe();
        files
    }

    /// The active system preferences.
    pub fn preferences(&self) -> SystemPreferences {
        self.preferences
    }

    /// Sets the active system preferences.
    pub fn set_preferences(&mut self, preferences: SystemPreferences) {
        self.preferences = preferences;
    }

    /// Every volume the board reported.
    pub fn volumes(&self) -> &[VolumeInfo] {
        &self.volumes
    }

    /// The volume the page is looking at, if there is one.
    pub fn volume(&self) -> Option<&VolumeInfo> {
        self.volumes.get(self.current)
    }

    /// Which volume that is.
    pub fn current(&self) -> usize {
        self.current
    }

    /// The directory that is up.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// What that directory holds.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The row a finger has picked out.
    pub fn selected(&self) -> Option<&Entry> {
        self.selected.and_then(|index| self.entries.get(index))
    }

    /// What has been kept for a paste.
    pub fn clipboard(&self) -> Option<&str> {
        self.clipboard.as_deref()
    }

    /// Whether the name prompt is up.
    pub fn prompting(&self) -> bool {
        self.prompt.is_some()
    }

    /// The last failure, if anything has failed since the app was opened.
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }

    /// Whether there is anywhere above the directory that is up.
    pub fn can_go_up(&self) -> bool {
        self.volumes
            .get(self.current)
            .is_some_and(|volume| path::parent(&self.cwd, &volume.mount_point).is_some())
    }

    /// Look for a card, re-read the volume list, and list the directory that is up.
    ///
    /// The probe and the read are two statements and two locks rather than one: a card is found by a
    /// [`refresh`](pomelo_hal::StorageBackend::refresh) that may spend the driver's timeouts, and
    /// holding that lock while listing a directory would keep every other app off the storage backend
    /// for as long as the probe takes.
    pub fn probe(&mut self) {
        {
            let mut storage = self.board.storage();
            let _ = storage.refresh();
        }

        self.volumes = self.board.storage().volumes().unwrap_or_default();

        if self.current >= self.volumes.len() {
            self.current = 0;
        }

        self.reload();
    }

    /// List the directory that is up, against the volume that is open.
    ///
    /// A `cwd` that has wandered outside the current volume — which happens when a card is taken out
    /// while one of its directories is up — is put back to the volume's root, because the path a
    /// person would be shown would be a directory that is not there any more.
    fn reload(&mut self) {
        let Some(volume) = self.volumes.get(self.current) else {
            self.cwd.clear();
            self.entries.clear();
            self.selected = None;
            return;
        };

        let root = volume.mount_point.trim_end_matches('/').to_string();

        if self.cwd.is_empty() || !self.cwd.starts_with(&root) {
            self.cwd = root;
        }

        self.entries = list(&self.cwd);
        self.selected = None;
    }

    /// Go into `index`, or select it when it is not a directory.
    fn open(&mut self, index: usize) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };

        if entry.is_dir() && self.selected == Some(index) {
            self.cwd = entry.path.clone();
            self.reload();
        } else {
            self.selected = Some(index);
        }
    }

    /// Go up one directory, within the current volume.
    fn up(&mut self) {
        let Some(volume) = self.volumes.get(self.current) else {
            return;
        };

        if let Some(parent) = path::parent(&self.cwd, &volume.mount_point) {
            self.cwd = parent;
            self.reload();
        }
    }

    /// Open the volume at `index`, if the board reports one there.
    fn show_volume(&mut self, index: usize) {
        if index < self.volumes.len() {
            self.current = index;
            self.cwd = self.volumes[index].mount_point.clone();
            self.reload();
        }
    }

    /// Run what the prompt was asking about, and put it away.
    ///
    /// Every arm lists the directory again afterwards, so that what is on screen is what is on the
    /// filesystem: a name that was refused does not sit in the list as if it had worked, and a
    /// removal that failed is not drawn as a row that is still there *and* an error at once.
    fn confirm(&mut self) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };

        let name = prompt.text.trim().to_string();

        let result = match &prompt.asking {
            Asking::NewFolder => {
                if path::valid_name(&name) {
                    ops::make_dir(&path::join(&self.cwd, &name))
                } else {
                    Ok(())
                }
            }
            Asking::Rename(from) => {
                if path::valid_name(&name) {
                    ops::rename(&self.board, from, &path::join(&self.cwd, &name))
                } else {
                    Ok(())
                }
            }
            Asking::Delete(whom, is_dir) => ops::remove(&self.board, whom, *is_dir),
        };

        self.fault = result.err();
        self.reload();
    }

    /// One key of the prompt's keyboard.
    ///
    /// The app owns the string, as the terminal owns its input line and the settings app owns its
    /// password: `iced::widget::text_input` takes its focus from a physical keyboard and a focus
    /// system, and this panel has neither — the keys under the prompt *are* the keyboard.
    fn key(&mut self, action: KeyAction) {
        match action {
            KeyAction::Char(character) => self.type_name(character),
            KeyAction::Space => self.type_name(' '),
            KeyAction::Backspace => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.text.pop();
                }
            }
            KeyAction::SwitchMode(mode) => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.keyboard = mode;
                }
            }
            KeyAction::Enter => self.confirm(),
        }
    }

    /// One character into the name the prompt is asking for, and nothing when nothing is being asked.
    fn type_name(&mut self, character: char) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.text.push(character);
        }
    }

    /// Put what is selected on the clipboard, to be copied somewhere else.
    fn copy(&mut self) {
        self.clipboard = self.selected().map(|entry| entry.path.clone());
    }

    /// Copy the clipboard into the directory that is up, under its own name.
    fn paste(&mut self) {
        let Some(source) = self.clipboard.clone() else {
            return;
        };

        let name = base(&source);

        if name.is_empty() {
            return;
        }

        self.fault = ops::copy(&self.board, &source, &path::join(&self.cwd, name)).err();
        self.reload();
    }

    /// One message.
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Volume(index) => self.show_volume(index),
            Message::Refresh => self.probe(),
            Message::Open(index) => self.open(index),
            Message::Up => self.up(),
            Message::NewFolder => {
                self.prompt = Some(Prompt {
                    asking: Asking::NewFolder,
                    text: String::new(),
                    keyboard: KeyboardMode::Lower,
                });
            }
            Message::Rename => {
                if let Some(entry) = self.selected() {
                    self.prompt = Some(Prompt {
                        asking: Asking::Rename(entry.path.clone()),
                        text: entry.name.clone(),
                        keyboard: KeyboardMode::Lower,
                    });
                }
            }
            Message::Delete => {
                if let Some(entry) = self.selected() {
                    self.prompt = Some(Prompt {
                        asking: Asking::Delete(entry.path.clone(), entry.is_dir()),
                        text: String::new(),
                        keyboard: KeyboardMode::Lower,
                    });
                }
            }
            Message::Copy => self.copy(),
            Message::Paste => self.paste(),
            Message::Key(action) => self.key(action),
            Message::Confirm => self.confirm(),
            Message::Cancel => self.prompt = None,
        }
    }

    /// The theme: the same two palettes the launcher and every other app uses.
    pub fn theme(&self) -> Theme {
        ui::theme(self.preferences.theme)
    }

    /// Nothing about a file manager changes on its own.
    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::none()
    }

    /// The page.
    pub fn view(&self) -> iced::Element<'_, Message> {
        ui::view(self)
    }
}

/// Everything in `dir`, ordered, as a value the page owns.
///
/// A directory that cannot be listed is an empty one and not a failure of the page: a `cwd` that has
/// gone away under the app is a list with nothing in it under a heading that still says where it was,
/// which is a better screen than an error box for a card that was pulled out mid-scroll.
fn list(dir: &str) -> Vec<Entry> {
    let Ok(reading) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let entries = reading
        .filter_map(|one| one.ok())
        .filter_map(|one| Entry::read(&one.path()))
        .collect();

    entry::sorted(entries)
}

/// The last name in a path: `photo.jpg` for `/sdcard/trips/photo.jpg`.
fn base(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}
