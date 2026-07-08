#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    embed_log_tauri::run();
}
