//! The screen at the other end of a session: what a machine's bytes do to a page.
//!
//! A shell across the network does not send text. It sends a *stream*: characters, carriage returns,
//! bells, and escape sequences that move a cursor, erase what was drawn before and choose a colour. A
//! terminal's job is to be the thing that turns that back into a page, and this is that thing.
//!
//! # What this screen is, and the one thing it is not
//!
//! It is a **page that a stream appends to**, with the one exception that a carriage return puts the
//! cursor back at the start of the line it is on — which is what a prompt, a compiler's progress and
//! every `docker pull` bar are made of. Everything a person does at a shell prompt draws this way:
//! `ls`, `df -h`, `docker ps`, `git status`, a `cat` that runs off the screen.
//!
//! What it is *not* is a fixed grid of cells with a cursor free to move around it, and that is not a
//! decision that was taken here — it is the panel. A grid can only line up in a font whose every
//! character is the same width, and this app has no such font: [`style::measure`](crate::style::measure)
//! is a deliberate estimate (iced's shaper is unreachable from `view`) over a proportional face. A
//! screen that honoured `ESC [ 10 ; 20 H` would put the cursor of a `vi` somewhere that is not where
//! the glyphs of a `vi` land, which is worse than not honouring it at all. So cursor addressing is
//! *dropped* — see [`Screen::csi`] — and with it the programs built on it: `vi`, `top`, `htop`, `less`
//! when it takes the screen. They are the price of a proportional font, and they are worth saying out
//! loud rather than discovering in front of a server that will not stop.
//!
//! Which leaves a screen that is honest about being one: everything above it — the commands, the
//! colours, the erasing, the carriage-return redraw — works, and what is left over is ignored rather
//! than drawn wrong.

use crate::shell::Rgb;

/// The sixteen colours an SGR sequence can name, as the VS Code terminal paints them: the palette this
/// app's own transcript already imitates, kept in one place so a `ls` is the same green here as it is
/// on a laptop.
const ANSI: [(u8, u8, u8); 16] = [
    (0x00, 0x00, 0x00),
    (0xcd, 0x31, 0x31),
    (0x0d, 0xbc, 0x79),
    (0xe5, 0xe5, 0x10),
    (0x24, 0x72, 0xc8),
    (0xbc, 0x3f, 0xbc),
    (0x11, 0xa8, 0xcd),
    (0xe5, 0xe5, 0xe5),
    (0x66, 0x66, 0x66),
    (0xf1, 0x4c, 0x4c),
    (0x23, 0xd1, 0x8b),
    (0xf5, 0xf5, 0x43),
    // #569CD6, which is [`crate::shell::VS_PROMPT_DIR`] — the blue this app already draws a directory
    // in, so that a `ls` coloured by the machine and a prompt coloured by this app agree.
    (0x56, 0x9c, 0xd6),
    (0xd6, 0x70, 0xd6),
    (0x29, 0xb8, 0xdb),
    (0xe5, 0xe5, 0xe5),
];

/// How many lines the screen keeps. Past this the oldest go, which is what a scrollback is.
///
/// 120 lines of a full 80 characters is 9600 cells, and a cell is a character and an optional colour —
/// about 77 KB. That is the whole cost of a terminal's memory on this board, which is why the number is
/// stated here rather than left to grow: a `yes` typed into a session would otherwise fill the heap.
pub const MAX_LINES: usize = 120;

/// What the panel can show of a line before it is cut off. The app wraps what it draws, so this is only
/// a bound on a pathological line — a `base64` of a file, say — and not the width of anything.
const MAX_COLUMNS: usize = 512;

/// One cell of the page: a character, and the colour the machine had chosen when it printed it.
///
/// The colour is kept per character and not per line because that is what a stream gives: `ls` sets a
/// colour, prints a name, and sets another, all within one line. Adjacent cells that agree become one
/// run at drawing time — see [`Screen::lines`] — so a page of one colour costs one draw call a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The character. A space is a character like any other here: a machine that printed one meant it.
    pub ch: char,
    /// The colour SGR had chosen, or `None` for the terminal's own foreground.
    pub color: Option<Rgb>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            color: None,
        }
    }
}

