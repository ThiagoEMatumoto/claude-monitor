#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod claude;
mod commands;

use commands::AppState;
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_sql::{Migration, MigrationKind};

fn main() {
    env_logger::init();

    let migrations = vec![
        Migration {
            version: 1,
            description: "create_snapshots_table",
            sql: "CREATE TABLE IF NOT EXISTS snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL DEFAULT (datetime('now')),
                five_hour REAL NOT NULL,
                seven_day REAL NOT NULL,
                sonnet REAL,
                opus REAL
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_timestamp ON snapshots(timestamp);",
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "create_config_table",
            sql: "CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT OR IGNORE INTO config (key, value) VALUES ('alert_threshold_warning', '75');
            INSERT OR IGNORE INTO config (key, value) VALUES ('alert_threshold_critical', '90');
            INSERT OR IGNORE INTO config (key, value) VALUES ('poll_interval_secs', '300');",
            kind: MigrationKind::Up,
        },
    ];

    let initial_creds = claude::load_credentials();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:claude-monitor.db", migrations)
                .build(),
        )
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            credentials: Mutex::new(initial_creds),
        })
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Show Monitor", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .title("Claude Monitor")
                .tooltip("Claude Monitor")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("panel") {
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                            } else {
                                // Position at top-right, below GNOME panel (~32px)
                                if let Ok(Some(monitor)) = win.current_monitor() {
                                    let screen = monitor.size();
                                    let pos = monitor.position();
                                    let size = win.outer_size().unwrap_or(tauri::PhysicalSize::new(340, 520));
                                    let scale = monitor.scale_factor();
                                    let x = pos.x + (screen.width as i32) - (size.width as i32) - (8.0 * scale) as i32;
                                    let y = pos.y + (32.0 * scale) as i32;
                                    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                                }
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_usage,
            commands::login,
            commands::is_authenticated,
            commands::logout,
        ])
        .run(tauri::generate_context!())
        .expect("error running claude-monitor");
}
