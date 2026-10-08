use super::Command;
use crate::shell::TerminalModel;

pub struct PwdCommand;

impl Command for PwdCommand {
    fn name(&self) -> &'static str {
        "pwd"
    }

    fn description(&self) -> &'static str {
        "Print current working directory"
    }

    fn execute(&self, state: &mut TerminalModel, _args: &[&str]) {
        let cwd = state.cwd.clone();
        state.push_output(&cwd);
    }
}
