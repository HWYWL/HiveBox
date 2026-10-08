//! The key table: what is on each row, and how much of its row each key takes.
//!
//! The keyboard is **flex**. The box the band is given decides the size of everything: a key is a
//! share of its row ([`Key::portion`]), a row is a quarter of the band's height, and the only
//! absolute numbers left are the gaps, the padding and the type — the repository's rule, fluid
//! boxes and absolute type. Nothing here is a measurement of a screen, so nothing here has to be
//! told one.
//!
//! The shares are the original's (`VirtualKeyboard::build_resizable`), normalised so a letter key is
//! [`KEY_PORTION`]: the keyboard crate the original shared cannot be reused — it was built from the
//! framework this repository used to have — but its layout *is* the specification, and its ratios are
//! what it was: `key_w * 1.6` for the mode key, the shift key filling the edge of its row, and the
//! same four rows, keys, colours and numeric mode as the iOS keyboard.
//!
//! The original keyboard labels shift, backspace and return with icons (`⇧ ⌫ ↵`), which are
//! rendered using [`pomelo_material_symbols`]. The input cursor is painted as a block by whoever
//! owns the text (see `lib.rs`).

/// Which page the on-screen keyboard is on.
///
/// This is state, not a widget: it survives across key presses and is read back to draw the
/// keyboard, so whoever owns the text owns this too and hands it to [`rows`] on every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardMode {
    Lower,
    Upper,
    Numbers,
    Symbols,
}

impl KeyboardMode {
    /// Applies a key action to update the keyboard mode if it requests a mode switch.
    ///
    /// Shift is locked: pressing Shift toggles between [`KeyboardMode::Lower`] and
    /// [`KeyboardMode::Upper`]. Character inputs do not automatically revert the shift state.
    pub fn update(&mut self, action: KeyAction) {
        if let KeyAction::SwitchMode(mode) = action {
            *self = mode;
        }
    }
}

/// What pressing a key does.
///
/// The keyboard does not act on it: it hands it back through the closure the caller passed to
/// [`band`](super::band), and the app decides what a character, a backspace or a mode switch means
/// for the string it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    /// Type `char` at the cursor.
    Char(char),
    /// Delete the character before the cursor.
    Backspace,
    /// Commit the text (`return`).
    Enter,
    /// Type a space.
    Space,
    /// Switch the keyboard to another page.
    SwitchMode(KeyboardMode),
}

/// Which palette a key takes. Mirrors the original keyboard's `KeyKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Character,
    Special,
    Return,
    ShiftActive,
}

use pomelo_material_symbols::Icon;

/// How a key is labelled: plain text or an icon from `pomelo_material_symbols`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLabel {
    Text(&'static str),
    Icon(Icon),
}

impl KeyLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyLabel::Text(s) => s,
            KeyLabel::Icon(icon) => icon.glyph(),
        }
    }
}

impl From<&'static str> for KeyLabel {
    fn from(s: &'static str) -> Self {
        KeyLabel::Text(s)
    }
}

impl From<&&'static str> for KeyLabel {
    fn from(s: &&'static str) -> Self {
        KeyLabel::Text(*s)
    }
}

impl From<Icon> for KeyLabel {
    fn from(icon: Icon) -> Self {
        KeyLabel::Icon(icon)
    }
}

impl PartialEq<&str> for KeyLabel {
    fn eq(&self, other: &&str) -> bool {
        match self {
            KeyLabel::Text(s) => s == other,
            KeyLabel::Icon(icon) => icon.glyph() == *other,
        }
    }
}

impl PartialEq<Icon> for KeyLabel {
    fn eq(&self, other: &Icon) -> bool {
        match self {
            KeyLabel::Icon(icon) => icon == other,
            KeyLabel::Text(_) => false,
        }
    }
}

/// One key: what it draws, how much of its row it takes, and what it does.
#[derive(Debug, Clone, Copy)]
pub struct Key {
    pub label: KeyLabel,
    pub kind: KeyKind,
    pub font: f32,
    /// This key's share of its row. The rows are flex, so a key's width is its share of whatever
    /// width the band is given — there is no width for this end of the keyboard to know.
    pub portion: u16,
    pub action: KeyAction,
}

