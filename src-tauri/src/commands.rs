use crate::claude;
use serde::Serialize;
use std::sync::Mutex;
use tauri::menu::Menu;
use tauri::{State, Wry};

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
}

#[tauri::command]
pub async fn get_usage(state: State<'_, AppState>) -> Result<UsageData, String> {
    let token = {
        let guard = state.credentials.lock().map_err(|e| e.to_string())?;
        guard
            .as_ref()
            .ok_or("not authenticated")?
            .access_token
            .clone()
    };

    let usage = claude::get_usage(&token).await?;

    Ok(UsageData {
        five_hour: usage.five_hour.utilization,
        five_hour_resets_at: usage.five_hour.resets_at,
        seven_day: usage.seven_day.utilization,
        seven_day_resets_at: usage.seven_day.resets_at,
        opus: usage.seven_day_opus.as_ref().map(|p| p.utilization),
        opus_resets_at: usage.seven_day_opus.and_then(|p| p.resets_at),
        sonnet: usage.seven_day_sonnet.as_ref().map(|p| p.utilization),
        sonnet_resets_at: usage.seven_day_sonnet.and_then(|p| p.resets_at),
    })
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
                let _ = mi.set_text(text);
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
        let _ = tray.set_title(Some(&format!("{}% | {}%", five_pct, seven_pct)));
    }

    Ok(())
}
