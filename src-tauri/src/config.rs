#![allow(dead_code)]
use std::time::Duration;

// --- HTTP ---
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

// --- Polling ---
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 300;

// --- Sessions ---
pub const SESSION_SCAN_WINDOW_HOURS: u64 = 1;
pub const RECENT_SESSIONS_HOURS: u64 = 24;
pub const STALE_THRESHOLD_SECS: u64 = 5;
pub const RECENT_SESSIONS_LIMIT: usize = 20;
pub const LAST_TEXT_TRUNCATION: usize = 200;
pub const SESSION_WATCHER_INTERVAL_SECS: u64 = 3;

// --- File Reading ---
pub const TAIL_READ_BYTES: u64 = 10240;

// --- Alerts ---
pub const DEFAULT_WARNING_THRESHOLD: u32 = 75;
pub const DEFAULT_CRITICAL_THRESHOLD: u32 = 90;

// --- Notifications ---
pub const NOTIFICATION_COOLDOWN_MINS: u64 = 30;

// --- Snapshots ---
pub const SNAPSHOT_RETENTION_DAYS: u32 = 30;

// --- Token ---
pub const TOKEN_EXPIRY_BUFFER_SECS: i64 = 300;
pub const TOKEN_REFRESH_BUFFER_SECS: i64 = 60;