/// One cell of a row: a key, or half a key's worth of nothing.
#[derive(Debug, Clone, Copy)]
pub enum Cell {
    Key(Key),
    /// The second row of the iOS keyboard is inset by half a key at each end. Nine flex cells where
    /// the rows above and below have ten would be drawn wider instead, so the inset is part of the
    /// table rather than something the layout has to guess.
    HalfKey,
}

impl Cell {
    /// The key in this cell, if it is one.
    pub fn key(&self) -> Option<&Key> {
        match self {
            Cell::Key(key) => Some(key),
            Cell::HalfKey => None,
        }
    }
}

/// A letter key's share of its row.
///
/// Every other share in the table is a multiple of this one, so the table reads as the design's own
/// ratios: 1.55 for shift and del, 1.35 for the number page's middle keys, 1.6 for the mode key,
/// 1.8 for return.
pub const KEY_PORTION: u16 = 100;

/// The half-key inset at each end of the second row — see [`Cell::HalfKey`].
pub const HALF_KEY_PORTION: u16 = KEY_PORTION / 2;

/// Shift and del: the original's `w_spec`, which filled the edges of its row.
const SHIFT_PORTION: u16 = 155;
/// The same two keys on the number and symbol pages, where the middle keys are wider (1.35x).
const WIDE_SHIFT_PORTION: u16 = 170;
/// A middle key on the number and symbol pages.
const WIDE_KEY_PORTION: u16 = 135;
/// The mode key on the bottom row ("123" / "ABC").
const MODE_PORTION: u16 = 160;
/// Space, the widest key: the original made it the row's remainder, which is what a share of 6.43
/// letter keys comes to.
const SPACE_PORTION: u16 = 643;
/// The dot beside it, a letter key's width.
const DOT_PORTION: u16 = KEY_PORTION;
/// Return, the original's `key_w * 1.8`.
const ENTER_PORTION: u16 = 180;

/// The two label sizes. Absolute, like all the type in this repository: a key's label does not
/// follow the box it is in — the number is what the 480 px design panel ends at, and both are the
/// sizes the platform bakes glyphs for.
pub const CHAR_FONT: f32 = 24.0;
pub const SPEC_FONT: f32 = 16.0;
pub const ICON_FONT: f32 = 22.0;

/// The four rows, top to bottom, in the order they are drawn.
pub fn rows(mode: KeyboardMode) -> Vec<Vec<Cell>> {
    vec![
        char_row(row1_chars(mode), KEY_PORTION),
        row2(mode),
        row3(mode),
        row4(mode),
    ]
}

fn key(
    label: impl Into<KeyLabel>,
    kind: KeyKind,
    font: f32,
    portion: u16,
    action: KeyAction,
) -> Cell {
    Cell::Key(Key {
        label: label.into(),
        kind,
        font,
        portion,
        action,
    })
}

