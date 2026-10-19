//! Formatting numbers into a fixed buffer, without an allocator.
//!
//! An application has no heap, so a clock cannot build a `String` and a calculator cannot keep its
//! expression in one. [`Buf`] is the answer: a `&mut [u8]` with a length, implementing
//! [`core::fmt::Write`], used with [`format_into`] and then borrowed as a `&str` for exactly as long
//! as the call that draws it.
//!
//! ```ignore
//! let mut buf = Buf::<32>::new();
//! let label = format_into!(buf, "{:02}:{:02}", hours, minutes);
//! ui.text(24.0, 24.0, label, 40.0, theme::TEXT);
//! ```

use core::fmt;

/// A fixed-size UTF-8 buffer that implements [`fmt::Write`].
///
/// Writes that do not fit are dropped rather than refused: a label that is one character short is a
/// screen that is still readable, and a formatting call that returns an error in an application with
/// no way to handle it is not.
pub struct Buf<'a> {
    bytes: &'a mut [u8],
    len: usize,
}

impl<'a> Buf<'a> {
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, len: 0 }
    }

    /// The bytes written so far, as a string. Always valid UTF-8: [`fmt::Write::write_str`] only
    /// writes whole strings.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }

    /// Empties the buffer, keeping its storage.
    pub fn reset(&mut self) {
        self.len = 0;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl fmt::Write for Buf<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let room = self.bytes.len() - self.len;
        let mut len = text.len().min(room);

        while len > 0 && !text.is_char_boundary(len) {
            len -= 1;
        }

        self.bytes[self.len..self.len + len].copy_from_slice(&text.as_bytes()[..len]);
        self.len += len;

        Ok(())
    }
}

/// Formats into a [`Buf`] and returns the string it now holds.
#[macro_export]
macro_rules! format_into {
    ($buf:expr, $($arg:tt)*) => {{
        $buf.reset();
        let _ = ::core::fmt::Write::write_fmt(&mut $buf, ::core::format_args!($($arg)*));
        $buf.as_str()
    }};
}

/// A [`Buf`] over a stack array, for callers that do not want to name the array.
///
/// ```ignore
/// let mut storage = [0u8; 32];
/// let mut buffer = Buf::new(&mut storage);
/// ```
pub type SmallBuf = [u8; 32];

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write;

    #[test]
    fn numbers_are_formatted_into_the_buffer() {
        let mut storage = [0u8; 16];
        let mut buffer = Buf::new(&mut storage);

        write!(&mut buffer, "{:02}:{:02}", 9, 5).unwrap();
        assert_eq!(buffer.as_str(), "09:05");
        assert_eq!(buffer.len(), 5);

        buffer.reset();
        assert_eq!(buffer.as_str(), "");
    }

    /// Text that does not fit is cut at a character boundary: the buffer is never left holding half
    /// a UTF-8 sequence, which would draw a replacement glyph.
    #[test]
    fn text_is_cut_where_a_character_ends() {
        let mut storage = [0u8; 4];
        let mut buffer = Buf::new(&mut storage);

        write!(&mut buffer, "中文").unwrap();
        assert_eq!(buffer.as_str(), "中");

        // A further write has no room left and changes nothing rather than appending half a
        // character.
        write!(&mut buffer, "文").unwrap();
        assert_eq!(buffer.as_str(), "中");
    }

    #[test]
    fn the_format_macro_returns_what_it_formatted() {
        let mut storage = [0u8; 24];
        let mut buffer = Buf::new(&mut storage);
        let text = format_into!(buffer, "{:.1}%", 84.25);

        assert_eq!(text, "84.2%");
    }
}
