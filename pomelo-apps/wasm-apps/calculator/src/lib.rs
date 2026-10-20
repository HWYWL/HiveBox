//! The calculator, as a package.
//!
//! Four operations and twenty keys, and the whole of it is a display list: the arithmetic is `i64`,
//! the display is a fixed buffer, and there is no heap to ask for. It is the first pilot that
//! answers whether an interface this simple belongs in the sandbox — see the store's notes.

#![no_std]

use app_sdk::{app, icons, theme, widgets, App, Context, Event, Rect, Ui};

/// 这个包自己的颜色，与 `tools/build_apps.py` 里 calculator 那条目的 `#F69E22` 是同一个。
const ACCENT: app_sdk::ui::Color = 0x00F6_9E22;

/// The four operations, as the one byte the state keeps.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

/// One key: its label and what pressing it means.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Key {
    Digit(u8),
    DoubleZero,
    Clear,
    Backspace,
    Negate,
    Percent,
    Op(Op),
    Equals,
}

/// The keys, in the order the grid draws them: four columns, five rows.
const KEYS: [(Key, &str); 20] = [
    (Key::Clear, "C"),
    (Key::Backspace, "<"),
    (Key::Negate, "+/-"),
    (Key::Op(Op::Div), "/"),
    (Key::Digit(7), "7"),
    (Key::Digit(8), "8"),
    (Key::Digit(9), "9"),
    (Key::Op(Op::Mul), "x"),
    (Key::Digit(4), "4"),
    (Key::Digit(5), "5"),
    (Key::Digit(6), "6"),
    (Key::Op(Op::Sub), "-"),
    (Key::Digit(1), "1"),
    (Key::Digit(2), "2"),
    (Key::Digit(3), "3"),
    (Key::Op(Op::Add), "+"),
    (Key::Digit(0), "0"),
    (Key::DoubleZero, "00"),
    (Key::Percent, "%"),
    (Key::Equals, "="),
];

struct Calculator {
    /// What the display shows: the number being typed, or the last answer.
    entry: [u8; 20],
    entry_len: usize,
    /// The accumulated value and the operation waiting for the next number.
    accumulator: i64,
    pending: Option<Op>,
    /// Whether the next digit starts a new number rather than extending the answer.
    fresh: bool,
    error: bool,
    /// The key the finger is on, so that it can be drawn pressed.
    pressed: Option<usize>,
}

impl Calculator {
    fn value(&self) -> i64 {
        parse(&self.entry[..self.entry_len])
    }

    fn set(&mut self, value: i64) {
        let mut buffer = [0u8; 20];

        {
            use core::fmt::Write;

            let mut buf = app_sdk::text::Buf::new(&mut buffer);
            let _ = write!(&mut buf, "{value}");
            self.entry_len = buf.len();
        }

        self.entry[..self.entry_len].copy_from_slice(&buffer[..self.entry_len]);
        self.fresh = true;
    }

    fn push_digit(&mut self, digit: u8) {
        if self.fresh || self.error {
            self.entry_len = 0;
            self.fresh = false;
            self.error = false;
        }

        if self.entry_len >= self.entry.len() {
            return;
        }

        if self.entry_len == 1 && self.entry[0] == b'0' {
            self.entry_len = 0;
        }

        self.entry[self.entry_len] = b'0' + digit;
        self.entry_len += 1;
    }

    fn apply(&mut self, op: Op) {
        let value = self.value();

        self.accumulator = match self.pending {
            Some(pending) => evaluate(self.accumulator, pending, value),
            None => value,
        };

        self.pending = Some(op);
        self.set(self.accumulator);
    }

    fn equals(&mut self) {
        let value = self.value();

        if let Some(pending) = self.pending.take() {
            self.accumulator = evaluate(self.accumulator, pending, value);
            self.set(self.accumulator);
        }
    }