fn char_row(chars: &[(&'static str, char)], portion: u16) -> Vec<Cell> {
    chars
        .iter()
        .map(|(label, ch)| {
            key(
                *label,
                KeyKind::Character,
                CHAR_FONT,
                portion,
                KeyAction::Char(*ch),
            )
        })
        .collect()
}

/// The second row: nine keys on the letter pages, inset by half a key at each end; ten on the
/// number and symbol pages, which have no room to inset and none of the design's reason to.
fn row2(mode: KeyboardMode) -> Vec<Cell> {
    let middle = row2_chars(mode);
    let keys = char_row(middle, KEY_PORTION);

    if row1_chars(mode).len() == middle.len() {
        return keys;
    }

    let mut row = Vec::with_capacity(keys.len() + 2);
    row.push(Cell::HalfKey);
    row.extend(keys);
    row.push(Cell::HalfKey);

    row
}

/// Shift / caps, the middle characters, and Del.
fn row3(mode: KeyboardMode) -> Vec<Cell> {
    let (shift_label, shift_font, shift_kind, shift_event) = match mode {
        KeyboardMode::Lower => (
            KeyLabel::Icon(Icon::SHIFT),
            ICON_FONT,
            KeyKind::Special,
            KeyAction::SwitchMode(KeyboardMode::Upper),
        ),
        KeyboardMode::Upper => (
            KeyLabel::Icon(Icon::SHIFT),
            ICON_FONT,
            KeyKind::ShiftActive,
            KeyAction::SwitchMode(KeyboardMode::Lower),
        ),
        KeyboardMode::Numbers => (
            KeyLabel::Text("#+="),
            SPEC_FONT,
            KeyKind::Special,
            KeyAction::SwitchMode(KeyboardMode::Symbols),
        ),
        KeyboardMode::Symbols => (
            KeyLabel::Text("123"),
            SPEC_FONT,
            KeyKind::Special,
            KeyAction::SwitchMode(KeyboardMode::Numbers),
        ),
    };

    let middle = middle_chars(mode);
    // Five keys (numbers/symbols) get wider keys, and the shift and del beside them give up the edge
    // of the row they filled on the letter pages.
    let (mid_portion, shift_portion) = if middle.len() == 5 {
        (WIDE_KEY_PORTION, WIDE_SHIFT_PORTION)
    } else {
        (KEY_PORTION, SHIFT_PORTION)
    };

    let mut keys = vec![key(
        shift_label,
        shift_kind,
        shift_font,
        shift_portion,
        shift_event,
    )];
    keys.extend(middle.iter().map(|(label, ch)| {
        key(
            *label,
            KeyKind::Character,
            CHAR_FONT,
            mid_portion,
            KeyAction::Char(*ch),
        )
    }));
    keys.push(key(
        Icon::BACKSPACE,
        KeyKind::Special,
        ICON_FONT,
        shift_portion,
        KeyAction::Backspace,
    ));
    keys
}

/// Mode toggle, space, dot and return.
fn row4(mode: KeyboardMode) -> Vec<Cell> {
    let (mode_label, mode_event) = match mode {
        KeyboardMode::Lower | KeyboardMode::Upper => {
            ("123", KeyAction::SwitchMode(KeyboardMode::Numbers))
        }
        KeyboardMode::Numbers | KeyboardMode::Symbols => {
            ("ABC", KeyAction::SwitchMode(KeyboardMode::Lower))
        }
    };

    vec![
        key(
            mode_label,
            KeyKind::Special,
            SPEC_FONT,
            MODE_PORTION,
            mode_event,
        ),
        key(
            "space",
            KeyKind::Character,
            SPEC_FONT,
            SPACE_PORTION,
            KeyAction::Space,
        ),
        key(
            ".",
            KeyKind::Character,
            CHAR_FONT,
            DOT_PORTION,
            KeyAction::Char('.'),
        ),
        key(
            Icon::KEYBOARD_RETURN,
            KeyKind::Return,
            ICON_FONT,
            ENTER_PORTION,
            KeyAction::Enter,
        ),
    ]
}

fn row1_chars(mode: KeyboardMode) -> &'static [(&'static str, char)] {
    match mode {
        KeyboardMode::Lower => &[
            ("q", 'q'),
            ("w", 'w'),
            ("e", 'e'),
            ("r", 'r'),
            ("t", 't'),
            ("y", 'y'),
            ("u", 'u'),
            ("i", 'i'),
            ("o", 'o'),
            ("p", 'p'),
        ],
        KeyboardMode::Upper => &[
            ("Q", 'Q'),
            ("W", 'W'),
            ("E", 'E'),
            ("R", 'R'),
            ("T", 'T'),
            ("Y", 'Y'),
            ("U", 'U'),
            ("I", 'I'),
            ("O", 'O'),
            ("P", 'P'),
        ],
        KeyboardMode::Numbers => &[
            ("1", '1'),
            ("2", '2'),
            ("3", '3'),
            ("4", '4'),
            ("5", '5'),
            ("6", '6'),
            ("7", '7'),
            ("8", '8'),
            ("9", '9'),
            ("0", '0'),
        ],
        KeyboardMode::Symbols => &[
            ("[", '['),
            ("]", ']'),
            ("{", '{'),
            ("}", '}'),
            ("#", '#'),
            ("%", '%'),
            ("^", '^'),
            ("*", '*'),
            ("+", '+'),
            ("=", '='),
        ],
    }
}

