use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentSession {
    pub session_id: String,
    pub slug: String,
    pub project: String,
    pub cwd: String,
    pub last_modified: String,
    pub last_text: String,
    pub status: String, // "active", "waiting", "idle"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingSession {
    pub session_id: String,
    pub slug: String,
    pub project: String,
    pub cwd: String,
    pub idle_since: String,
    pub last_text: String,
    pub session_type: String, // "question", "approval", "completed"
    pub pending_tool: Option<String>,
    pub pending_files: Vec<String>,
}

pub struct SessionsState {
    pub sessions: Mutex<HashMap<String, WaitingSession>>,
    pub notified: Mutex<HashSet<String>>,
}

impl SessionsState {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            notified: Mutex::new(HashSet::new()),
        }
    }
}

/// Read the last N bytes of a file and parse each line as JSON.
fn read_jsonl_tail(path: &Path) -> Vec<serde_json::Value> {
    read_jsonl_tail_bytes(path, crate::config::TAIL_READ_BYTES)
}

fn read_jsonl_tail_bytes(path: &Path, bytes: u64) -> Vec<serde_json::Value> {
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return vec![],
    };

    let file_len = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return vec![],
    };

    let seek_pos = if file_len > bytes {
        file_len - bytes
    } else {
        0
    };

    if file.seek(SeekFrom::Start(seek_pos)).is_err() {
        return vec![];
    }

    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return vec![];
    }

    buf.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str(trimmed).ok()
        })
        .collect()
}

/// Extract project name from cwd (last path component).
pub fn project_from_cwd(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| cwd.to_string())
}

/// Check if any `claude` process is running.
fn claude_processes_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "claude"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Play notification sound using paplay (PulseAudio) or aplay fallback.
fn play_notification_sound() {
    let sound = "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga";
    if Path::new(sound).exists() {
        let _ = Command::new("paplay").arg(sound).spawn();
    }
}

/// Classification result with tool context for P5 enriched session display.
pub struct SessionClassification {
    pub session_type: &'static str,
    pub last_text: String,
    pub pending_tool: Option<String>,
    pub pending_files: Vec<String>,
}

/// Extract tool name and file paths from the last tool_use block in an assistant message.
fn extract_tool_info(assistant_entry: Option<&serde_json::Value>) -> (Option<String>, Vec<String>) {
    let content = assistant_entry
        .and_then(|e| e.pointer("/message/content"))
        .and_then(|c| c.as_array());

    let Some(content) = content else {
        return (None, vec![]);
    };

    let mut tool_name = None;
    let mut files = vec![];

    // Get the LAST tool_use block (the one pending approval)
    for block in content.iter().rev() {
        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
            if tool_name.is_none() {
                tool_name = block.get("name").and_then(|n| n.as_str()).map(String::from);
            }
            if let Some(input) = block.get("input") {
                for key in &["file_path", "path"] {
                    if let Some(val) = input.get(*key).and_then(|v| v.as_str()) {
                        let filename = Path::new(val)
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_else(|| val.to_string());
                        if !files.contains(&filename) {
                            files.push(filename);
                        }
                    }
                }
            }
            break;
        }
    }

    (tool_name, files)
}

