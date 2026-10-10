//! The three bits of arithmetic a list like this is made of, kept out of the widgets.
//!
//! Nothing here reads a board or draws anything: every function takes a number and returns a string,
//! which is what makes them testable without a panel — and what makes the one thing that must not
//! drift (how a stack is spelled) defined in exactly one place.

use std::time::Duration;

use pomelo_hal::TaskState;
use pomelo_widgets::preferences::Language;

/// What a task is doing, in one character.
///
/// One character because a column of states is read *across*: 运 and 阻 line up down the page the way
/// `R` and `S` do, where 运行中 and 阻塞 would make one column as wide as a sentence.
///
/// Which character it is depends on the language the system is set to, and the two sets name the same
/// four states: 运 / 阻 / 挂 / 退 are 运行, 阻塞, 挂起 and 退出, and `R` / `S` / `T` / `Z` are the letters
/// `htop` prints for exactly those four, in the same order. A reader of either language gets a column
/// they can run their eye down; a page that fixed on one of the two would be a page half in the other.
///
/// The mapping worth keeping in mind, in either language: a task waiting on a delay or a queue is 阻 —
/// which is what a `ps` prints as `S`, and what most of a settled board's rows are.
pub fn state_char(state: TaskState, language: Language) -> char {
    match (state, language) {
        (TaskState::Running, Language::Chinese) => '运',
        (TaskState::Blocked, Language::Chinese) => '阻',
        (TaskState::Suspended, Language::Chinese) => '挂',
        (TaskState::Deleted, Language::Chinese) => '退',
        (TaskState::Running, Language::English) => 'R',
        (TaskState::Blocked, Language::English) => 'S',
        (TaskState::Suspended, Language::English) => 'T',
        (TaskState::Deleted, Language::English) => 'Z',
    }
}

/// A stack reading as the shortest thing that still says it.
///
/// Bytes below a kilobyte, because that is the range that matters — a task with 200 bytes left is a
/// task about to fault, and `0.2K` would round that away — and kilobytes above it to one decimal,
/// because a list of six-digit numbers is a list nobody reads across.
pub fn stack_of(bytes: u32) -> String {
    if bytes < 1024 {
        format!("{bytes}")
    } else {
        format!("{:.1}K", f64::from(bytes) / 1024.0)
    }
}

/// How long the board has been up, as `HH:MM:SS`.
///
/// Hours are not wrapped: a board up for thirty hours says so, because "06:12:34" after a day and a
/// half would be a number that means something else. Days are not spelled out either — this is the
/// uptime of something that gets flashed, and an uptime in days would be a debugger's business.
pub fn clock_of(uptime: Duration) -> String {
    let seconds = uptime.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);

    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every state has one character in each language, and no two states share one.
    ///
    /// The sharing check is the one that matters: a column of states that spelled two of them the same
    /// way would be a column that could not be read at all, and nothing else on the page would say so —
    /// and it is checked per language, because two sets that are each right can still be one set with a
    /// hole in it.
    #[test]
    fn every_state_has_its_own_character_in_each_language() {
        let states = [
            TaskState::Running,
            TaskState::Blocked,
            TaskState::Suspended,
            TaskState::Deleted,
        ];

        for language in [Language::Chinese, Language::English] {
            let mut seen: Vec<char> = states
                .iter()
                .copied()
                .map(|state| state_char(state, language))
                .collect();
            seen.sort_unstable();
            seen.dedup();

            assert_eq!(
                seen.len(),
                states.len(),
                "{language:?}: four states, four characters"
            );
        }

        // And the two sets are the ones a reader of each language expects: the names of the states in
        // Chinese, and the letters `htop` prints for the same four.
        assert_eq!(state_char(TaskState::Running, Language::Chinese), '运');
        assert_eq!(state_char(TaskState::Blocked, Language::Chinese), '阻');
        assert_eq!(state_char(TaskState::Suspended, Language::Chinese), '挂');
        assert_eq!(state_char(TaskState::Deleted, Language::Chinese), '退');

        assert_eq!(state_char(TaskState::Running, Language::English), 'R');
        assert_eq!(state_char(TaskState::Blocked, Language::English), 'S');
        assert_eq!(state_char(TaskState::Suspended, Language::English), 'T');
        assert_eq!(state_char(TaskState::Deleted, Language::English), 'Z');
    }

    /// A stack is spelled so that the interesting end of the range stays exact.
    ///
    /// The two cases that matter: a task almost out of stack keeps its bytes (`200` and not `0.2K`),
    /// and a healthy one is shortened so the column stays a column.
    #[test]
    fn a_stack_is_big_or_small_but_always_readable() {
        assert_eq!(stack_of(0), "0");
        assert_eq!(stack_of(200), "200");
        assert_eq!(stack_of(1023), "1023");
        assert_eq!(stack_of(1024), "1.0K");
        assert_eq!(stack_of(2048), "2.0K");
        assert_eq!(stack_of(16 * 1024 + 512), "16.5K");
    }

    /// An uptime keeps its hours and pads everything else, so a column of them lines up.
    #[test]
    fn an_uptime_is_hours_minutes_and_seconds() {
        assert_eq!(clock_of(Duration::from_secs(0)), "00:00:00");
        assert_eq!(clock_of(Duration::from_secs(9)), "00:00:09");
        assert_eq!(clock_of(Duration::from_secs(754)), "00:12:34");
        assert_eq!(clock_of(Duration::from_secs(3600)), "01:00:00");
        assert_eq!(
            clock_of(Duration::from_secs(30 * 3600 + 61)),
            "30:01:01",
            "a board that has been up for a day says so rather than wrapping"
        );
    }
}