fn row2_chars(mode: KeyboardMode) -> &'static [(&'static str, char)] {
    match mode {
        KeyboardMode::Lower => &[
            ("a", 'a'),
            ("s", 's'),
            ("d", 'd'),
            ("f", 'f'),
            ("g", 'g'),
            ("h", 'h'),
            ("j", 'j'),
            ("k", 'k'),
            ("l", 'l'),
        ],
        KeyboardMode::Upper => &[
            ("A", 'A'),
            ("S", 'S'),
            ("D", 'D'),
            ("F", 'F'),
            ("G", 'G'),
            ("H", 'H'),
            ("J", 'J'),
            ("K", 'K'),
            ("L", 'L'),
        ],
        KeyboardMode::Numbers => &[
            ("-", '-'),
            ("/", '/'),
            (":", ':'),
            (";", ';'),
            ("(", '('),
            (")", ')'),
            ("$", '$'),
            ("&", '&'),
            ("@", '@'),
            ("\"", '"'),
        ],
        KeyboardMode::Symbols => &[
            ("_", '_'),
            ("\\", '\\'),
            ("|", '|'),
            ("~", '~'),
            ("<", '<'),
            (">", '>'),
            ("$", '$'),
            ("&", '&'),
            (":", ':'),
            (";", ';'),
        ],
    }
}