/// Classify session state from JSONL tail entries.
/// Returns None if the session is actively working or already has user input at the end.
fn classify_session(entries: &[serde_json::Value]) -> Option<SessionClassification> {
    if entries.is_empty() {
        return None;
    }

    // Find the last meaningful entries
    let last = &entries[entries.len() - 1];
    let last_type = last.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // If user just typed, session is active — not waiting
    if last_type == "user" {
        return None;
    }

    // Look at last few entries for patterns
    let last_assistant = entries.iter().rev().find(|e| {
        e.get("type").and_then(|v| v.as_str()) == Some("assistant")
    });

    let last_text = last_assistant
        .and_then(|e| e.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter().rev().find_map(|item| {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    item.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
        })
        .unwrap_or("");

    let stop_reason = last_assistant
        .and_then(|e| e.get("message"))
        .and_then(|m| m.get("stop_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Check for tool_use waiting for permission
    if stop_reason == "tool_use" {
        if last_type == "assistant" {
            let (pending_tool, pending_files) = extract_tool_info(last_assistant);
            return Some(SessionClassification {
                session_type: "approval",
                last_text: truncate_text_default(last_text),
                pending_tool,
                pending_files,
            });
        }
    }

    // Check for end_turn — Claude finished, waiting for user input
    if stop_reason == "end_turn" && last_type == "assistant" {
        let session_type = if last_text.trim_end().ends_with('?') {
            "question"
        } else {
            "completed"
        };
        return Some(SessionClassification {
            session_type,
            last_text: truncate_text_default(last_text),
            pending_tool: None,
            pending_files: vec![],
        });
    }

    None
}

fn truncate_text_default(text: &str) -> String {
    truncate_text(text, crate::config::LAST_TEXT_TRUNCATION)
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}…", &text[..max_len])
    }
}

/// Find all top-level JSONL files in ~/.claude/projects/*/ modified within `hours`.
pub fn find_session_files_within(hours: u64) -> Vec<PathBuf> {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return vec![],
    };

    let projects_dir = PathBuf::from(&home).join(".claude/projects");
    if !projects_dir.is_dir() {
        return vec![];
    }

    let mut results = vec![];
    let cutoff = SystemTime::now() - Duration::from_secs(hours.saturating_mul(3600));

    if let Ok(project_dirs) = fs::read_dir(&projects_dir) {
        for dir_entry in project_dirs.flatten() {
            let dir_path = dir_entry.path();
            if !dir_path.is_dir() {
                continue;
            }

            if let Ok(files) = fs::read_dir(&dir_path) {
                for file_entry in files.flatten() {
                    let file_path = file_entry.path();
                    if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    // Skip files inside subdirectories (subagents)
                    if !file_path.parent().map(|p| p == dir_path).unwrap_or(false) {
                        continue;
                    }
                    // Skip old files
                    if let Ok(meta) = file_path.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if modified < cutoff {
                                continue;
                            }
                        }
                    }
                    results.push(file_path);
                }
            }
        }
    }

    results
}

/// Find session files modified in the last hour (for waiting session detection).
fn find_session_files() -> Vec<PathBuf> {
    find_session_files_within(crate::config::SESSION_SCAN_WINDOW_HOURS)
}

/// Extract metadata from JSONL entries.
fn extract_metadata(entries: &[serde_json::Value]) -> (String, String, String) {
    let mut slug = String::new();
    let mut cwd = String::new();
    let mut session_id = String::new();

    // Scan from the end for faster access to recent data
    for entry in entries.iter().rev() {
        if slug.is_empty() {
            if let Some(s) = entry.get("slug").and_then(|v| v.as_str()) {
                slug = s.to_string();
            }
        }
        if cwd.is_empty() {
            if let Some(c) = entry.get("cwd").and_then(|v| v.as_str()) {
                cwd = c.to_string();
            }
        }
        if session_id.is_empty() {
            if let Some(id) = entry.get("sessionId").and_then(|v| v.as_str()) {
                session_id = id.to_string();
            }
        }
        if !slug.is_empty() && !cwd.is_empty() && !session_id.is_empty() {
            break;
        }
    }

    (slug, cwd, session_id)
}

/// Check signal files for turbo mode (instant notifications).
fn check_signal_files() -> Vec<(String, String)> {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return vec![],
    };

    let signals_dir = PathBuf::from(&home).join(".config/claude-monitor/signals");
    if !signals_dir.is_dir() {
        return vec![];
    }

    let mut signals = vec![];
    if let Ok(entries) = fs::read_dir(&signals_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if ext == "stop" || ext == "perm" {
                    let session_id = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let signal_type = if ext == "perm" {
                        "approval"
                    } else {
                        "completed"
                    };
                    signals.push((session_id, signal_type.to_string()));
                }
            }
        }
    }

    signals
}

/// Delete signal files for a session.
pub fn delete_signal(session_id: &str) {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };
    let signals_dir = PathBuf::from(&home).join(".config/claude-monitor/signals");
    for ext in &["stop", "perm"] {
        let path = signals_dir.join(format!("{}.{}", session_id, ext));
        let _ = fs::remove_file(path);
    }
}

