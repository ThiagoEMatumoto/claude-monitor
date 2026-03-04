use crate::claude;
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub credentials: Mutex<Option<claude::Credentials>>,
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