/// The colour `SGR` has chosen, as the machine *stated* it: a palette entry by number, or a colour
/// given outright. Deliberately not resolved into an [`Rgb`] when the sequence is read — see
/// [`Screen::put`] — because bold arrives in the same sequence and in either order, and `34;1` has to
/// mean what `1;34` means. Which of the two it was survives until a character is drawn, which is when
/// the bold is finally known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Colour {
    /// One of the sixteen entries of the palette, which bold can brighten.
    Palette(u16),
    /// A colour given as bytes: `38;5;n` past the sixteen, or `38;2;r;g;b`. Nothing here to brighten —
    /// the machine said exactly what it wanted.
    True(Rgb),
}

/// How far into an escape sequence a stream is.
///
/// State, because an escape sequence is only a sequence taken together: the `[` and the `m` of
/// `ESC [ 31 m` are ordinary characters to a screen that has not seen the `ESC`. Which is why the
/// machine's own output can never be fed through a filter — the filter and the drawing are the same
/// pass or they are wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Escape {
    /// Not in one, which is where nearly every byte is.
    None,
    /// Just past an `ESC`: the next byte says what kind of sequence this is.
    Start,
    /// Inside `ESC [ …`, collecting parameters until the byte that ends it.
    Csi,
    /// Inside an `ESC ] …`: a window title or a palette being set. It runs until `BEL` or `ESC \`, and
    /// both are handled because both are sent.
    Osc,
    /// Inside an `OSC` and just past an `ESC`, which means the `\` that ends it comes next.
    OscEnd,
    /// Inside `ESC ( …` and the like: a character set named in one byte, which this screen has only one
    /// font for.
    Charset,
}