/// Main session watcher — runs as a tokio background task.
pub fn start_session_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        info!("Session watcher started");
        let mut interval = tokio::time::interval(Duration::from_secs(crate::config::SESSION_WATCHER_INTERVAL_SECS));

        loop {
            interval.tick().await;

            let processes_alive = claude_processes_running();
            let session_files = find_session_files();
            let turbo_enabled = is_turbo_enabled_check();
            let signals = if turbo_enabled {
                check_signal_files()
            } else {
                vec![]
            };

            let mut new_waiting: HashMap<String, WaitingSession> = HashMap::new();

            // Process signal files (turbo mode — instant)
            for (sig_session_id, sig_type) in &signals {
                // Try to find matching JSONL for enrichment
                for file_path in &session_files {
                    let entries = read_jsonl_tail(file_path);
                    let (slug, cwd, session_id) = extract_metadata(&entries);

                    if session_id == *sig_session_id || slug == *sig_session_id {
                        let last_text = entries
                            .iter()
                            .rev()
                            .find(|e| {
                                e.get("type").and_then(|v| v.as_str()) == Some("assistant")
                            })
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.get("content"))
                            .and_then(|c| c.as_array())
                            .and_then(|arr| {
                                arr.iter().rev().find_map(|item| {
                                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                                        item.get("text").and_then(|t| t.as_str())
                                    } else {
                                        None
                                    }
                                })
                            })
                            .unwrap_or("");

                        new_waiting.insert(
                            session_id.clone(),
                            WaitingSession {
                                session_id: session_id.clone(),
                                slug,
                                project: project_from_cwd(&cwd),
                                cwd,
                                idle_since: chrono::Utc::now().to_rfc3339(),
                                last_text: truncate_text_default(last_text),
                                session_type: sig_type.clone(),
                                pending_tool: None,
                                pending_files: vec![],
                            },
                        );
                        break;
                    }
                }
            }

            // Process JSONL files (primary detection)
            for file_path in &session_files {
                let entries = read_jsonl_tail(file_path);
                if entries.is_empty() {
                    continue;
                }

                let (slug, cwd, session_id) = extract_metadata(&entries);
                if session_id.is_empty() {
                    continue;
                }

                // Skip if already found via signal
                if new_waiting.contains_key(&session_id) {
                    continue;
                }

                // Check file staleness (must be idle >5s for "waiting" classification)
                let stale_enough = file_path
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
                    .map(|elapsed| elapsed.as_secs() >= crate::config::STALE_THRESHOLD_SECS)
                    .unwrap_or(false);

                if !stale_enough {
                    // File recently modified — session is actively working
                    continue;
                }

                // No claude process running — session is closed
                if !processes_alive {
                    continue;
                }

                if let Some(classification) = classify_session(&entries) {
                    let idle_since = file_path
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|mtime| {
                            let duration = mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
                            Some(
                                chrono::DateTime::from_timestamp(
                                    duration.as_secs() as i64,
                                    duration.subsec_nanos(),
                                )?
                                .to_rfc3339(),
                            )
                        })
                        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

                    new_waiting.insert(
                        session_id.clone(),
                        WaitingSession {
                            session_id: session_id.clone(),
                            slug,
                            project: project_from_cwd(&cwd),
                            cwd,
                            idle_since,
                            last_text: classification.last_text,
                            session_type: classification.session_type.to_string(),
                            pending_tool: classification.pending_tool,
                            pending_files: classification.pending_files,
                        },
                    );
                }
            }

            // Compare with current state and emit changes
            let state = app.state::<SessionsState>();
            let (changed, removed_ids) = {
                let current = state.sessions.lock().unwrap();
                let current_keys: HashSet<_> = current.keys().cloned().collect();
                let new_keys: HashSet<_> = new_waiting.keys().cloned().collect();
                let removed: Vec<String> = current_keys.difference(&new_keys).cloned().collect();
                (current_keys != new_keys, removed)
            };

            // Clean up signal files for sessions that are no longer waiting
            if turbo_enabled {
                for id in &removed_ids {
                    delete_signal(id);
                }
            }

            // Check for newly waiting sessions (for notification)
            let new_session_ids: Vec<String> = {
                let notified = state.notified.lock().unwrap();
                new_waiting
                    .keys()
                    .filter(|id| !notified.contains(*id))
                    .cloned()
                    .collect()
            };

            // Update state
            {
                let mut sessions = state.sessions.lock().unwrap();
                *sessions = new_waiting.clone();
            }

            // Clean notified set — remove sessions that are no longer waiting
            {
                let mut notified = state.notified.lock().unwrap();
                notified.retain(|id| new_waiting.contains_key(id));
            }

            // Notify for new sessions
            for session_id in &new_session_ids {
                if let Some(session) = new_waiting.get(session_id) {
                    info!(
                        "New waiting session: {} ({}) — {}",
                        session.project, session.session_type, session.session_id
                    );
                    play_notification_sound();

                    // Mark as notified
                    let mut notified = state.notified.lock().unwrap();
                    notified.insert(session_id.clone());
                }
            }

            // Emit event if anything changed
            if changed || !new_session_ids.is_empty() {
                let sessions_vec: Vec<WaitingSession> = new_waiting.values().cloned().collect();
                let _ = app.emit("sessions-changed", &sessions_vec);
                debug!("Emitted sessions-changed with {} sessions", sessions_vec.len());
            }
        }
    });
}

