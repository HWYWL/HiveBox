use super::Command;
use crate::shell::{resolve_path, TerminalModel};

pub struct CatCommand;

impl Command for CatCommand {
    fn name(&self) -> &'static str {
        "cat"
    }

    fn description(&self) -> &'static str {
        "Concatenate and display file content"
    }

    fn execute(&self, state: &mut TerminalModel, args: &[&str]) {
        if let Some(filename) = args.first() {
            let path_str = resolve_path(&state.cwd, filename);
            match std::fs::read_to_string(&path_str) {
                Ok(content) => {
                    for line in content.lines().take(12) {
                        state.push_output(line);
                    }
                }
                Err(e) => {
                    state.push_output(&format!("cat: {}: {}", filename, e));
                }
            }
        } else {
            state.push_output("cat: missing file operand");
        }
    }
}
