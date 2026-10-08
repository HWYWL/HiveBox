use super::Command;
use crate::shell::TerminalModel;

pub struct EchoCommand;

impl Command for EchoCommand {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "Display a line of text"
    }

    fn execute(&self, state: &mut TerminalModel, args: &[&str]) {
        let msg = args.join(" ");
        state.push_output(&msg);
    }
}