/// List recent sessions from the last 24 hours.
pub fn list_recent_sessions(waiting_ids: &HashSet<String>) -> Vec<RecentSession> {
    let session_files = find_session_files_within(crate::config::RECENT_SESSIONS_HOURS);
    let mut results = Vec::new();

    for file_path in &session_files {
        let entries = read_jsonl_tail(file_path);
        if entries.is_empty() {
            continue;
        }

        let (slug, cwd, session_id) = extract_metadata(&entries);
        if session_id.is_empty() {
            continue;
        }

        // Skip sessions already in the waiting list
        if waiting_ids.contains(&session_id) {
            continue;
        }

        let last_modified = file_path
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|mtime| {
                let duration = mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
                Some(
                    chrono::DateTime::from_timestamp(
                        duration.as_secs() as i64,
                        duration.subsec_nanos(),
                    )?
                    .to_rfc3339(),
                )
            })
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        // Classify status
        let status: &str = if let Some(mtime) = file_path.metadata().ok().and_then(|m| m.modified().ok())
        {
            let elapsed = SystemTime::now()
                .duration_since(mtime)
                .unwrap_or(Duration::from_secs(0));
            if elapsed.as_secs() < crate::config::STALE_THRESHOLD_SECS {
                "active"
            } else if classify_session(&entries).is_some() {
                "waiting"
            } else {
                "idle"
            }
        } else {
            "idle"
        };

        let last_text = entries
            .iter()
            .rev()
            .find(|e| e.get("type").and_then(|v| v.as_str()) == Some("assistant"))
            .and_then(|e| e.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                arr.iter().rev().find_map(|item| {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        item.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or("");

        results.push(RecentSession {
            session_id,
            slug,
            project: project_from_cwd(&cwd),
            cwd,
            last_modified,
            last_text: truncate_text_default(last_text),
            status: status.to_string(),
        });
    }

    // Sort by last_modified desc
    results.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    results.truncate(crate::config::RECENT_SESSIONS_LIMIT);

    results
}

/// Check if turbo mode is enabled by looking for the signal script.
pub fn is_turbo_enabled_check() -> bool {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return false,
    };
    PathBuf::from(&home)
        .join(".claude/hooks/claude-monitor-signal.sh")
        .exists()
}

/// Install turbo mode hooks.
pub fn enable_turbo() -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let home_path = PathBuf::from(&home);

    // 1. Create signals directory
    let signals_dir = home_path.join(".config/claude-monitor/signals");
    fs::create_dir_all(&signals_dir).map_err(|e| format!("create signals dir: {}", e))?;

    // 2. Create the hook script
    let hooks_dir = home_path.join(".claude/hooks");
    fs::create_dir_all(&hooks_dir).map_err(|e| format!("create hooks dir: {}", e))?;

    let script_path = hooks_dir.join("claude-monitor-signal.sh");
    let script = r#"#!/bin/bash
# claude-monitor turbo mode signal script
# Writes a signal file for instant notification pickup
SIGNALS_DIR="$HOME/.config/claude-monitor/signals"
mkdir -p "$SIGNALS_DIR"

# Use SESSION_ID from environment or the hook event type
SESSION_ID="${SESSION_ID:-$$}"
HOOK_EVENT="${CLAUDE_HOOK_EVENT:-stop}"

if [ "$HOOK_EVENT" = "stop" ] || [ "$HOOK_EVENT" = "Stop" ]; then
    touch "$SIGNALS_DIR/${SESSION_ID}.stop"
elif [ "$HOOK_EVENT" = "notification" ] || [ "$HOOK_EVENT" = "Notification" ]; then
    touch "$SIGNALS_DIR/${SESSION_ID}.perm"
fi
"#;
    fs::write(&script_path, script)
        .map_err(|e| format!("write script: {}", e))?;

    // Make executable
    Command::new("chmod")
        .args(["+x", &script_path.to_string_lossy()])
        .output()
        .map_err(|e| format!("chmod: {}", e))?;

    // 3. Merge hooks into ~/.claude/settings.json
    let settings_path = home_path.join(".claude/settings.json");
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content =
            fs::read_to_string(&settings_path).map_err(|e| format!("read settings: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("parse settings: {}", e))?
    } else {
        serde_json::json!({})
    };

    let script_str = script_path.to_string_lossy().to_string();
    let hook_entry = serde_json::json!({
        "type": "command",
        "command": script_str
    });

    let hooks = settings
        .as_object_mut()
        .ok_or("settings not an object")?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    let hooks_obj = hooks
        .as_object_mut()
        .ok_or("hooks not an object")?;

    // Add Stop hook
    let stop_hooks = hooks_obj
        .entry("Stop")
        .or_insert_with(|| serde_json::json!([]));
    if let Some(arr) = stop_hooks.as_array_mut() {
        let already = arr.iter().any(|h| {
            h.get("command")
                .and_then(|c| c.as_str())
                .map(|c| c.contains("claude-monitor-signal"))
                .unwrap_or(false)
        });
        if !already {
            arr.push(hook_entry.clone());
        }
    }

    let pretty =
        serde_json::to_string_pretty(&settings).map_err(|e| format!("serialize: {}", e))?;
    fs::write(&settings_path, pretty).map_err(|e| format!("write settings: {}", e))?;

    info!("Turbo mode enabled");
    Ok(())
}

/// Remove turbo mode hooks.
pub fn disable_turbo() -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let home_path = PathBuf::from(&home);

    // 1. Remove hook script
    let script_path = home_path.join(".claude/hooks/claude-monitor-signal.sh");
    let _ = fs::remove_file(&script_path);

    // 2. Remove hooks from settings.json
    let settings_path = home_path.join(".claude/settings.json");
    if settings_path.exists() {
        let content =
            fs::read_to_string(&settings_path).map_err(|e| format!("read settings: {}", e))?;
        let mut settings: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("parse settings: {}", e))?;

        if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            for key in &["Stop"] {
                if let Some(arr) = hooks.get_mut(*key).and_then(|v| v.as_array_mut()) {
                    arr.retain(|h| {
                        !h.get("command")
                            .and_then(|c| c.as_str())
                            .map(|c| c.contains("claude-monitor-signal"))
                            .unwrap_or(false)
                    });
                }
            }
            // Clean up empty arrays
            let empty_keys: Vec<String> = hooks
                .iter()
                .filter(|(_, v)| v.as_array().map(|a| a.is_empty()).unwrap_or(false))
                .map(|(k, _)| k.clone())
                .collect();
            for key in empty_keys {
                hooks.remove(&key);
            }
        }

        let pretty =
            serde_json::to_string_pretty(&settings).map_err(|e| format!("serialize: {}", e))?;
        fs::write(&settings_path, pretty).map_err(|e| format!("write settings: {}", e))?;
    }

    // 3. Clean up signal files
    let signals_dir = home_path.join(".config/claude-monitor/signals");
    let _ = fs::remove_dir_all(&signals_dir);

    info!("Turbo mode disabled");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_text_short() {
        assert_eq!(truncate_text("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_text_exact() {
        assert_eq!(truncate_text("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_text_long() {
        let result = truncate_text("hello world", 5);
        assert_eq!(result, "hello…");
    }

    #[test]
    fn test_truncate_text_default_uses_config() {
        let long = "a".repeat(300);
        let result = truncate_text_default(&long);
        assert_eq!(
            result.len(),
            crate::config::LAST_TEXT_TRUNCATION + "…".len()
        );
    }

    #[test]
    fn test_project_from_cwd() {
        assert_eq!(project_from_cwd("/home/user/projects/myapp"), "myapp");
        assert_eq!(project_from_cwd("/"), "/");
        assert_eq!(project_from_cwd("single"), "single");
    }

    #[test]
    fn test_classify_session_empty() {
        assert!(classify_session(&[]).is_none());
    }

    #[test]
    fn test_classify_session_user_last() {
        let entries = vec![serde_json::json!({"type": "user"})];
        assert!(classify_session(&entries).is_none());
    }

    #[test]
    fn test_classify_session_end_turn_question() {
        let entries = vec![serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [{"type": "text", "text": "What should I do?"}],
                "stop_reason": "end_turn"
            }
        })];
        let result = classify_session(&entries);
        assert!(result.is_some());
        let c = result.unwrap();
        assert_eq!(c.session_type, "question");
        assert!(c.last_text.contains("What should I do?"));
        assert!(c.pending_tool.is_none());
    }

    #[test]
    fn test_classify_session_end_turn_completed() {
        let entries = vec![serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [{"type": "text", "text": "Done, all changes applied."}],
                "stop_reason": "end_turn"
            }
        })];
        let result = classify_session(&entries);
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_type, "completed");
    }

    #[test]
    fn test_classify_session_tool_use_approval() {
        let entries = vec![serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "text", "text": "I want to edit file.rs"},
                    {"type": "tool_use", "name": "Edit", "id": "t1", "input": {"file_path": "/home/user/project/src/main.rs"}}
                ],
                "stop_reason": "tool_use"
            }
        })];
        let result = classify_session(&entries);
        assert!(result.is_some());
        let c = result.unwrap();
        assert_eq!(c.session_type, "approval");
        assert_eq!(c.pending_tool, Some("Edit".to_string()));
        assert_eq!(c.pending_files, vec!["main.rs"]);
    }

    #[test]
    fn test_classify_session_tool_use_bash() {
        let entries = vec![serde_json::json!({
            "type": "assistant",
            "message": {
                "content": [
                    {"type": "tool_use", "name": "Bash", "id": "t1", "input": {"command": "npm test"}}
                ],
                "stop_reason": "tool_use"
            }
        })];
        let result = classify_session(&entries);
        assert!(result.is_some());
        let c = result.unwrap();
        assert_eq!(c.pending_tool, Some("Bash".to_string()));
        assert!(c.pending_files.is_empty()); // Bash doesn't have file_path
    }

    #[test]
    fn test_extract_metadata() {
        let entries = vec![
            serde_json::json!({"type": "user", "sessionId": "abc-123", "cwd": "/home/user/project", "slug": "my-session"}),
            serde_json::json!({"type": "assistant"}),
        ];
        let (slug, cwd, session_id) = extract_metadata(&entries);
        assert_eq!(slug, "my-session");
        assert_eq!(cwd, "/home/user/project");
        assert_eq!(session_id, "abc-123");
    }

    #[test]
    fn test_extract_metadata_empty() {
        let (slug, cwd, session_id) = extract_metadata(&[]);
        assert!(slug.is_empty());
        assert!(cwd.is_empty());
        assert!(session_id.is_empty());
    }

    #[test]
    fn test_read_jsonl_tail_bytes_nonexistent() {
        let path = std::path::Path::new("/nonexistent/file.jsonl");
        let result = read_jsonl_tail_bytes(path, 1024);
        assert!(result.is_empty());
    }
}
