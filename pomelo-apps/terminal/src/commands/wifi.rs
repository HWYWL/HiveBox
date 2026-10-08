use super::Command;
use crate::shell::TerminalModel;

pub struct WifiCommand;

impl Command for WifiCommand {
    fn name(&self) -> &'static str {
        "wifi"
    }

    fn description(&self) -> &'static str {
        "Manage Wi-Fi network (wifi scan, wifi status, wifi connect <ssid> <pass>)"
    }

    fn execute(&self, state: &mut TerminalModel, args: &[&str]) {
        let sub = args.first().copied().unwrap_or("status");
        match sub {
            "status" => {
                state.push_output("Wi-Fi Status: Connected");
                state.push_output("  SSID: ESP-Rust-Home  IP: 192.168.1.105  RSSI: -52dBm");
            }
            "scan" => {
                state.push_output("Scanning 2.4GHz Wi-Fi channels...");
                state.push_output("  1. [ESP-Rust-Home]    -52dBm (WPA2)");
                state.push_output("  2. [Office-5G-Guest]  -68dBm (WPA2)");
                state.push_output("  3. [Xiaomi-Router]    -75dBm (WPA3)");
            }
            "connect" => {
                if let Some(ssid) = args.get(1) {
                    state.push_output(&format!("Connecting to SSID '{}'...", ssid));
                    state.push_output("Connected! Obtained IP: 192.168.1.105");
                } else {
                    state.push_output("Usage: wifi connect <SSID> [password]");
                }
            }
            _ => {
                state.push_output(&format!(
                    "Unknown wifi subcommand '{}'. Usage: wifi [scan | status | connect <ssid> [pass]]",
                    sub
                ));
            }
        }
    }
}
