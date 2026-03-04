#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod claude;
mod commands;

fn main() {
    // Minimal main for compilation check — will be fully wired in Task 4
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error running claude-monitor");
}
