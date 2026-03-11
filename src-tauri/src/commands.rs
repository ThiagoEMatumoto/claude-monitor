use crate::analytics;
use crate::claude;
use crate::cost;
use crate::sessions::{self, RecentSession, SessionsState, WaitingSession};
use log::{info, warn};
use serde::Serialize;
use std::collections::HashSet;
use std::process::Command;
use std::sync::Mutex;
use tauri::menu::Menu;
use tauri::{Manager, State, Wry};

pub struct AppState {
    pub credentials: Mutex<Option<claude::Credentials>>,
    pub tray_menu: Mutex<Option<Menu<Wry>>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageData {
    pub five_hour: f32,
    pub five_hour_resets_at: Option<String>,
    pub seven_day: f32,
    pub seven_day_resets_at: Option<String>,
    pub opus: Option<f32>,
    pub opus_resets_at: Option<String>,
    pub sonnet: Option<f32>,
    pub sonnet_resets_at: Option<String>,
    pub extra_usage_enabled: bool,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub extra_usage_pct: Option<f32>,
}

fn build_usage_data(usage: claude::UsageResponse) -> UsageData {
    UsageData {
        five_hour: usage.five_hour.utilization,
        five_hour_resets_at: usage.five_hour.resets_at,
        seven_day: usage.seven_day.utilization,
        seven_day_resets_at: usage.seven_day.resets_at,
        opus: usage.seven_day_opus.as_ref().map(|p| p.utilization),
        opus_resets_at: usage.seven_day_opus.and_then(|p| p.resets_at),
        sonnet: usage.seven_day_sonnet.as_ref().map(|p| p.utilization),
        sonnet_resets_at: usage.seven_day_sonnet.and_then(|p| p.resets_at),
        extra_usage_enabled: usage.extra_usage.is_enabled,
        monthly_limit: usage.extra_usage.monthly_limit,
        used_credits: usage.extra_usage.used_credits,
        extra_usage_pct: usage.extra_usage.utilization,
    }
}

/// Try to get fresh credentials, first from CLI file, then via own refresh.
async fn refresh_credentials(state: &State<'_, AppState>, old_refresh_token: &str) -> Result<claude::Credentials, String> {
    // 1. Re-read CLI file — instant, CLI likely refreshed the token already
    if let Some(cli_creds) = claude::load_cli_credentials() {
        if !cli_creds.is_expired(crate::config::TOKEN_REFRESH_BUFFER_SECS) {
            info!("got fresh token from Claude CLI credentials");
            let mut guard = state.credentials.lock().map_err(|e| e.to_string())?;
            *guard = Some(cli_creds.clone());
            return Ok(cli_creds);
        }
    }

    // 2. Own refresh via API
    info!("CLI credentials unavailable or expired, refreshing via API");
    match claude::refresh_access_token(old_refresh_token).await {
        Ok(new_creds) => {
            let mut guard = state.credentials.lock().map_err(|e| e.to_string())?;
            *guard = Some(new_creds.clone());
            Ok(new_creds)
        }
        Err(e) => {
            warn!("token refresh failed: {}", e);
            let mut guard = state.credentials.lock().map_err(|e| e.to_string())?;
            *guard = None;
            claude::clear_credentials();
            Err("not authenticated".into())
        }
    }
}

#[tauri::command]
pub async fn get_usage(state: State<'_, AppState>) -> Result<UsageData, String> {
    // Always prefer fresh CLI credentials (CLI keeps tokens refreshed automatically)
    let creds = if let Some(cli_creds) = claude::load_cli_credentials() {
        if !cli_creds.is_expired(crate::config::TOKEN_REFRESH_BUFFER_SECS) {
            let mut guard = state.credentials.lock().map_err(|e| e.to_string())?;
            *guard = Some(cli_creds.clone());
            cli_creds
        } else {
            let guard = state.credentials.lock().map_err(|e| e.to_string())?;
            guard.clone().ok_or("not authenticated")?
        }
    } else {
        let guard = state.credentials.lock().map_err(|e| e.to_string())?;
        guard.clone().ok_or("not authenticated")?
    };

    // Proactive: if token is expired/expiring, refresh before calling API
    let creds = if creds.is_expired(crate::config::TOKEN_EXPIRY_BUFFER_SECS) {
        refresh_credentials(&state, &creds.refresh_token).await?
    } else {
        creds
    };

    // Try API call
    match claude::get_usage(&creds.access_token).await {
        Ok(usage) => Ok(build_usage_data(usage)),
        Err(e) if e.contains("401") => {
            // Reactive: token rejected, refresh and retry
            let new_creds = refresh_credentials(&state, &creds.refresh_token).await?;
            let usage = claude::get_usage(&new_creds.access_token).await?;
            Ok(build_usage_data(usage))
        }
        Err(e) if e.contains("429") => {
            // Rate limited — try CLI token if we weren't already using it
            if let Some(cli_creds) = claude::load_cli_credentials() {
                if cli_creds.access_token != creds.access_token {
                    info!("rate limited, retrying with fresh CLI token");
                    {
                        let mut guard = state.credentials.lock().map_err(|e| e.to_string())?;
                        *guard = Some(cli_creds.clone());
                    }
                    let usage = claude::get_usage(&cli_creds.access_token).await?;
                    return Ok(build_usage_data(usage));
                }
            }
            Err(e)
        }
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn login(state: State<'_, AppState>) -> Result<(), String> {
    let creds = claude::oauth_login().await?;
    let mut guard = state.credentials.lock().map_err(|e| e.to_string())?;
    *guard = Some(creds);
    Ok(())
}

#[tauri::command]
pub fn is_authenticated(state: State<'_, AppState>) -> bool {
    state
        .credentials
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

#[tauri::command]
pub fn logout(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.credentials.lock().map_err(|e| e.to_string())?;
    *guard = None;
    claude::clear_credentials();
    Ok(())
}

#[tauri::command]
pub fn update_tray_menu(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    five_hour: f32,
    seven_day: f32,
    five_hour_reset: Option<String>,
    seven_day_reset: Option<String>,
    sonnet: Option<f32>,
    opus: Option<f32>,
) -> Result<(), String> {
    let guard = state.tray_menu.lock().map_err(|e| e.to_string())?;
    let menu = guard.as_ref().ok_or("tray menu not initialized")?;

    fn indicator(pct: f32) -> &'static str {
        if pct >= 75.0 { "🔴" }
        else if pct >= 50.0 { "🟡" }
        else { "🟢" }
    }

    fn set_menu_text(menu: &Menu<Wry>, id: &str, text: &str) {
        if let Some(item) = menu.get(id) {
            if let Some(mi) = item.as_menuitem() {
                let text_owned = text.to_string();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    mi.set_text(&text_owned)
                }));
                if let Err(_) = result {
                    log::warn!("set_menu_text panicked for '{}' (GTK assertion), skipping", id);
                }
            }
        }
    }

