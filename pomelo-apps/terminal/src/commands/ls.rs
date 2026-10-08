use super::Command;
use crate::shell::{resolve_path, TerminalModel};

pub struct LsCommand;

impl Command for LsCommand {
    fn name(&self) -> &'static str {
        "ls"
    }

    fn description(&self) -> &'static str {
        "List directory contents with file size and type"
    }

    fn execute(&self, state: &mut TerminalModel, args: &[&str]) {
        let target_raw = args.first().copied().unwrap_or("");
        let target_dir = resolve_path(&state.cwd, target_raw);

        match std::fs::read_dir(&target_dir) {
            Ok(read_dir) => {
                let mut entries = Vec::new();
                for entry in read_dir.flatten() {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    let full_path = std::path::Path::new(&target_dir).join(&fname);
                    if let Ok(meta) = std::fs::metadata(&full_path) {
                        if meta.is_dir() {
                            entries.push(format!("{}/", fname));
                        } else {
                            let size = meta.len();
                            let formatted_size = if size < 1024 {
                                format!("{}B", size)
                            } else if size < 1024 * 1024 {
                                format!("{:.1}KB", size as f64 / 1024.0)
                            } else {
                                format!("{:.1}MB", size as f64 / (1024.0 * 1024.0))
                            };
                            entries.push(format!("{} ({})", fname, formatted_size));
                        }
                    } else {
                        entries.push(fname);
                    }
                }
                if entries.is_empty() {
                    state.push_output("(empty directory)");
                } else {
                    state.push_output(&entries.join("  "));
                }
            }
            Err(e) => {
                state.push_output(&format!(
                    "ls: cannot open directory '{}': {}",
                    target_dir, e
                ));
            }
        }
    }
}
