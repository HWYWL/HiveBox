//! Just enough JSON to read a manifest.
//!
//! The board has no JSON library and this page needs four strings out of one document, so this is a
//! reader for *one shape* rather than a parser: an object whose members may be objects, and whose
//! leaves are strings and numbers. Nothing here builds a value or owns a byte — [`member`] hands back a
//! slice of the document it was given, which is what makes reading a field cost nothing and what keeps
//! a 7 MB firmware image out of this module's way.
//!
//! What it is not: it does not decode escapes, it does not read arrays into anything, and it does not
//! validate. A manifest that is not JSON at all comes back as `None` from every field, which is the
//! same answer as a manifest missing that field — and the page draws the same line for both, because
//! they are the same thing to a person: the repository does not say what it has.

/// The value of `key`, anywhere in `json`, as the bytes it is written with.
///
/// "Anywhere" is at any depth, which is what makes a two-step read of a nested manifest work: ask for
/// `"firmware"`, take the object that comes back, and ask *it* for `"file"`. The step is not a
/// convenience — a flat search for `"file"` on the whole document would find the font's as readily as
/// the firmware's, and the two are different downloads.
///
/// A member whose value was skipped is *stepped over* rather than descended into, which is the other
/// half of the same guarantee: asking for `"font"` does not stop at the `"file"` inside the firmware.
pub fn member<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = json;

    while let Some(quote) = rest.find('"') {
        let (name, after) = string_token(&rest[quote + 1..])?;
        let after = after.trim_start();

        let Some(value_start) = after.strip_prefix(':') else {
            // A string that was not a member name: inside a value this module has already skipped, or
            // the document is not the shape it claims to be. Either way, carry on after it.
            rest = after;
            continue;
        };

        let value_start = value_start.trim_start();
        let (value, after_value) = value(value_start)?;

        if name == key {
            return Some(value);
        }

        rest = after_value;
    }

    None
}

/// The value of `key` as a string, unquoted. `None` when the member is missing, or is not a string.
pub fn string<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let value = member(json, key)?.trim();
    let inner = value.strip_prefix('"')?;
    let (text, _) = string_token(inner)?;

    Some(text)
}

/// The value of `key` as a number of bytes. `None` when the member is missing or is not one.
///
/// u64 and not f64, because the one number this page reads off a manifest is a file's size, and a size
/// is a count.
pub fn number(json: &str, key: &str) -> Option<u64> {
    member(json, key)?.trim().parse().ok()
}

/// A string's contents and what follows its closing quote, given everything after its opening one.
///
/// Backslash escapes are stepped over so that an escaped quote does not end the string, but they are
/// not decoded: a manifest is written by a build script, and a file name in it has no escapes to
/// decode.
fn string_token(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let mut at = 0;

    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return Some((&s[..at], &s[at + 1..])),
            _ => at += 1,
        }
    }

    None
}

/// A value and what follows it, given everything from its first byte.
///
/// The four shapes JSON has here: an object or an array, which is matched to its closing bracket with
/// the strings inside it stepped over; a string; and a number, `true`, `false` or `null`, which ends at
/// the first byte that cannot be part of it.
fn value(s: &str) -> Option<(&str, &str)> {
    let bytes = s.as_bytes();
    let first = *bytes.first()?;

    match first {
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut at = 0;

            while at < bytes.len() {
                match bytes[at] {
                    b'"' => {
                        let (_, tail) = string_token(&s[at + 1..])?;
                        at = s.len() - tail.len();
                        continue;
                    }
                    // Both kinds of bracket count towards the same depth, because an object holding an
                    // array of objects is one value and this is a reader that must come back with all
                    // of it rather than with the first `}` that happens to close something.
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;

                        if depth == 0 {
                            return Some((&s[..at + 1], &s[at + 1..]));
                        }
                    }
                    _ => {}
                }

                at += 1;
            }

            None
        }
        b'"' => {
            let (text, tail) = string_token(&s[1..])?;

            // The slice is taken from `s` rather than from `text`, so that what comes back is the value
            // *with* its quotes: a caller that asked for a member gets what is written there, and
            // [`string`] is the one that takes them off.
            Some((&s[..text.len() + 2], tail))
        }
        _ => {
            let end = s
                .find(|c: char| c == ',' || c == '}' || c == ']' || c.is_whitespace())
                .unwrap_or(s.len());

            Some((&s[..end], &s[end..]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The manifest this project publishes, cut down to the fields the page reads.
    const MANIFEST: &str = r#"{
        "generated": "2026-10-16T09:00:00Z",
        "firmware": {
            "version": "0.7.0",
            "file": "hivebox-firmware-v0.7.0.bin",
            "size": 7222464,
            "sha256": "3f2a"
        },
        "font": {
            "version": "1",
            "file": "hivebox-font-24px-v1.bin",
            "size": 2080400,
            "sha256": "9c41"
        }
    }"#;

    /// A member is found at any depth, and a member's value is skipped rather than descended into: the
    /// firmware's `"file"` is not what a search for the font's finds.
    #[test]
    fn a_member_is_read_out_of_the_object_that_holds_it() {
        let firmware = member(MANIFEST, "firmware").expect("the firmware's object");
        let font = member(MANIFEST, "font").expect("the font's object");

        assert_eq!(string(firmware, "file"), Some("hivebox-firmware-v0.7.0.bin"));
        assert_eq!(string(font, "file"), Some("hivebox-font-24px-v1.bin"));
        assert_eq!(string(firmware, "sha256"), Some("3f2a"));
        assert_eq!(number(firmware, "size"), Some(7_222_464));
        assert_eq!(number(font, "size"), Some(2_080_400));
    }

    /// A member the document does not have, and one whose value is the wrong kind, are both `None`.
    #[test]
    fn a_missing_or_mistyped_member_is_nothing() {
        assert_eq!(string(MANIFEST, "absent"), None);
        assert_eq!(number(MANIFEST, "file"), None);
        assert_eq!(string(MANIFEST, "size"), None);
        assert_eq!(member("", "firmware"), None);
        assert_eq!(member("not json at all", "firmware"), None);
    }

    /// The one shape that would catch a scanner that counted braces without watching quotes: a brace
    /// inside a string.
    #[test]
    fn a_brace_inside_a_string_does_not_end_an_object() {
        let json = r#"{"font": {"note": "a } in a string", "file": "hivebox-font-24px-v1.bin"}}"#;
        let font = member(json, "font").expect("the font's object");

        assert_eq!(string(font, "file"), Some("hivebox-font-24px-v1.bin"));
    }

    /// An escaped quote does not end a string, and a file name may not be truncated at one.
    ///
    /// The read is the two-step one the module is built around — the member's object first, then the
    /// field inside it — because a member's value is stepped over rather than descended into.
    #[test]
    fn an_escaped_quote_does_not_end_a_string() {
        let json = r#"{"font": {"file": "a\"b.bin"}}"#;
        let font = member(json, "font").expect("the font's object");

        assert_eq!(string(font, "file"), Some(r#"a\"b.bin"#));
    }
}