    let five_pct = five_hour.round() as i32;
    let seven_pct = seven_day.round() as i32;

    set_menu_text(menu, "session_display",
        &format!("{} Session: {}%", indicator(five_hour), five_pct));
    set_menu_text(menu, "session_reset",
        &format!("     resets {}", five_hour_reset.as_deref().unwrap_or("--")));

    set_menu_text(menu, "weekly_display",
        &format!("{} Weekly: {}%", indicator(seven_day), seven_pct));
    set_menu_text(menu, "weekly_reset",
        &format!("     resets {}", seven_day_reset.as_deref().unwrap_or("--")));

    let sonnet_pct = sonnet.unwrap_or(0.0).round() as i32;
    let opus_pct = opus.unwrap_or(0.0).round() as i32;
    set_menu_text(menu, "sonnet_display",
        &format!("     Sonnet: {}%", sonnet_pct));
    set_menu_text(menu, "opus_display",
        &format!("     Opus: {}%", opus_pct));

    // Update tray title for GNOME panel compact display
    if let Some(tray) = app.tray_by_id("main-tray") {
        let title = format!("{}% | {}%", five_pct, seven_pct);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tray.set_title(Some(&title))
        }));
        if let Err(_) = result {
            log::warn!("tray set_title panicked (GTK assertion), skipping");
        }
    }

    Ok(())
}

#[tauri::command]
pub fn update_tray_icon(app: tauri::AppHandle, max_usage: f32) -> Result<(), String> {
    use crate::tray_icon::{UsageLevel, generate_icon};

    let level = UsageLevel::from_pct(max_usage);
    let png_bytes = generate_icon(level);
    let icon = tauri::image::Image::from_bytes(&png_bytes)
        .map_err(|e| format!("icon decode: {}", e))?;

    if let Some(tray) = app.tray_by_id("main-tray") {
        // Catch GTK assertion panics (e.g. gtk_widget_get_scale_factor on invalid widget)
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tray.set_icon(Some(icon))
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("tray set_icon error: {}", e),
            Err(_) => log::error!("tray set_icon panicked (GTK widget assertion), skipping"),
        }
    }
    Ok(())
}

// === Session Commands ===

#[tauri::command]
pub fn get_waiting_sessions(state: State<'_, SessionsState>) -> Vec<WaitingSession> {
    state
        .sessions
        .lock()
        .map(|s| s.values().cloned().collect())
        .unwrap_or_default()
}