fn middle_chars(mode: KeyboardMode) -> &'static [(&'static str, char)] {
    match mode {
        KeyboardMode::Lower => &[
            ("z", 'z'),
            ("x", 'x'),
            ("c", 'c'),
            ("v", 'v'),
            ("b", 'b'),
            ("n", 'n'),
            ("m", 'm'),
        ],
        KeyboardMode::Upper => &[
            ("Z", 'Z'),
            ("X", 'X'),
            ("C", 'C'),
            ("V", 'V'),
            ("B", 'B'),
            ("N", 'N'),
            ("M", 'M'),
        ],
        KeyboardMode::Numbers | KeyboardMode::Symbols => {
            &[(".", '.'), (",", ','), ("?", '?'), ("!", '!'), ("'", '\'')]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys of a row, in the order they are drawn.
    ///
    /// A row is a list of [`Cell`]s because one of them has half-keys in it; every test here is
    /// about the keys.
    fn row_keys(cells: &[Cell]) -> Vec<&Key> {
        cells.iter().filter_map(Cell::key).collect()
    }

    /// The share of its row a cell takes.
    fn portion_of(cell: &Cell) -> u16 {
        match cell {
            Cell::Key(key) => key.portion,
            Cell::HalfKey => HALF_KEY_PORTION,
        }
    }

    /// The second row is as wide as the first.
    ///
    /// It has one key fewer, so a row of nine flex cells would be drawn with wider keys than the row
    /// above it — the two half-key cells are what make them the same width, and what insets the row
    /// the way the iOS keyboard's is.
    #[test]
    fn the_second_row_is_as_wide_as_the_first() {
        for mode in [KeyboardMode::Lower, KeyboardMode::Upper] {
            let table = rows(mode);
            let width = |row: &[Cell]| row.iter().map(portion_of).sum::<u16>();

            assert_eq!(width(&table[0]), width(&table[1]), "{mode:?}");
            assert_eq!(table[1].len(), 11, "{mode:?} row 2 cells");
        }
    }

    #[test]
    fn every_mode_has_four_rows() {
        for mode in [
            KeyboardMode::Lower,
            KeyboardMode::Upper,
            KeyboardMode::Numbers,
            KeyboardMode::Symbols,
        ] {
            let table = rows(mode);

            assert_eq!(table.len(), 4, "{mode:?}");

            // 10, then 9 (or 10 in the numeric mode), then shift + the middle + del, then the
            // bottom row. The numeric and symbolic pages have five middle keys where the letters
            // have seven, and those five are drawn wider — so row 3 is the row whose length tells
            // the two families of page apart.
            let letters = matches!(mode, KeyboardMode::Lower | KeyboardMode::Upper);
            assert_eq!(row_keys(&table[0]).len(), 10, "{mode:?} row 1");
            assert_eq!(
                row_keys(&table[1]).len(),
                if letters { 9 } else { 10 },
                "{mode:?} row 2"
            );
            assert_eq!(
                row_keys(&table[2]).len(),
                if letters { 9 } else { 7 },
                "{mode:?} row 3"
            );
            assert_eq!(row_keys(&table[3]).len(), 4, "{mode:?} row 4");
        }
    }

    #[test]
    fn a_mode_switch_is_what_comes_back_from_the_shift_key() {
        let lower = rows(KeyboardMode::Lower);
        let upper = rows(KeyboardMode::Upper);

        assert_eq!(
            row_keys(&lower[2])[0].action,
            KeyAction::SwitchMode(KeyboardMode::Upper)
        );
        assert_eq!(
            row_keys(&upper[2])[0].action,
            KeyAction::SwitchMode(KeyboardMode::Lower)
        );

        // The *case* is the thing that changes, and it is visible in the labels.
        assert_eq!(row_keys(&lower[0])[0].label, "q");
        assert_eq!(row_keys(&upper[0])[0].label, "Q");
    }

    #[test]
    fn keyboard_mode_update_locks_shift_across_char_actions() {
        let mut mode = KeyboardMode::Lower;

        // Shift pressed -> switches to Upper (locked)
        mode.update(KeyAction::SwitchMode(KeyboardMode::Upper));
        assert_eq!(mode, KeyboardMode::Upper);

        // Typing characters retains uppercase mode
        mode.update(KeyAction::Char('A'));
        assert_eq!(mode, KeyboardMode::Upper);
        mode.update(KeyAction::Char('B'));
        assert_eq!(mode, KeyboardMode::Upper);

        // Shift pressed again -> unlocks back to Lower
        mode.update(KeyAction::SwitchMode(KeyboardMode::Lower));
        assert_eq!(mode, KeyboardMode::Lower);
    }

    #[test]
    fn the_shift_key_says_which_mode_it_is_in() {
        let lower = rows(KeyboardMode::Lower);
        let upper = rows(KeyboardMode::Upper);

        // `ShiftActive` is the white key: the one palette that is not the keyboard's grey.
        assert_eq!(row_keys(&lower[2])[0].kind, KeyKind::Special);
        assert_eq!(row_keys(&upper[2])[0].kind, KeyKind::ShiftActive);
        assert_eq!(row_keys(&lower[2])[0].label, Icon::SHIFT);
        assert_eq!(row_keys(&upper[2])[0].label, Icon::SHIFT);
    }

    #[test]
    fn the_bottom_row_always_has_a_way_back_to_the_letters() {
        for mode in [KeyboardMode::Numbers, KeyboardMode::Symbols] {
            let table = rows(mode);
            let bottom = row_keys(&table[3]);

            assert_eq!(bottom[0].label, "ABC");
            assert_eq!(
                bottom[0].action,
                KeyAction::SwitchMode(KeyboardMode::Lower),
                "{mode:?}"
            );
        }

        // …and the letter pages have the way into the numbers.
        for mode in [KeyboardMode::Lower, KeyboardMode::Upper] {
            let table = rows(mode);

            assert_eq!(row_keys(&table[3])[0].label, "123");
        }
    }

    #[test]
    fn the_return_key_commits() {
        let table = rows(KeyboardMode::Lower);
        let bottom = row_keys(&table[3]);

        assert_eq!(bottom[3].label, Icon::KEYBOARD_RETURN);
        assert_eq!(bottom[3].action, KeyAction::Enter);
        assert_eq!(bottom[3].kind, KeyKind::Return);
        assert_eq!(bottom[1].action, KeyAction::Space);
        assert_eq!(bottom[2].action, KeyAction::Char('.'));
    }

    #[test]
    fn the_delete_key_is_backspace_icon() {
        for mode in [
            KeyboardMode::Lower,
            KeyboardMode::Upper,
            KeyboardMode::Numbers,
            KeyboardMode::Symbols,
        ] {
            let table = rows(mode);
            let row3 = row_keys(&table[2]);
            let del = row3.last().unwrap();
            assert_eq!(del.label, Icon::BACKSPACE);
            assert_eq!(del.action, KeyAction::Backspace);
        }
    }

    /// The space key is the widest, and every share is a multiple of a letter key's.
    ///
    /// This is what replaced the geometry: the proportions used to be worked out from a band width
    /// in pixels, and they are shares now — so the one thing worth asserting about them is that the
    /// relationships the original had are still in the table.
    #[test]
    fn the_shares_keep_the_originals_relationships() {
        let table = rows(KeyboardMode::Lower);
        let bottom = row_keys(&table[3]);
        let space = bottom[1].portion;

        assert!(
            space > bottom[0].portion,
            "space is wider than the mode key"
        );
        assert!(
            bottom[3].portion > bottom[0].portion,
            "return is wider than it"
        );

        // Shift and del are the widest keys of their row on the letter pages.
        let row3 = row_keys(&table[2]);

        assert!(
            row3[0].portion > row3[1].portion,
            "shift is wider than a letter"
        );
        assert_eq!(
            row3[0].portion,
            row3[row3.len() - 1].portion,
            "del matches it"
        );
    }
}
