use super::Command;
use crate::shell::TerminalModel;

pub struct FreeCommand;

impl Command for FreeCommand {
    fn name(&self) -> &'static str {
        "free"
    }

    fn description(&self) -> &'static str {
        "Show memory and LittleFS storage status"
    }

    fn execute(&self, state: &mut TerminalModel, _args: &[&str]) {
        state.push_output("Memory: Free DRAM ~320KB, Free PSRAM ~16MB");
        state.push_output("Storage: LittleFS mounted at /internal (3MB Partition)");
    }
}
