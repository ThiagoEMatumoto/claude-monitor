#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analytics;
mod claude;
mod commands;
mod config;
mod sessions;
mod tray_icon;

use commands::AppState;
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tauri_plugin_sql::{Migration, MigrationKind};

use std::fs;
use std::io::Write;

/// Acquire a lockfile to prevent duplicate instances.
/// Returns the File handle (lock held while alive) or exits if another instance is running.
fn acquire_singleton_lock() -> fs::File {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let lock_path = std::path::PathBuf::from(home).join(".config/claude-monitor/singleton.lock");
    let _ = fs::create_dir_all(lock_path.parent().unwrap());

    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&lock_path)
        .expect("failed to open lockfile");

    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        eprintln!("claude-monitor: another instance is already running, exiting.");
        std::process::exit(0);
    }

    // Write PID for debugging
    let mut f = file.try_clone().expect("clone lockfile");
    let _ = write!(f, "{}", std::process::id());

    file
}

fn main() {
    env_logger::init();
    let _lock = acquire_singleton_lock();

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
        Migration {
            version: 3,
            description: "add_extra_usage_columns",
            sql: "ALTER TABLE snapshots ADD COLUMN extra_usage_pct REAL;
                  ALTER TABLE snapshots ADD COLUMN used_credits REAL;",
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
            tray_menu: Mutex::new(None),
        })
        .manage(sessions::SessionsState::new())
        .setup(|app| {
            let sessions_display = MenuItem::with_id(app, "sessions_display", "No sessions waiting", false, None::<&str>)?;
            let sessions_sep = PredefinedMenuItem::separator(app)?;
            let session_display = MenuItem::with_id(app, "session_display", "🟢 Session: --%", false, None::<&str>)?;
            let session_reset = MenuItem::with_id(app, "session_reset", "    resets in --", false, None::<&str>)?;
            let weekly_display = MenuItem::with_id(app, "weekly_display", "🟢 Weekly: --%", false, None::<&str>)?;
            let weekly_reset = MenuItem::with_id(app, "weekly_reset", "    resets in --", false, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sonnet_display = MenuItem::with_id(app, "sonnet_display", "    Sonnet: --%", false, None::<&str>)?;
            let opus_display = MenuItem::with_id(app, "opus_display", "    Opus: --%", false, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let show_details = MenuItem::with_id(app, "show_details", "Show Details…", true, None::<&str>)?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[
                &sessions_display, &sessions_sep,
                &session_display, &session_reset,
                &weekly_display, &weekly_reset,
                &sep1,
                &sonnet_display, &opus_display,
                &sep2, &show_details,
                &sep3, &quit,
            ])?;

            // Store menu reference for dynamic updates
            let state = app.state::<AppState>();
            if let Ok(mut guard) = state.tray_menu.lock() {
                *guard = Some(menu.clone());
            }

            let icon_bytes = include_bytes!("../icons/32x32.png");
            let icon_img = tauri::image::Image::from_bytes(icon_bytes)
                .expect("failed to load tray icon");

            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon_img)
                .title("Claude Monitor")
                .tooltip("Claude Monitor")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "show_details" => {
                        if let Some(win) = app.get_webview_window("panel") {
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                            } else {
                                // Fixed top-right positioning (cursor_position unreliable on GNOME/Wayland)
                                if let Ok(Some(monitor)) = win.current_monitor() {
                                    let screen = monitor.size();
                                    let mon_pos = monitor.position();
                                    let scale = monitor.scale_factor();
                                    let x = mon_pos.x + screen.width as i32 - 340 - (8.0 * scale) as i32;
                                    let y = mon_pos.y + (32.0 * scale) as i32;
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

            // Force GNOME to read fresh icon by bumping temp filename counter (0→1)
            let refresh_icon = tauri::image::Image::from_bytes(icon_bytes)
                .expect("failed to load tray icon");
            tray.set_icon(Some(refresh_icon))?;

            // Start session watcher
            sessions::start_session_watcher(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_usage,
            commands::login,
            commands::is_authenticated,
            commands::logout,
            commands::update_tray_menu,
            commands::update_tray_icon,
            commands::get_waiting_sessions,
            commands::update_tray_sessions,
            commands::play_sound,
            commands::enable_turbo_mode,
            commands::disable_turbo_mode,
            commands::is_turbo_enabled,
            commands::resume_session,
            commands::get_recent_sessions,
            commands::get_session_analytics,
            commands::get_cache_stats,
            commands::get_tool_stats,
        ])
        .run(tauri::generate_context!())
        .expect("error running claude-monitor");
}