#[tauri::command]
pub fn update_tray_sessions(
    _app: tauri::AppHandle,
    state: State<'_, AppState>,
    count: u32,
) -> Result<(), String> {
    let guard = state.tray_menu.lock().map_err(|e| e.to_string())?;
    let menu = guard.as_ref().ok_or("tray menu not initialized")?;

    fn set_menu_text(menu: &Menu<Wry>, id: &str, text: &str) {
        if let Some(item) = menu.get(id) {
            if let Some(mi) = item.as_menuitem() {
                let text_owned = text.to_string();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    mi.set_text(&text_owned)
                }));
                if let Err(_) = result {
                    log::warn!("set_menu_text panicked for '{}' (GTK assertion), skipping", id);
                }
            }
        }
    }

    let text = if count == 0 {
        "No sessions waiting".to_string()
    } else if count == 1 {
        "1 session waiting".to_string()
    } else {
        format!("{} sessions waiting", count)
    };

    set_menu_text(menu, "sessions_display", &text);

    Ok(())
}

#[tauri::command]
pub fn play_sound() -> Result<(), String> {
    play_system_sound();
    Ok(())
}

/// Play a notification sound using platform-appropriate tools.
fn play_system_sound() {
    if cfg!(target_os = "macos") {
        // macOS: use afplay with system sounds
        let candidates = [
            "/System/Library/Sounds/Glass.aiff",
            "/System/Library/Sounds/Ping.aiff",
            "/System/Library/Sounds/Pop.aiff",
        ];
        for sound in &candidates {
            if std::path::Path::new(sound).exists() {
                let _ = Command::new("afplay").arg(sound).spawn();
                return;
            }
        }
    } else {
        // Linux: use paplay/aplay with freedesktop sounds
        let sound = "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga";
        if std::path::Path::new(sound).exists() {
            let _ = Command::new("paplay").arg(sound).spawn();
        }
    }
}

#[tauri::command]
pub fn enable_turbo_mode() -> Result<(), String> {
    sessions::enable_turbo()
}

#[tauri::command]
pub fn disable_turbo_mode() -> Result<(), String> {
    sessions::disable_turbo()
}

#[tauri::command]
pub fn is_turbo_enabled() -> bool {
    sessions::is_turbo_enabled_check()
}

// === Resume & Recent Sessions ===

/// Find an available terminal emulator.
fn find_terminal() -> Option<String> {
    if cfg!(target_os = "macos") {
        // On macOS, check for popular terminal apps
        let candidates = [
            ("/Applications/iTerm.app", "iTerm"),
            ("/Applications/Terminal.app", "Terminal"),
            ("/System/Applications/Utilities/Terminal.app", "Terminal"),
        ];
        for (app_path, name) in &candidates {
            if std::path::Path::new(app_path).exists() {
                return Some(name.to_string());
            }
        }
        None
    } else {
        let candidates = [
            "gnome-terminal",
            "x-terminal-emulator",
            "konsole",
            "xfce4-terminal",
            "xterm",
        ];
        for term in &candidates {
            if Command::new("which")
                .arg(term)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return Some(term.to_string());
            }
        }
        None
    }
}

/// Shell-escape a string using POSIX single-quote escaping.
fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[tauri::command]
pub fn resume_session(session_id: String, cwd: String) -> Result<(), String> {
    // Validate cwd
    let cwd_path = std::path::Path::new(&cwd);
    if !cwd_path.is_dir() {
        return Err(format!("directory does not exist: {}", cwd));
    }

    let terminal = find_terminal().ok_or("no terminal emulator found")?;
    let escaped_id = shell_escape(&session_id);
    let escaped_cwd = shell_escape(&cwd);
    let bash_cmd = format!("unset CLAUDECODE; claude -r {}; exec bash", escaped_id);

    info!("Resuming session {} in {} via {}", session_id, cwd, terminal);

    let result = if cfg!(target_os = "macos") {
        resume_session_macos(&terminal, &cwd, &bash_cmd)
    } else {
        resume_session_linux(&terminal, &cwd, &escaped_cwd, &bash_cmd)
    };

    result
        .map(|_| ())
        .map_err(|e| format!("failed to spawn terminal: {}", e))
}

/// Resume a session in a macOS terminal emulator.
fn resume_session_macos(terminal: &str, cwd: &str, bash_cmd: &str) -> Result<std::process::Child, std::io::Error> {
    let script = format!(
        "tell application \"{}\" to do script \"cd {} && {}\"",
        terminal,
        shell_escape(cwd),
        bash_cmd.replace('\\', "\\\\").replace('"', "\\\"")
    );
    Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .spawn()
}

