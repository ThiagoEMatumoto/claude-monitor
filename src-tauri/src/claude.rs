use base64::{engine::general_purpose, Engine as _};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

pub const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
pub const ANTHROPIC_AUTH_URL: &str = "https://claude.ai/oauth/authorize";
pub const ANTHROPIC_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
pub const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const ANTHROPIC_AUTH_SCOPE: &str = "user:profile user:inference user:sessions:claude_code";
pub const OAUTH_REDIRECT_PORT: u16 = 54546;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

impl Credentials {
    /// Returns true if the token expires within `buffer_secs` seconds.
    /// Returns false if expiry is unknown (legacy credentials).
    pub fn is_expired(&self, buffer_secs: i64) -> bool {
        match self.expires_at {
            Some(exp) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                now + buffer_secs >= exp
            }
            None => false,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub token_type: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsagePeriod {
    pub utilization: f32,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtraUsage {
    pub is_enabled: bool,
    pub monthly_limit: Option<f64>,
    pub used_credits: Option<f64>,
    pub utilization: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsageResponse {
    pub five_hour: UsagePeriod,
    pub seven_day: UsagePeriod,
    pub seven_day_oauth_apps: Option<UsagePeriod>,
    pub seven_day_opus: Option<UsagePeriod>,
    pub seven_day_sonnet: Option<UsagePeriod>,
    pub seven_day_cowork: Option<UsagePeriod>,
    pub iguana_necktie: Option<UsagePeriod>,
    pub seven_day_iguana_necktie: Option<UsagePeriod>,
    pub extra_usage: ExtraUsage,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

// --- CLI Credentials (primary source) ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliOAuthCredentials {
    access_token: String,
    refresh_token: String,
    expires_at: Option<i64>, // milliseconds
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliCredentialsFile {
    claude_ai_oauth: Option<CliOAuthCredentials>,
}

/// Load credentials from the Claude CLI file (~/.claude/.credentials.json).
/// The CLI keeps this token fresh automatically, so it's the preferred source.
pub fn load_cli_credentials() -> Option<Credentials> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(&home).join(".claude/.credentials.json");
    let data = fs::read_to_string(&path).ok()?;
    let file: CliCredentialsFile = serde_json::from_str(&data).ok()?;
    let oauth = file.claude_ai_oauth?;
    info!("loaded credentials from Claude CLI: {:?}", path);
    Some(Credentials {
        access_token: oauth.access_token,
        refresh_token: oauth.refresh_token,
        expires_at: oauth.expires_at.map(|ms| ms / 1000), // ms → seconds
    })
}

// --- Own Credentials (fallback) ---

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/claude-monitor")
}

pub fn load_credentials() -> Option<Credentials> {
    // 1. Claude CLI credentials (primary — always fresh)
    if let Some(creds) = load_cli_credentials() {
        return Some(creds);
    }

    // 2. Own credentials
    let own_path = config_dir().join("credentials.json");
    if let Ok(data) = fs::read_to_string(&own_path) {
        if let Ok(creds) = serde_json::from_str(&data) {
            info!("loaded credentials from {:?}", own_path);
            return Some(creds);
        }
    }

    // 3. Legacy claude-tray credentials
    let home = std::env::var("HOME").ok()?;
    let tray_path = PathBuf::from(&home).join(".config/claude-tray/credentials.json");
    if let Ok(data) = fs::read_to_string(&tray_path) {
        if let Ok(creds) = serde_json::from_str::<Credentials>(&data) {
            info!("loaded credentials from claude-tray: {:?}", tray_path);
            let _ = save_credentials(&creds);
            return Some(creds);
        }
    }

    None
}

pub fn save_credentials(creds: &Credentials) -> Result<(), String> {
    let dir = config_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(creds).map_err(|e| e.to_string())?;
    fs::write(dir.join("credentials.json"), json).map_err(|e| e.to_string())
}

pub fn clear_credentials() {
    let path = config_dir().join("credentials.json");
    let _ = fs::remove_file(path);
}

// --- OAuth ---

fn extract_param(request: &str, param: &str) -> Result<String, String> {
    let search = format!("{}=", param);
    let start = request
        .find(&search)
        .ok_or_else(|| format!("param {} not found", param))?
        + search.len();
    let end = request[start..]
        .find(|c: char| c == '&' || c == ' ' || c == '\r' || c == '\n')
        .map(|i| start + i)
        .unwrap_or(request.len());
    Ok(request[start..end].to_string())
}

pub async fn oauth_login() -> Result<Credentials, String> {
    let verifier = {
        let bytes: [u8; 32] = rand::random();
        general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    };
    let state = {
        let bytes: [u8; 32] = rand::random();
        hex::encode(bytes)
    };
    let challenge = {
        let mut h = Sha256::new();
        h.update(verifier.as_bytes());
        general_purpose::URL_SAFE_NO_PAD.encode(h.finalize())
    };

    let redirect = format!("http://localhost:{}/callback", OAUTH_REDIRECT_PORT);
    let auth_url = format!(
        "{}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        ANTHROPIC_AUTH_URL,
        ANTHROPIC_CLIENT_ID,
        urlencoding::encode(&redirect),
        urlencoding::encode(ANTHROPIC_AUTH_SCOPE),
        challenge,
        state
    );

    open::that(&auth_url).map_err(|e| format!("failed to open browser: {}", e))?;

    let listener = TcpListener::bind(format!("127.0.0.1:{}", OAUTH_REDIRECT_PORT))
        .map_err(|e| e.to_string())?;
    let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
    let mut buf = [0u8; 2048];
    stream.read(&mut buf).map_err(|e| e.to_string())?;
    let req = String::from_utf8_lossy(&buf);

    let recv_state = extract_param(&req, "state")?;
    if recv_state != state {
        return Err("state mismatch".into());
    }
    let code = extract_param(&req, "code")?;

    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Login successful! You can close this tab.</h1></body></html>"
    );

    let body = json!({
        "code": code,
        "state": state,
        "grant_type": "authorization_code",
        "client_id": ANTHROPIC_CLIENT_ID,
        "redirect_uri": redirect,
        "code_verifier": verifier
    });

    let client = reqwest::Client::builder()
        .timeout(crate::config::HTTP_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(ANTHROPIC_TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let text = resp.text().await.map_err(|e| e.to_string())?;
    let token: TokenResponse =
        serde_json::from_str(&text).map_err(|e| format!("parse error: {} — body: {}", e, text))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let creds = Credentials {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: Some(now + token.expires_in as i64),
    };
    save_credentials(&creds)?;
    Ok(creds)
}

// --- Token Refresh ---

pub async fn refresh_access_token(refresh_token: &str) -> Result<Credentials, String> {
    let body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": ANTHROPIC_CLIENT_ID,
    });

    let client = reqwest::Client::builder()
        .timeout(crate::config::HTTP_TIMEOUT)
        .build()
        .map_err(|e| format!("refresh client build failed: {}", e))?;

    let resp = client
        .post(ANTHROPIC_TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("refresh request failed: {}", e))?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;

    if !status.is_success() {
        warn!("token refresh failed {}: {}", status, text);
        return Err(format!("refresh failed {}: {}", status, text));
    }

    let token: TokenResponse = serde_json::from_str(&text)
        .map_err(|e| format!("refresh parse error: {} — body: {}", e, text))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let creds = Credentials {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: Some(now + token.expires_in as i64),
    };
    save_credentials(&creds)?;
    info!("access token refreshed successfully");
    Ok(creds)
}

// --- Usage API ---

pub async fn get_usage(access_token: &str) -> Result<UsageResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(crate::config::HTTP_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(CLAUDE_USAGE_URL)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-monitor/0.1.0")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let status_code = status.as_u16();
    let text = resp.text().await.map_err(|e| e.to_string())?;

    if !status.is_success() {
        warn!("usage API returned {}: {}", status, text);
        return Err(format!("{}: {}", status_code, text));
    }

    serde_json::from_str(&text).map_err(|e| format!("parse error: {} — body: {}", e, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_not_expired() {
        let future = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 3600;
        let creds = Credentials {
            access_token: "test".into(),
            refresh_token: "test".into(),
            expires_at: Some(future),
        };
        assert!(!creds.is_expired(60));
    }

    #[test]
    fn test_credentials_expired() {
        let past = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 100;
        let creds = Credentials {
            access_token: "test".into(),
            refresh_token: "test".into(),
            expires_at: Some(past),
        };
        assert!(creds.is_expired(0));
    }

    #[test]
    fn test_credentials_no_expiry() {
        let creds = Credentials {
            access_token: "test".into(),
            refresh_token: "test".into(),
            expires_at: None,
        };
        assert!(!creds.is_expired(300));
    }

    #[test]
    fn test_credentials_expiring_within_buffer() {
        let soon = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + 30;
        let creds = Credentials {
            access_token: "test".into(),
            refresh_token: "test".into(),
            expires_at: Some(soon),
        };
        assert!(creds.is_expired(60)); // within buffer
        assert!(!creds.is_expired(10)); // outside buffer
    }

    #[test]
    fn test_extract_param_success() {
        let req = "GET /callback?code=abc123&state=xyz HTTP/1.1";
        assert_eq!(extract_param(req, "code").unwrap(), "abc123");
        assert_eq!(extract_param(req, "state").unwrap(), "xyz");
    }

    #[test]
    fn test_extract_param_not_found() {
        let req = "GET /callback?code=abc HTTP/1.1";
        assert!(extract_param(req, "missing").is_err());
    }
}