    fn press(&mut self, key: Key) {
        match key {
            Key::Digit(digit) => self.push_digit(digit),
            Key::DoubleZero => {
                self.push_digit(0);
                self.push_digit(0);
            }
            Key::Op(op) => self.apply(op),
            Key::Equals => self.equals(),
            Key::Percent => {
                let value = self.value() / 100;
                self.set(value);
            }
            Key::Clear => {
                self.entry_len = 1;
                self.entry[0] = b'0';
                self.accumulator = 0;
                self.pending = None;
                self.fresh = true;
                self.error = false;
            }
            Key::Backspace => {
                if self.entry_len > 1 {
                    self.entry_len -= 1;
                } else {
                    self.entry_len = 1;
                    self.entry[0] = b'0';
                }
            }
            Key::Negate => {
                let value = -self.value();
                self.set(value);
            }
        }
    }
}

fn evaluate(left: i64, op: Op, right: i64) -> i64 {
    match op {
        Op::Add => left.saturating_add(right),
        Op::Sub => left.saturating_sub(right),
        Op::Mul => left.saturating_mul(right),
        Op::Div => {
            if right == 0 {
                return 0;
            }

            left / right
        }
    }
}

fn parse(bytes: &[u8]) -> i64 {
    let mut value: i64 = 0;
    let mut negative = false;
    let mut at = 0;

    if bytes.first() == Some(&b'-') {
        negative = true;
        at = 1;
    }

    while at < bytes.len() {
        let digit = bytes[at].wrapping_sub(b'0');

        if digit > 9 {
            break;
        }

        value = value.saturating_mul(10).saturating_add(digit as i64);
        at += 1;
    }

    if negative {
        -value
    } else {
        value
    }
}

/// The key grid: four columns between the page margins.
fn key_rect(index: usize) -> Rect {
    const MARGIN: f32 = 18.0;
    const GAP: f32 = 8.0;
    const TOP: f32 = 214.0;
    const BOTTOM: f32 = 470.0;

    let column = (index % 4) as f32;
    let row = (index / 4) as f32;
    let width = (480.0 - MARGIN * 2.0 - GAP * 3.0) / 4.0;
    let height = (BOTTOM - TOP - GAP * 4.0) / 5.0;

    Rect::new(
        MARGIN + column * (width + GAP),
        TOP + row * (height + GAP),
        width,
        height,
    )
}

impl App for Calculator {
    fn new(_: &Context) -> Self {
        let mut calculator = Self {
            entry: [0; 20],
            entry_len: 1,
            accumulator: 0,
            pending: None,
            fresh: true,
            error: false,
            pressed: None,
        };

        calculator.entry[0] = b'0';

        calculator
    }

    fn update(&mut self, _event: Event, context: &mut Context) {
        self.pressed = (0..KEYS.len()).find(|index| context.input.pressing(key_rect(*index)));

        for (index, (key, _)) in KEYS.iter().enumerate() {
            if context.tapped(key_rect(index)) {
                self.press(*key);
            }
        }
    }

    fn view(&mut self, ui: &mut Ui) {
        ui.clear(theme::BLACK);
        // 页头：这一页的图标与名字，跟别的应用是同一个开头。
        widgets::page_header(
            ui,
            Rect::new(18.0, 18.0, 420.0, 44.0),
            icons::GRID_VIEW,
            ACCENT,
            "计算器",
            "四则运算",
        );
        ui.rounded_rect(Rect::new(18.0, 66.0, 444.0, 124.0), 20.0, theme::SURFACE);

        let shown = if self.error {
            "错误"
        } else if self.entry_len == 0 {
            "0"
        } else {
            core::str::from_utf8(&self.entry[..self.entry_len]).unwrap_or("0")
        };

        ui.text_in(
            Rect::new(34.0, 96.0, 412.0, 64.0),
            shown,
            52.0,
            theme::TEXT,
            app_sdk::app_abi::Align::Right,
        );

        for (index, (key, label)) in KEYS.iter().enumerate() {
            let rect = key_rect(index);

            let fill = match key {
                Key::Equals => theme::ACCENT,
                Key::Op(_) => theme::SURFACE_HIGH,
                _ => theme::SURFACE,
            };

            widgets::button(
                ui,
                rect,
                label,
                26.0,
                fill,
                theme::TEXT,
                self.pressed == Some(index),
            );
        }
    }
}

app!(Calculator);
