use super::Command;
use crate::shell::{resolve_path, TerminalModel};

pub struct RmCommand;

impl Command for RmCommand {
    fn name(&self) -> &'static str {
        "rm"
    }

    fn description(&self) -> &'static str {
        "Remove a file from storage"
    }

    fn execute(&self, state: &mut TerminalModel, args: &[&str]) {
        if let Some(filename) = args.first() {
            let path_str = resolve_path(&state.cwd, filename);
            match std::fs::remove_file(&path_str) {
                Ok(_) => {
                    state.push_output(&format!("removed '{}'", filename));
                }
                Err(e) => {
                    state.push_output(&format!("rm: {}: {}", filename, e));
                }
            }
        } else {
            state.push_output("rm: missing file operand");
        }
    }
}
