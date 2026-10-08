pub mod cat;
pub mod cd;
pub mod clear;
pub mod echo;
pub mod free;
pub mod help;
pub mod ls;
pub mod pwd;
pub mod rm;
pub mod wifi;

use crate::shell::TerminalModel;

pub trait Command: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn execute(&self, state: &mut TerminalModel, args: &[&str]);
}

pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut reg = Self {
            commands: Vec::new(),
        };
        reg.register(help::HelpCommand);
        reg.register(ls::LsCommand);
        reg.register(cd::CdCommand);
        reg.register(pwd::PwdCommand);
        reg.register(cat::CatCommand);
        reg.register(rm::RmCommand);
        reg.register(free::FreeCommand);
        reg.register(echo::EchoCommand);
        reg.register(clear::ClearCommand);
        reg.register(wifi::WifiCommand);
        reg
    }

    pub fn register(&mut self, cmd: impl Command + 'static) {
        self.commands.push(Box::new(cmd));
    }

    pub fn list(&self) -> &[Box<dyn Command>] {
        &self.commands
    }

    pub fn dispatch(&self, state: &mut TerminalModel, input_line: &str) {
        let parts: Vec<&str> = input_line.split_whitespace().collect();
        if parts.is_empty() {
            return;
        }

        let cmd_name = parts[0];
        let args = &parts[1..];

        for cmd in &self.commands {
            if cmd.name() == cmd_name {
                cmd.execute(state, args);
                return;
            }
        }

        state.push_output(&format!(
            "{}: command not found. Type 'help' for available commands.",
            cmd_name
        ));
    }
}

// Global shared registry instance
lazy_static_or_thread_safe_registry!();

macro_rules! lazy_static_or_thread_safe_registry {
    () => {
        pub fn get_registry() -> &'static CommandRegistry {
            static REGISTRY: std::sync::OnceLock<CommandRegistry> = std::sync::OnceLock::new();
            REGISTRY.get_or_init(CommandRegistry::new)
        }
    };
}
use lazy_static_or_thread_safe_registry;
