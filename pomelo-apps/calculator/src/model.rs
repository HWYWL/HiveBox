//! The calculator's state machine.
//!
//! All the arithmetic rules live here — no UI, no widgets, no `Rc`. The UI reads
//! [`CalcModel::primary_display`] / [`CalcModel::secondary_display`] and calls the
//! `input_*` / `calculate` / `clear` methods; a test can drive the same methods
//! without building a single widget.

use crate::format::{add_commas, eval_op, format_raw_number};

/// What the user has typed so far, plus the pending operation.
#[derive(Debug, Clone)]
pub struct CalcModel {
    /// Digits currently being typed (or the last result).
    pub current_input: String,
    /// Left-hand operand of the pending operation, once an operator was pressed.
    pub first_operand: Option<f64>,
    /// The left-hand operand as typed (kept so the history line can show it).
    pub first_operand_str: String,
    /// The pending operator, if any.
    pub operator: Option<char>,
    /// The "a ÷ b" history line shown above the result.
    pub secondary: String,
    /// `true` while showing a finished result: the next digit starts a new number.
    pub just_calculated: bool,
}

impl Default for CalcModel {
    fn default() -> Self {
        Self::new()
    }
}

impl CalcModel {
    pub fn new() -> Self {
        Self {
            current_input: "0".to_string(),
            first_operand: None,
            first_operand_str: String::new(),
            operator: None,
            secondary: String::new(),
            just_calculated: false,
        }
    }

    pub fn input_digit(&mut self, digit: char) {
        if self.just_calculated {
            self.current_input = digit.to_string();
            self.just_calculated = false;
            self.secondary.clear();
        } else if self.current_input == "0" {
            self.current_input = digit.to_string();
        } else if self.current_input.len() < 12 {
            self.current_input.push(digit);
        }
    }

    pub fn input_dot(&mut self) {
        if self.just_calculated {
            self.current_input = "0.".to_string();
            self.just_calculated = false;
            self.secondary.clear();
        } else if self.current_input.is_empty() {
            self.current_input = "0.".to_string();
        } else if !self.current_input.contains('.') {
            self.current_input.push('.');
        }
    }

    pub fn input_operator(&mut self, op: char) {
        let current_val: f64 = self.current_input.parse().unwrap_or(0.0);

        if let (Some(first), Some(prev_op)) = (self.first_operand, self.operator) {
            if !self.current_input.is_empty() && !self.just_calculated {
                let res = eval_op(first, prev_op, current_val);
                self.secondary = format!(
                    "{}\u{200A}{}\u{200A}{}",
                    add_commas(&self.first_operand_str),
                    prev_op,
                    add_commas(&self.current_input)
                );
                self.first_operand = Some(res);
                self.first_operand_str = format_raw_number(res);
            }
        } else {
            self.first_operand = Some(current_val);
            self.first_operand_str = if self.current_input.is_empty() {
                "0".to_string()
            } else {
                self.current_input.clone()
            };
        }

        self.operator = Some(op);
        self.current_input.clear();
        self.just_calculated = false;
    }

    pub fn calculate(&mut self) {
        if let (Some(first), Some(op)) = (self.first_operand, self.operator) {
            let second_val: f64 = if self.current_input.is_empty() {
                first
            } else {
                self.current_input.parse().unwrap_or(0.0)
            };
            let second_str = if self.current_input.is_empty() {
                self.first_operand_str.clone()
            } else {
                self.current_input.clone()
            };

            let res = eval_op(first, op, second_val);
            self.secondary = format!(
                "{}\u{200A}{}\u{200A}{}",
                add_commas(&self.first_operand_str),
                op,
                add_commas(&second_str)
            );
            self.current_input = format_raw_number(res);
            self.first_operand = None;
            self.first_operand_str.clear();
            self.operator = None;
            self.just_calculated = true;
        }
    }

    pub fn clear(&mut self) {
        self.current_input = "0".to_string();
        self.first_operand = None;
        self.first_operand_str.clear();
        self.operator = None;
        self.secondary.clear();
        self.just_calculated = false;
    }

    pub fn toggle_sign(&mut self) {
        if self.current_input != "0" && !self.current_input.is_empty() {
            if self.current_input.starts_with('-') {
                self.current_input.remove(0);
            } else {
                self.current_input.insert(0, '-');
            }
        }
    }

    pub fn percent(&mut self) {
        let val: f64 = self.current_input.parse().unwrap_or(0.0);
        let res = val / 100.0;
        self.current_input = format_raw_number(res);
    }

    /// The big line: the number being typed, or `a ÷ b` while an operation is
    /// pending.
    pub fn primary_display(&self) -> String {
        if self.just_calculated {
            add_commas(&self.current_input)
        } else if let Some(op) = self.operator {
            if self.current_input.is_empty() {
                format!("{}\u{200A}{}", add_commas(&self.first_operand_str), op)
            } else {
                format!(
                    "{}\u{200A}{}\u{200A}{}",
                    add_commas(&self.first_operand_str),
                    op,
                    add_commas(&self.current_input)
                )
            }
        } else {
            add_commas(&self.current_input)
        }
    }

    /// The small history line. A single space keeps the card from changing height
    /// when there is nothing to show.
    pub fn secondary_display(&self) -> &str {
        if self.secondary.is_empty() {
            " "
        } else {
            &self.secondary
        }
    }
}