/// A page a machine's bytes have been drawn into.
pub struct Screen {
    /// Every line, oldest first, and always at least one. The last is the one being written into: a
    /// stream that only appends — the shape this screen supports — is always writing at the end.
    lines: Vec<Vec<Cell>>,
    /// Where on that last line the next character goes, counted in characters.
    col: usize,
    /// The colour SGR has chosen, until it chooses another. Unresolved on purpose: see [`Colour`].
    color: Option<Colour>,
    /// Whether SGR has asked for bold. Kept apart from the colour because the two arrive in either
    /// order and a `1` before a `31` means the same thing as a `31` before a `1`.
    bold: bool,
    /// How far into an escape sequence the stream is.
    escape: Escape,
    /// The parameters of the sequence being read, reused from one to the next: a `ls` of a colour
    /// terminal sends an `SGR` per entry, and a fresh `Vec` for each would be an allocation per file.
    params: Vec<u16>,
    /// The bytes of a character that have arrived but do not make one yet. See [`Screen::feed`].
    pending: Vec<u8>,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    /// An empty page: one blank line and a cursor at the start of it.
    pub fn new() -> Self {
        Self {
            lines: vec![Vec::new()],
            col: 0,
            color: None,
            bold: false,
            escape: Escape::None,
            params: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// Draws a chunk of what a machine said.
    ///
    /// A chunk and not a line, because that is what arrives: a shell's output is whatever the network
    /// had ready, and half a character is a thing that happens — which is why this takes bytes and
    /// keeps [`Screen::pending`] across calls rather than being handed a `&str`.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            match self.escape {
                Escape::None => self.plain(byte),
                // `ESC [` and `ESC ]` open the two sequences that carry payload; every other byte after
                // an `ESC` is the whole sequence, which is what a character set or a key like `ESC O A`
                // is.
                Escape::Start => {
                    self.escape = match byte {
                        b'[' => Escape::Csi,
                        b']' => Escape::Osc,
                        b'(' | b')' | b'*' | b'+' => Escape::Charset,
                        _ => Escape::None,
                    }
                }
                Escape::Csi => self.csi(byte),
                Escape::Osc => match byte {
                    0x07 => self.escape = Escape::None,
                    0x1b => self.escape = Escape::OscEnd,
                    _ => {}
                },
                Escape::OscEnd => self.escape = Escape::None,
                Escape::Charset => self.escape = Escape::None,
            }
        }
    }

    /// What is on the page, as text: one entry per line, trimmed of nothing.
    ///
    /// Trimmed of nothing on purpose — a `df -h` is columns and the spaces between them *are* the
    /// table — so a trailing empty line is an empty string and not a missing one.
    pub fn lines(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|line| line.iter().map(|cell| cell.ch).collect())
            .collect()
    }

    /// What is on the page, with each line's colours: the text of a run and the colour it was drawn in,
    /// adjacent characters of one colour merged into one run.
    ///
    /// Built at drawing time rather than kept as runs, because runs are what a screen is *drawn* with
    /// and cells are what it is *edited* with: a carriage return rewriting the middle of a line is easy
    /// in cells and is a merge and a split in runs.
    pub fn runs(&self) -> Vec<Vec<(String, Option<Rgb>)>> {
        self.lines
            .iter()
            .map(|line| {
                let mut runs: Vec<(String, Option<Rgb>)> = Vec::new();

                for cell in line {
                    match runs.last_mut() {
                        Some((text, color)) if *color == cell.color => text.push(cell.ch),
                        _ => runs.push((String::from(cell.ch), cell.color)),
                    }
                }

                runs
            })
            .collect()
    }

    /// How many lines there are, which is what a viewport's height is measured in.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether nothing has been drawn at all.
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Throws the page away, for a session starting over or a `clear` that asked for it.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.lines.push(Vec::new());
        self.col = 0;
    }

    /// Draws one byte that is not part of an escape sequence, which is nearly all of them.
    fn plain(&mut self, byte: u8) {
        match byte {
            0x1b => self.escape = Escape::Start,
            b'\n' => self.new_line(),
            // A carriage return is the one cursor movement that works here, and it is the one that
            // matters: a progress bar, a spinner and a prompt being redrawn all come back to the start
            // of the line they are on rather than going somewhere else.
            b'\r' => self.col = 0,
            // A backspace, which is how a shell un-draws the last character of what was typed.
            0x08 => self.col = self.col.saturating_sub(1),
            // The bell. The board has a speaker and it is not wired to this, deliberately: a terminal
            // that beeped at the panel across the room would be a terminal nobody leaves open.
            0x07 => {}
            _ if byte < 0x20 => {}
            // A character, which may be the first byte of one rather than the whole of it.
            _ => self.character(byte),
        }
    }

    /// Collects one byte of a character, and draws it when there is a character to draw.
    fn character(&mut self, byte: u8) {
        self.pending.push(byte);

        match std::str::from_utf8(&self.pending) {
            Ok(text) => {
                let drawn: Vec<char> = text.chars().collect();
                self.pending.clear();

                for ch in drawn {
                    self.put(ch);
                }
            }
            // Not a character *yet*: a character can straddle two reads, and the four bytes of the
            // longest one are not here. Waiting is the whole reason this screen takes bytes.
            Err(error) if error.error_len().is_none() && self.pending.len() < 4 => {}
            // Not a character at all. Said so and dropped, the way a terminal does, rather than left in
            // the buffer to corrupt everything after it.
            Err(_) => {
                self.pending.clear();
                self.put('\u{fffd}');
            }
        }
    }

    /// Draws one character at the cursor and moves past it.
    ///
    /// The colour is resolved *here* and not when the `SGR` was read, which is the whole point of
    /// [`Colour`]: by the time a character arrives the sequence that chose the colour is over, so the
    /// bold is known and `34;1` draws the bright blue that `1;34` does.
    fn put(&mut self, ch: char) {
        if self.col >= MAX_COLUMNS {
            return;
        }

        let color = self.color.map(|colour| match colour {
            Colour::Palette(index) => self.pick(index),
            Colour::True(rgb) => rgb,
        });
        let line = self.lines.last_mut().expect("a screen always has a line");

        // Past the end of what is there: a carriage return may have brought the cursor back into the
        // middle of a line that has since been drawn longer, so this writes over what it finds and
        // pads with spaces where it finds nothing.
        while line.len() < self.col {
            line.push(Cell::default());
        }

        let cell = Cell { ch, color };

        if self.col < line.len() {
            line[self.col] = cell;
        } else {
            line.push(cell);
        }

        self.col += 1;
    }

    /// Moves to a new line, which is the only way this screen grows.
    ///
    /// "New line" and not "next line": a stream does not say where it is going, and this screen has no
    /// cursor to take anywhere. What it has is a page that appends, which is what a shell's output is.
    fn new_line(&mut self) {
        self.col = 0;
        self.lines.push(Vec::new());

        // The scrollback's end: the oldest line goes, which is what a terminal has always done with it.
        if self.lines.len() > MAX_LINES {
            self.lines.remove(0);
        }
    }

    /// Reads one byte of an `ESC [ …` sequence, and does what it says when the byte that ends it comes.
    ///
    /// This is where the honest limit lives. `m` (colour), `K` and `J` (erasing) are what a stream
    /// appending to a page uses, and all three are here. Everything else in the standard — `H` and `f`
    /// (go to a position), `A`–`D` (go a few cells), `G` and `d` (go to a column or a row), `r` (a
    /// scrolling region), `s` and `u` (remember and go back), and the `?`-prefixed modes an alternate
    /// screen is opened with — is a cursor being told where to go, and a page with no grid has nowhere
    /// to take it. They are read and dropped rather than half-honoured: a cursor that moved a row and
    /// drew nothing, or drew in the wrong place, is worse than one that stayed put. Which is why `vi`
    /// and `top` do not work here — see the module documentation.
    fn csi(&mut self, byte: u8) {
        match byte {
            // A digit continues the parameter being read. The `min` is not paranoia: a stream can send
            // any digits it likes, and the parameters are `u16`.
            b'0'..=b'9' => {
                let value = self.params.pop().unwrap_or(0);
                self.params
                    .push((value * 10 + u16::from(byte - b'0')).min(9999));
            }
            // A separator: a parameter with nothing before it is a nought, which is what `ESC [ ; H`
            // means and what `ESC [ 31 ; 1 m` relies on.
            b';' => {
                if self.params.is_empty() {
                    self.params.push(0);
                }
                self.params.push(0);
            }
            // The private markers (`?` for a mode, `<` and `=` for what a keyboard sends) and the
            // intermediate bytes before a final one. Read and dropped, because everything they appear in
            // is dropped too — but *dropped* and not ended, which matters: a `?` taken for the end of the
            // sequence would leave the `25l` of `ESC [ ? 25 l` drawn on the page as text.
            0x20..=0x2f | 0x3c..=0x3f => {}
            // The byte that ends it.
            0x40..=0x7e => self.ended(byte),
            // Not part of any sequence: give up on it rather than reading the rest of the stream as
            // though it were.
            _ => {
                self.params.clear();
                self.escape = Escape::None;
            }
        }
    }

    /// Does what the byte that ended an `ESC [ …` sequence says, for the three things a page can do.
    fn ended(&mut self, byte: u8) {
        match byte {
            b'm' => self.sgr(),
            b'K' => self.erase_line(),
            b'J' => self.erase_display(),
            _ => {}
        }

        self.params.clear();
        self.escape = Escape::None;
    }

    /// `ESC [ … m`: colours, which are the one thing a stream says constantly and the one thing a page
    /// can honour exactly — a colour does not need a grid.
    fn sgr(&mut self) {
        // `ESC [ m` with nothing in it is a reset: an empty list and a nought mean the same thing.
        if self.params.is_empty() {
            self.reset_sgr();
            return;
        }

        // Taken rather than borrowed because a colour is chosen with `self`, and given back at the end so
        // that the next sequence reuses the allocation. An `SGR` per file in a directory listing is not
        // a place to be allocating.
        let params = std::mem::take(&mut self.params);
        let mut rest = params.iter().copied();

        while let Some(param) = rest.next() {
            match param {
                0 => self.reset_sgr(),
                // Bold, which a terminal reads as "brighter": `ls` colours a directory with `1;34`, and a
                // screen that ignored the `1` would draw it the same blue as everything else.
                1 => self.bold = true,
                22 => self.bold = false,
                30..=37 => self.color = Some(Colour::Palette(param - 30)),
                39 => self.color = None,
                90..=97 => self.color = Some(Colour::Palette(param - 90 + 8)),
                // `38;5;n` and `38;2;r;g;b`, which carry their own parameters and so have to be read out
                // of the same list.
                38 => {
                    // A malformed tail — `38;9` and the like — leaves the colour as it was rather than
                    // guessing, and the parameters it swallowed are the list's to be finished with.
                    if let Some(colour) = extended(&mut rest) {
                        self.color = Some(colour);
                    }
                }
                // The background, and the rest of what `SGR` can say — underlining, inverse, blinking.
                // Read past: a page with one font can say a colour and not much more, and half an
                // underline is worse than a plain word.
                _ => {}
            }
        }

        self.params = params;
    }

    /// The colour of the sixteen-colour palette's entry `index`, brightened if bold was asked for.
    ///
    /// The pair is what makes a coloured `ls` legible: the eight original colours are dark on a black
    /// panel, and a terminal that ignored bold would draw every directory in the same muddy blue.
    fn pick(&self, index: u16) -> Rgb {
        let index = if self.bold && index < 8 {
            index + 8
        } else {
            index
        };

        ansi(index)
    }

    /// `ESC [ K`: erase in the line the cursor is on. Nought and two are what a redraw is made of.
    fn erase_line(&mut self) {
        let mode = self.params.first().copied().unwrap_or(0);
        let col = self.col;
        let line = self.lines.last_mut().expect("a screen always has a line");

        match mode {
            // From the cursor to the end of the line.
            0 => line.truncate(col),
            // From the start of the line to the cursor.
            1 => {
                for cell in line.iter_mut().take(col) {
                    *cell = Cell::default();
                }
            }
            // The whole line, leaving the cursor where it is.
            2 => line.clear(),
            _ => {}
        }
    }

    /// `ESC [ J`: erase in the page. Two is the `clear` command; nought is a program taking back what it
    /// just printed, which is what an erasing progress line does.
    fn erase_display(&mut self) {
        match self.params.first().copied().unwrap_or(0) {
            0 => {
                let col = self.col;

                if let Some(line) = self.lines.last_mut() {
                    line.truncate(col);
                }
            }
            // Two erases the page, and three erases the page and the scrollback with it — which is what
            // `clear` means, and the difference between the two is not worth a second screen state here.
            2 | 3 => self.clear(),
            _ => {}
        }
    }

    /// Back to the terminal's own foreground, which is what a nought in an `SGR` means.
    fn reset_sgr(&mut self) {
        self.color = None;
        self.bold = false;
    }
}

