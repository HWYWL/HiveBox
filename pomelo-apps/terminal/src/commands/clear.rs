use super::Command;
use crate::shell::TerminalModel;

pub struct ClearCommand;

impl Command for ClearCommand {
    fn name(&self) -> &'static str {
        "clear"
    }

    fn description(&self) -> &'static str {
        "Clear terminal screen output"
    }

    fn execute(&self, state: &mut TerminalModel, _args: &[&str]) {
        state.history.clear();
    }
}
