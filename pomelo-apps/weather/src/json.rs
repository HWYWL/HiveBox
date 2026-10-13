//! Just enough JSON to read a forecast.
//!
//! # Why this is a module and not a dependency
//!
//! Open-Meteo answers with a JSON document, and nothing in this crate's graph can read one: the
//! three crates the apps share bring no parser, and every page before this one read its numbers out
//! of the board rather than out of a document. Pulling in a parser for one page's one document would
//! put a tree of crates in the firmware's image for the half of JSON a weather answer uses — which is
//! four of its seven kinds of value, and none of its interesting corners but the escapes.
//!
//! So this is a reader, and it is a *reader* rather than a validator: it answers what the document
//! says, and the page decides whether that is a forecast. Where it has an opinion, it has it in the
//! direction of refusing — a `\uD800` is not a character, an `inf` is not a number this page can
//! draw, and a second document after the first is not one document.
//!
//! # What it is not
//!
//! It builds a tree of the whole document before anyone reads a field. That is the right shape here
//! and the wrong one for a document of megabytes — the answer this page asks for is a kilobyte — and
//! the honest note for whoever needs the other shape: the reader is a `Reader` with a cursor in it,
//! and a streaming caller would keep the cursor rather than the tree.

/// A JSON document, as the values it is made of.
///
/// An object is a `Vec` of pairs rather than a map: a lookup is a scan of a handful of fields, and
/// keeping the order the service wrote is what lets a parser's own test say a document round-trips.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The field `key` of an object, or `None` for anything else.
    ///
    /// The one accessor a caller of a *document* needs, and the reason it returns an `Option` rather
    /// than a fault: "this document has no `daily`" and "this document has no `daily` in it" are the
    /// same sentence in the code that reads it.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The number this value is, or `None` if it is not one.
    ///
    /// `true` is not `1` here, which is the one place this is stricter than JavaScript: a boolean is
    /// not a reading, and a page that drew one as a temperature would be drawing the wrong number
    /// rather than a wrong kind of thing.
    pub fn number(&self) -> Option<f64> {
        match self {
            Json::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// The text this value is, or `None` if it is not text.
    pub fn string(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    /// The elements this value is, or `None` if it is not an array.
    pub fn array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }
}

/// Why a text was not a JSON document.
///
/// Four, and not one: they are four different mistakes — a body that was cut off, a body that is not
/// JSON at all, a string this reader will not decode, and two documents where one was expected — and
/// the tests are the only thing that reads them, which is what a closed list of *reasons* is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonFault {
    /// The text ended in the middle of a value.
    Truncated,
    /// A character where a value could not go, or a token that is not one of JSON's.
    Unexpected,
    /// An escape, or the text after one, that JSON does not define.
    Escape,
    /// A whole document that parsed, and then had more after it.
    Trailing,
}

/// Reads `text` as a JSON document.
pub fn parse(text: &str) -> Result<Json, JsonFault> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        at: 0,
    };

    reader.skip_space();
    let value = reader.value()?;

    // Trailing space is fine, trailing anything else is a second document: a body with two of them
    // in it is a body this page cannot be sure it read the right one of.
    reader.skip_space();
    if reader.at < reader.bytes.len() {
        return Err(JsonFault::Trailing);
    }

    Ok(value)
}