/// The colour of one of the sixteen entries, in range and not out of it: a stream is not trusted to be
/// well formed, and an index of 200 is a `ls` that should still draw something.
fn ansi(index: u16) -> Rgb {
    let (r, g, b) = ANSI[usize::from(index.min(15))];

    Rgb { r, g, b }
}

/// The colour of an `SGR`'s extended form: `5;n` for the 256-colour palette, or `2;r;g;b` for true
/// colour, which this panel can draw exactly and so is kept exactly.
fn extended(params: &mut impl Iterator<Item = u16>) -> Option<Colour> {
    match params.next()? {
        5 => params.next().map(eight_bit),
        2 => {
            let r = params.next()?.min(255) as u8;
            let g = params.next()?.min(255) as u8;
            let b = params.next()?.min(255) as u8;

            Some(Colour::True(Rgb { r, g, b }))
        }
        _ => None,
    }
}

/// One of the 256 colours of `38;5;n`: the sixteen above, then a cube, then a ramp.
///
/// Worth the twenty lines because it is what a machine actually sends: `ls --color` and a great deal of
/// application output name their colours by number rather than by the sixteen names, and a screen that
/// dropped those would draw a coloured listing in one flat grey.
///
/// The first sixteen come back as [`Colour::Palette`] rather than as their [`Rgb`], so that `38;5;4`
/// brightens under bold exactly as `34` does — a machine has no way of knowing which of the two forms
/// this screen prefers, and they have to mean the same thing.
fn eight_bit(index: u16) -> Colour {
    match index {
        0..=15 => Colour::Palette(index),
        16..=231 => {
            // A 6×6×6 cube, and the levels are not evenly spaced — the first step is the big one, which
            // is why this is a table and not a multiply.
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let cube = index - 16;

            Colour::True(Rgb {
                r: LEVELS[usize::from(cube / 36)],
                g: LEVELS[usize::from((cube / 6) % 6)],
                b: LEVELS[usize::from(cube % 6)],
            })
        }
        // A grey ramp, `#080808` up to `#EEEEEE` in even steps.
        _ => {
            let grey = 8 + (index.min(255) - 232) as u8 * 10;

            Colour::True(Rgb {
                r: grey,
                g: grey,
                b: grey,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen with one chunk of a machine's output drawn into it.
    fn fed(bytes: &[u8]) -> Screen {
        let mut screen = Screen::new();
        screen.feed(bytes);

        screen
    }

    /// What the page says, one string a line.
    fn lines(bytes: &[u8]) -> Vec<String> {
        fed(bytes).lines()
    }

    /// Text lands on the page, and an empty page is one blank line rather than none.
    #[test]
    fn text_lands_on_the_page() {
        assert_eq!(Screen::new().lines(), vec![""]);
        assert_eq!(lines(b"hello"), vec!["hello"]);
    }

    /// A newline starts a line: the shape every command's output has.
    #[test]
    fn a_newline_starts_a_line() {
        assert_eq!(lines(b"one\r\ntwo\r\n"), vec!["one", "two", ""]);
        assert_eq!(lines(b"one\ntwo"), vec!["one", "two"]);
    }

    /// A carriage return goes back to the start of the line and writes over what is there — which is
    /// what a progress bar is, and the only cursor movement this screen has.
    #[test]
    fn a_carriage_return_writes_over_the_line() {
        assert_eq!(lines(b"10%\r20%"), vec!["20%"], "a bar redrawing itself");
        assert_eq!(
            lines(b"abcdef\rxy"),
            vec!["xycdef"],
            "and only as much as it overwrote"
        );
    }

    /// A backspace moves back, and the next character lands where it left.
    #[test]
    fn a_backspace_moves_back() {
        assert_eq!(lines(b"ab\x08c"), vec!["ac"]);
    }

    /// Colours are kept per character, and adjacent characters of one colour are drawn as one run.
    #[test]
    fn colours_come_out_as_runs() {
        let screen = fed(b"\x1b[31mred\x1b[0m plain");

        assert_eq!(
            screen.runs(),
            vec![vec![
                (String::from("red"), Some(ansi(1))),
                (String::from(" plain"), None),
            ]]
        );
    }

    /// Bold brightens the colour it is next to, in either order — which is what makes a coloured `ls`
    /// legible on a black panel.
    #[test]
    fn bold_brightens_a_colour() {
        assert_eq!(fed(b"\x1b[1;34mx").runs()[0][0].1, Some(ansi(12)));
        assert_eq!(fed(b"\x1b[34;1mx").runs()[0][0].1, Some(ansi(12)));
        assert_eq!(fed(b"\x1b[34mx").runs()[0][0].1, Some(ansi(4)));
        assert_eq!(fed(b"\x1b[1;34m\x1b[22mx").runs()[0][0].1, Some(ansi(4)));
        // And the same blue said the other way round, because a machine has no way of knowing which
        // form this screen prefers.
        assert_eq!(fed(b"\x1b[38;5;4mx").runs()[0][0].1, Some(ansi(4)));
        assert_eq!(fed(b"\x1b[1;38;5;4mx").runs()[0][0].1, Some(ansi(12)));
    }

    /// True colour is kept exactly, because this panel can draw it exactly.
    #[test]
    fn a_true_colour_is_kept() {
        assert_eq!(
            fed(b"\x1b[38;2;10;20;30mx").runs()[0][0].1,
            Some(Rgb {
                r: 10,
                g: 20,
                b: 30
            })
        );
    }

    /// The 256-colour palette a machine actually sends, in its three parts.
    #[test]
    fn an_eight_bit_colour_is_read() {
        // The cube's brightest red.
        assert_eq!(fed(b"\x1b[38;5;196mx").runs()[0][0].1, Some(Rgb { r: 255, g: 0, b: 0 }));
        // The first sixteen are the ones above.
        assert_eq!(fed(b"\x1b[38;5;3mx").runs()[0][0].1, Some(ansi(3)));
        // And the ramp at the end of it.
        assert_eq!(fed(b"\x1b[38;5;232mx").runs()[0][0].1, Some(Rgb { r: 8, g: 8, b: 8 }));
    }

    /// A sequence this screen cannot honour is dropped, and — the part that matters — is not drawn.
    #[test]
    fn a_sequence_that_cannot_be_honoured_is_not_drawn() {
        assert_eq!(lines(b"\x1b[10;20Hx"), vec!["x"], "cursor addressing");
        assert_eq!(lines(b"\x1b[2A\x1b[Ky"), vec!["y"], "and moving about the page");
        assert_eq!(lines(b"\x1b[?25lz"), vec!["z"], "and the private modes");
        assert_eq!(lines(b"\x1b]0;a title\x07w"), vec!["w"], "and a window title");
        assert_eq!(lines(b"\x1b(Bv"), vec!["v"], "and a character set");
    }

    /// `ESC [ K` takes back the rest of the line, which is what an erasing prompt does.
    #[test]
    fn erasing_the_line_leaves_what_was_there() {
        assert_eq!(lines(b"abcdef\x08\x08\x1b[K"), vec!["abcd"]);
        assert_eq!(lines(b"abcdef\x1b[1K"), vec!["      "]);
        assert_eq!(lines(b"abcdef\x1b[2K"), vec![""]);
    }

    /// `ESC [ 2 J` is a `clear`: the page is one blank line again.
    #[test]
    fn clearing_the_page_empties_it() {
        let mut screen = fed(b"one\r\ntwo");
        assert!(!screen.is_empty());

        screen.feed(b"\x1b[2J");

        assert_eq!(screen.lines(), vec![""]);
        assert!(screen.is_empty());
        assert_eq!(screen.len(), 1);
    }

    /// A character can arrive in pieces, which is the whole reason this takes bytes: a Chinese
    /// character is three of them and the network does not care where the breaks fall.
    #[test]
    fn a_character_split_across_two_reads() {
        let mut screen = Screen::new();
        let bytes = "你".as_bytes();

        screen.feed(&bytes[..1]);
        assert_eq!(screen.lines(), vec![""], "not a character yet");

        screen.feed(&bytes[1..]);
        assert_eq!(screen.lines(), vec!["你"]);
    }

    /// A byte that is not a character is said so and dropped, rather than kept to corrupt what follows.
    #[test]
    fn a_byte_that_is_not_a_character_is_replaced() {
        assert_eq!(lines(b"a\xffb"), vec!["a\u{fffd}b"]);
    }

    /// The scrollback has an end, because a `yes` typed into a session otherwise fills the heap.
    #[test]
    fn the_scrollback_has_an_end() {
        let mut screen = Screen::new();

        for _ in 0..MAX_LINES * 2 {
            screen.feed(b"line\r\n");
        }

        assert_eq!(screen.len(), MAX_LINES);
    }

    /// A line that will not end is cut off, for the same reason.
    #[test]
    fn a_line_that_will_not_end_is_cut_off() {
        let mut screen = Screen::new();
        screen.feed(&vec![b'x'; MAX_COLUMNS * 2]);

        assert_eq!(screen.lines()[0].chars().count(), MAX_COLUMNS);
    }

    /// A control character that means nothing here is not drawn, and the letters after it are.
    #[test]
    fn a_control_character_is_not_drawn() {
        assert_eq!(lines(b"a\x07b\x00c"), vec!["abc"]);
    }
}
