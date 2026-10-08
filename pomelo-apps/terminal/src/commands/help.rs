use super::Command;
use crate::shell::TerminalModel;

pub struct HelpCommand;

impl Command for HelpCommand {
    fn name(&self) -> &'static str {
        "help"
    }

    fn description(&self) -> &'static str {
        "List all available shell commands"
    }

    fn execute(&self, state: &mut TerminalModel, _args: &[&str]) {
        state.push_output("Pomelo UI Shell - Builtin Commands:");
        let registry = super::get_registry();
        for cmd in registry.list() {
            state.push_output(&format!("  {: <6} - {}", cmd.name(), cmd.description()));
        }
    }
}