/// The cursor this reader is: the document's bytes, and where in them it is.
///
/// Bytes rather than `char`s, because every character JSON gives a *meaning* to is ASCII: the
/// multi-byte ones can only be inside a string, where they are copied through untouched. A string's
/// escapes are the one place a byte is not enough, and [`Reader::string`] reads them there.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    /// Steps over the whitespace JSON allows between values.
    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    /// The byte the cursor is on, without moving it.
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    /// The one value at the cursor, whatever kind it is.
    fn value(&mut self) -> Result<Json, JsonFault> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true").map(|_| Json::Bool(true)),
            Some(b'f') => self.literal("false").map(|_| Json::Bool(false)),
            Some(b'n') => self.literal("null").map(|_| Json::Null),
            Some(_) => self.number().map(Json::Number),
            None => Err(JsonFault::Truncated),
        }
    }

    /// An object, with the cursor on its `{`.
    fn object(&mut self) -> Result<Json, JsonFault> {
        self.at += 1; // `{`
        self.skip_space();

        let mut fields = Vec::new();

        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(fields));
        }

        loop {
            self.skip_space();
            let key = self.string()?;

            self.skip_space();
            self.expect(b':')?;

            self.skip_space();
            let value = self.value()?;
            fields.push((key, value));

            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(fields));
                }
                Some(_) => return Err(JsonFault::Unexpected),
                None => return Err(JsonFault::Truncated),
            }
        }
    }

    /// An array, with the cursor on its `[`.
    fn array(&mut self) -> Result<Json, JsonFault> {
        self.at += 1; // `[`
        self.skip_space();

        let mut items = Vec::new();

        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }

        loop {
            self.skip_space();
            items.push(self.value()?);

            self.skip_space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                Some(_) => return Err(JsonFault::Unexpected),
                None => return Err(JsonFault::Truncated),
            }
        }
    }

    /// A string, with the cursor on its opening quote.
    fn string(&mut self) -> Result<String, JsonFault> {
        if self.peek() != Some(b'"') {
            return Err(JsonFault::Unexpected);
        }
        self.at += 1;

        // Bytes rather than `char`s while there is nothing to decode, and pushed through
        // `String::from_utf8` at the end: the body arrived as bytes and was read as `&str` only
        // because it was valid UTF-8, so the only way this can fail is an escape that spelled
        // something else — which is a fault of the escape, not of the text.
        let mut out: Vec<u8> = Vec::new();

        loop {
            let byte = self.peek().ok_or(JsonFault::Truncated)?;
            self.at += 1;

            match byte {
                b'"' => {
                    return String::from_utf8(out).map_err(|_| JsonFault::Escape);
                }
                b'\\' => {
                    let escape = self.peek().ok_or(JsonFault::Truncated)?;
                    self.at += 1;

                    match escape {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let mut unit = self.hex4()?;

                            // A character above U+FFFF is written as two units, and neither half is
                            // a character on its own: the high one is held back until it is known
                            // whether the low one follows, and a half that never finds its partner
                            // is written as the replacement character rather than as nothing —
                            // a body that was mangled in transit should read as mangled.
                            if (0xD800..0xDC00).contains(&unit) {
                                let low = if self.peek() == Some(b'\\')
                                    && self.bytes.get(self.at + 1) == Some(&b'u')
                                {
                                    self.at += 2;
                                    Some(self.hex4()?)
                                } else {
                                    None
                                };

                                let combined = low
                                    .filter(|low| (0xDC00..0xE000).contains(low))
                                    .map(|low| 0x1_0000 + ((unit - 0xD800) << 10) + (low - 0xDC00));

                                unit = combined.unwrap_or(0xFFFD);
                            }

                            let character = char::from_u32(unit).unwrap_or('\u{FFFD}');
                            let mut encoded = [0u8; 4];
                            out.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                        }
                        _ => return Err(JsonFault::Escape),
                    }
                }
                // A control byte unescaped is not JSON's, and it is the one case a body that is not
                // really JSON can hide in: refusing it is what keeps this from reading somebody
                // else's binary as a string.
                byte if byte < 0x20 => return Err(JsonFault::Escape),
                byte => out.push(byte),
            }
        }
    }

    /// The four hexadecimal digits of a `\u` escape.
    fn hex4(&mut self) -> Result<u32, JsonFault> {
        let mut unit = 0u32;

        for _ in 0..4 {
            let digit = self.peek().ok_or(JsonFault::Truncated)?;
            self.at += 1;

            let value = match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'f' => digit - b'a' + 10,
                b'A'..=b'F' => digit - b'A' + 10,
                _ => return Err(JsonFault::Escape),
            };

            unit = unit * 16 + value as u32;
        }

        Ok(unit)
    }

    /// A number, with the cursor on its first byte.
    ///
    /// The digits JSON allows are collected and handed to `f64::from_str`, which is the same reader
    /// every other Rust program uses for a number. What it does not allow is *not* checked for —
    /// `+1` and `1.` parse here and would not parse there — because the characters that can reach a
    /// float at all are the ones collected below, and a stricter grammar would be a second opinion
    /// about a text this page did not write.
    fn number(&mut self) -> Result<f64, JsonFault> {
        let start = self.at;

        while matches!(
            self.peek(),
            Some(b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
        ) {
            self.at += 1;
        }

        let text = &self.bytes[start..self.at];
        let text = std::str::from_utf8(text).map_err(|_| JsonFault::Unexpected)?;

        // Finite, because the page draws this: `1e999` parses to an infinity, and a card reading
        // `inf°` is worse than a page saying it could not read the answer.
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(value),
            _ => Err(JsonFault::Unexpected),
        }
    }

    /// One of JSON's three bare words.
    fn literal(&mut self, word: &str) -> Result<(), JsonFault> {
        let end = self.at + word.len();

        if self.bytes.get(self.at..end) == Some(word.as_bytes()) {
            self.at = end;
            Ok(())
        } else {
            Err(JsonFault::Unexpected)
        }
    }

    /// Steps over a byte that has to be there.
    fn expect(&mut self, byte: u8) -> Result<(), JsonFault> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else if self.peek().is_none() {
            Err(JsonFault::Truncated)
        } else {
            Err(JsonFault::Unexpected)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kinds of value, read out of a document that has one of each.
    #[test]
    fn every_kind_of_value_comes_back_as_itself() {
        let document = parse(
            r#"{"text":"a","number":-1.5e2,"yes":true,"no":false,"nothing":null,"list":[1,2,3],"nested":{"a":[{"b":1}]}}"#,
        )
        .unwrap();

        assert_eq!(document.get("text").and_then(Json::string), Some("a"));
        assert_eq!(document.get("number").and_then(Json::number), Some(-150.0));
        assert_eq!(document.get("yes"), Some(&Json::Bool(true)));
        assert_eq!(document.get("no"), Some(&Json::Bool(false)));
        assert_eq!(document.get("nothing"), Some(&Json::Null));

        let list = document.get("list").and_then(Json::array).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[2].number(), Some(3.0));

        // The shape the forecast really arrives in: an object holding an object holding arrays.
        let nested = document.get("nested").unwrap();
        let inner = nested.get("a").and_then(Json::array).unwrap();
        assert_eq!(inner[0].get("b").and_then(Json::number), Some(1.0));
    }

    /// Whitespace is between tokens and not inside them, and a document is not its own whitespace.
    #[test]
    fn whitespace_is_stepped_over_and_nothing_else_is() {
        assert_eq!(parse(" \n\t 42 \r\n ").unwrap().number(), Some(42.0));
        assert_eq!(parse("{}").unwrap(), Json::Object(Vec::new()));
        assert_eq!(parse("[]").unwrap(), Json::Array(Vec::new()));
    }

    /// A string's escapes are decoded, including the one that is two units.
    #[test]
    fn escapes_are_decoded() {
        let document = parse(r#"{"s":"a\"b\\c\/d\ne\u0041\ud83c\udf24"}"#).unwrap();
        assert_eq!(
            document.get("s").and_then(Json::string),
            Some("a\"b\\c/d\neA\u{1F324}"),
            "a quote, a backslash, a slash, a newline, U+0041 and U+1F324"
        );

        // A lone half of a pair is not a character, and reads as the replacement character rather
        // than as nothing at all.
        let lone = parse(r#"{"s":"\ud83c"}"#).unwrap();
        assert_eq!(lone.get("s").and_then(Json::string), Some("\u{FFFD}"));
    }

    /// A text that is not a document is a fault, and which fault it is is the reader's business.
    #[test]
    fn what_is_not_a_document_is_refused() {
        assert_eq!(parse(""), Err(JsonFault::Truncated));
        assert_eq!(parse("{\"a\":"), Err(JsonFault::Truncated));
        assert_eq!(parse("[1,2"), Err(JsonFault::Truncated));
        assert_eq!(parse("\"abc"), Err(JsonFault::Truncated));

        assert_eq!(parse("tru"), Err(JsonFault::Unexpected));
        assert_eq!(parse("{1:2}"), Err(JsonFault::Unexpected));
        assert_eq!(parse("{} {}"), Err(JsonFault::Trailing));
        assert_eq!(parse("1 2"), Err(JsonFault::Trailing));
        assert_eq!(parse("nan"), Err(JsonFault::Unexpected));

        assert_eq!(parse(r#""\q""#), Err(JsonFault::Escape));
        assert_eq!(parse("\"a\nb\""), Err(JsonFault::Escape), "a raw newline");
        assert_eq!(parse(r#""\u00zz""#), Err(JsonFault::Escape));
    }

    /// The one number this page will not draw: an infinity.
    #[test]
    fn a_number_that_is_not_finite_is_not_a_reading() {
        assert_eq!(parse("1e999"), Err(JsonFault::Unexpected));
        assert_eq!(parse("-1e999"), Err(JsonFault::Unexpected));
    }

    /// A value of one kind is not a value of another, and the accessors say so rather than convert.
    #[test]
    fn the_accessors_do_not_convert() {
        let document = parse(r#"{"one":1,"text":"1","list":[1]}"#).unwrap();

        assert_eq!(document.get("one").and_then(Json::string), None);
        assert_eq!(document.get("text").and_then(Json::number), None);
        assert_eq!(document.get("text").and_then(Json::array), None);
        assert_eq!(document.get("list").and_then(Json::string), None);
        assert_eq!(document.get("missing"), None);
        assert_eq!(document.get("one").and_then(|json| json.get("a")), None);
    }
}
