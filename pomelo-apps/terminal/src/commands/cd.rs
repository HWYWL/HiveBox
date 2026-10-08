use super::Command;
use crate::shell::{resolve_path, TerminalModel};

pub struct CdCommand;

impl Command for CdCommand {
    fn name(&self) -> &'static str {
        "cd"
    }

    fn description(&self) -> &'static str {
        "Change current working directory"
    }

    fn execute(&self, state: &mut TerminalModel, args: &[&str]) {
        let target_raw = args.first().copied().unwrap_or("~");
        let candidate_path = resolve_path(&state.cwd, target_raw);
        let path = std::path::Path::new(&candidate_path);

        if path.is_dir() {
            state.cwd = candidate_path;
        } else {
            state.push_output(&format!("cd: no such file or directory: {}", target_raw));
        }
    }
}
