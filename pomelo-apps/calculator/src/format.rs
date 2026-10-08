//! Pure number and display helpers.
//!
//! Nothing here touches UI or state: every function is a total function of its
//! arguments. Keeping them separate is what makes the state machine in
//! [`crate::state`] easy to read, and makes these easy to unit-test.

/// Applies an operator to two operands.
///
/// Division by zero yields `0.0` instead of an infinity, so the display never has
/// to render a non-number.
///
/// `op` is the character the keypad shows: `×` and `÷`, not `*` and `/`. The two used to differ —
/// the keypad sent ASCII and this match arms were the display characters — and the fallback arm
/// below turned every multiplication and division into "the second operand", silently. That is
/// what an unmatched operator still must not do, so the fallback is now a `NaN`, which
/// [`format_raw_number`] renders as `Error`.
pub fn eval_op(first: f64, op: char, second: f64) -> f64 {
    match op {
        '+' | '＋' => first + second,
        '-' | '－' => first - second,
        '×' => first * second,
        '÷' => {
            if second == 0.0 {
                0.0
            } else {
                first / second
            }
        }
        _ => f64::NAN,
    }
}

/// Formats a raw result for the display: integers without a decimal point,
/// everything else with up to six decimals and no trailing zeros.
pub fn format_raw_number(n: f64) -> String {
    if n.is_nan() || n.is_infinite() {
        return "Error".to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e12 {
        format!("{:.0}", n)
    } else {
        let raw = format!("{:.6}", n);
        raw.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Inserts thousands separators into the integer part of `s`.
pub fn add_commas(s: &str) -> String {
    if s.is_empty() || s == "Error" {
        return s.to_string();
    }
    let is_negative = s.starts_with('-');
    let raw = if is_negative { &s[1..] } else { s };

    let parts: Vec<&str> = raw.split('.').collect();
    let int_part = parts[0];
    let frac_part = if parts.len() > 1 {
        Some(parts[1])
    } else {
        None
    };

    let mut formatted_int = String::with_capacity(int_part.len() + int_part.len() / 3);
    let len = int_part.len();
    for (i, ch) in int_part.chars().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            formatted_int.push(',');
        }
        formatted_int.push(ch);
    }

    let mut res = String::new();
    if is_negative {
        res.push('-');
    }
    res.push_str(&formatted_int);
    if let Some(frac) = frac_part {
        res.push('.');
        res.push_str(frac);
    }
    res
}