/// Resume a session in a Linux terminal emulator.
fn resume_session_linux(terminal: &str, cwd: &str, escaped_cwd: &str, bash_cmd: &str) -> Result<std::process::Child, std::io::Error> {
    match terminal {
        "gnome-terminal" => Command::new(terminal)
            .arg(format!("--working-directory={}", cwd))
            .arg("--")
            .args(["bash", "-c", bash_cmd])
            .spawn(),
        "konsole" => Command::new(terminal)
            .arg("--workdir")
            .arg(cwd)
            .arg("-e")
            .args(["bash", "-c", bash_cmd])
            .spawn(),
        "xfce4-terminal" => Command::new(terminal)
            .arg(format!("--working-directory={}", cwd))
            .arg("-e")
            .arg(&format!("bash -c {}", shell_escape(bash_cmd)))
            .spawn(),
        "xterm" => Command::new(terminal)
            .arg("-e")
            .arg(&format!("cd {} && {}", escaped_cwd, bash_cmd))
            .spawn(),
        _ => Command::new(terminal)
            .arg("-e")
            .args(["bash", "-c", &format!("cd {} && {}", escaped_cwd, bash_cmd)])
            .spawn(),
    }
}

#[tauri::command]
pub fn get_recent_sessions(state: State<'_, SessionsState>) -> Vec<RecentSession> {
    let waiting_ids: HashSet<String> = state
        .sessions
        .lock()
        .map(|s| s.keys().cloned().collect())
        .unwrap_or_default();
    sessions::list_recent_sessions(&waiting_ids)
}

// === Analytics Commands ===

#[tauri::command]
pub fn get_session_analytics(hours: u64) -> analytics::AggregateTokenStats {
    analytics::get_session_analytics(hours)
}

#[tauri::command]
pub fn get_cache_stats(hours: u64) -> analytics::CacheStats {
    analytics::get_cache_stats(hours)
}

#[tauri::command]
pub fn get_tool_stats(hours: u64) -> Vec<analytics::ToolStats> {
    analytics::get_tool_stats(hours)
}

// === Cost Commands (P1) ===

#[tauri::command]
pub fn get_cost_summary(hours: u64) -> cost::CostSummary {
    cost::get_cost_summary(hours)
}

// === Project Cache Stats (P4) ===

#[tauri::command]
pub fn get_project_cache_stats(hours: u64) -> Vec<analytics::ProjectCacheStats> {
    analytics::get_cache_stats_by_project(hours)
}

// === Productivity Stats (P6) ===

#[tauri::command]
pub fn get_productivity_stats(hours: u64) -> analytics::ProductivityStats {
    analytics::get_productivity_stats(hours)
}

#[tauri::command]
pub fn hide_window(app: tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("panel") {
        let _ = tauri::WebviewWindow::hide(&win);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_usage(extra_enabled: bool, limit: Option<f64>, used: Option<f64>, util: Option<f32>) -> claude::UsageResponse {
        claude::UsageResponse {
            five_hour: claude::UsagePeriod { utilization: 42.0, resets_at: Some("2026-01-01T12:00:00Z".into()) },
            seven_day: claude::UsagePeriod { utilization: 55.0, resets_at: Some("2026-01-07T00:00:00Z".into()) },
            seven_day_oauth_apps: None,
            seven_day_opus: Some(claude::UsagePeriod { utilization: 30.0, resets_at: None }),
            seven_day_sonnet: Some(claude::UsagePeriod { utilization: 60.0, resets_at: None }),
            seven_day_cowork: None,
            iguana_necktie: None,
            seven_day_iguana_necktie: None,
            extra_usage: claude::ExtraUsage {
                is_enabled: extra_enabled,
                monthly_limit: limit,
                used_credits: used,
                utilization: util,
            },
            extra: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    #[test]
    fn test_build_usage_data_extra_usage_enabled() {
        let data = build_usage_data(make_usage(true, Some(100.0), Some(42.5), Some(0.425)));
        assert!(data.extra_usage_enabled);
        assert_eq!(data.monthly_limit, Some(100.0));
        assert_eq!(data.used_credits, Some(42.5));
        assert_eq!(data.extra_usage_pct, Some(0.425));
    }

    #[test]
    fn test_build_usage_data_extra_usage_disabled() {
        let data = build_usage_data(make_usage(false, None, None, None));
        assert!(!data.extra_usage_enabled);
        assert_eq!(data.monthly_limit, None);
        assert_eq!(data.used_credits, None);
        assert_eq!(data.extra_usage_pct, None);
    }

    #[test]
    fn test_build_usage_data_base_fields() {
        let data = build_usage_data(make_usage(false, None, None, None));
        assert_eq!(data.five_hour, 42.0);
        assert_eq!(data.seven_day, 55.0);
        assert_eq!(data.opus, Some(30.0));
        assert_eq!(data.sonnet, Some(60.0));
    }
}
