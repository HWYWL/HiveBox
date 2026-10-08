//! The key table.
//!
//! A key is a label, a role, and what it does to the model — the same split the original
//! calculator made, kept out of the widget assembly so the table can be read as a table.

use crate::model::CalcModel;

/// Which palette a key takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Function,
    Number,
    Operator,
    Equals,
}

/// What pressing a key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Digit(char),
    Dot,
    Operator(char),
    Equals,
    Clear,
    Sign,
    Percent,
}

impl Key {
    /// Applies the key to the model. The one place that knows the model's API.
    pub fn apply(self, model: &mut CalcModel) {
        match self {
            Key::Digit(digit) => model.input_digit(digit),
            Key::Dot => model.input_dot(),
            Key::Operator(operator) => model.input_operator(operator),
            Key::Equals => model.calculate(),
            Key::Clear => model.clear(),
            Key::Sign => model.toggle_sign(),
            Key::Percent => model.percent(),
        }
    }
}

/// One key of the keypad.
pub struct Entry {
    /// What is drawn on it.
    pub label: &'static str,
    /// The palette it uses.
    pub kind: Kind,
    /// What it does.
    pub key: Key,
    /// How many columns it spans.
    pub span: u16,
}

/// The keypad, row by row, in the order it is drawn.
pub const LAYOUT: &[&[Entry]] = &[
    &[
        Entry {
            label: "C",
            kind: Kind::Function,
            key: Key::Clear,
            span: 1,
        },
        Entry {
            label: "+/-",
            kind: Kind::Function,
            key: Key::Sign,
            span: 1,
        },
        Entry {
            label: "%",
            kind: Kind::Function,
            key: Key::Percent,
            span: 1,
        },
        Entry {
            label: "÷",
            kind: Kind::Operator,
            key: Key::Operator('÷'),
            span: 1,
        },
    ],
    &[
        Entry {
            label: "7",
            kind: Kind::Number,
            key: Key::Digit('7'),
            span: 1,
        },
        Entry {
            label: "8",
            kind: Kind::Number,
            key: Key::Digit('8'),
            span: 1,
        },
        Entry {
            label: "9",
            kind: Kind::Number,
            key: Key::Digit('9'),
            span: 1,
        },
        Entry {
            label: "×",
            kind: Kind::Operator,
            key: Key::Operator('×'),
            span: 1,
        },
    ],
    &[
        Entry {
            label: "4",
            kind: Kind::Number,
            key: Key::Digit('4'),
            span: 1,
        },
        Entry {
            label: "5",
            kind: Kind::Number,
            key: Key::Digit('5'),
            span: 1,
        },
        Entry {
            label: "6",
            kind: Kind::Number,
            key: Key::Digit('6'),
            span: 1,
        },
        Entry {
            label: "－",
            kind: Kind::Operator,
            key: Key::Operator('－'),
            span: 1,
        },
    ],
    &[
        Entry {
            label: "1",
            kind: Kind::Number,
            key: Key::Digit('1'),
            span: 1,
        },
        Entry {
            label: "2",
            kind: Kind::Number,
            key: Key::Digit('2'),
            span: 1,
        },
        Entry {
            label: "3",
            kind: Kind::Number,
            key: Key::Digit('3'),
            span: 1,
        },
        Entry {
            label: "＋",
            kind: Kind::Operator,
            key: Key::Operator('＋'),
            span: 1,
        },
    ],
    &[
        Entry {
            label: "0",
            kind: Kind::Number,
            key: Key::Digit('0'),
            span: 2,
        },
        Entry {
            label: ".",
            kind: Kind::Number,
            key: Key::Dot,
            span: 1,
        },
        Entry {
            label: "=",
            kind: Kind::Equals,
            key: Key::Equals,
            span: 1,
        },
    ],
];
