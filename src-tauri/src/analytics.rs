use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::sessions;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenSummary {
    pub session_id: String,
    pub project: String,
    pub model: String,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub message_count: u64,
    pub tool_calls: Vec<ToolCallRecord>,
    pub first_seen: String,
    pub last_seen: String,
    pub file_offset: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub call_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateTokenStats {
    pub total_input: u64,
    pub total_output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub session_count: u64,
    pub message_count: u64,
    pub sessions: Vec<SessionTokenSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub total_cache_creation: u64,
    pub total_cache_read: u64,
    pub total_uncached_input: u64,
    pub hit_rate: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStats {
    pub tool_name: String,
    pub call_count: u64,
    pub pct_of_total: f64,
}

/// Scan a JSONL session file from a given offset, extracting token usage and tool call data.
pub fn scan_session_tokens(path: &Path, last_offset: u64) -> Option<(SessionTokenSummary, u64)> {
    let mut file = fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();

    if file_len <= last_offset {
        return None; // No new data
    }

    if last_offset > 0 {
        file.seek(SeekFrom::Start(last_offset)).ok()?;
    }

    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;

    let mut summary = SessionTokenSummary::default();
    let mut tool_counts: HashMap<String, u64> = HashMap::new();
    let mut has_data = false;

    for line in buf.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Extract session metadata from any entry
        if summary.session_id.is_empty() {
            if let Some(id) = entry.get("sessionId").and_then(|v| v.as_str()) {
                summary.session_id = id.to_string();
            }
        }
        if summary.project.is_empty() {
            if let Some(cwd) = entry.get("cwd").and_then(|v| v.as_str()) {
                summary.project = sessions::project_from_cwd(cwd);
            }
        }

        // Extract timestamp for first_seen/last_seen
        if let Some(ts) = entry.get("timestamp").and_then(|v| v.as_str()) {
            if summary.first_seen.is_empty() {
                summary.first_seen = ts.to_string();
            }
            summary.last_seen = ts.to_string();
        }

        // Only process assistant messages for token counts
        let entry_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if entry_type != "assistant" {
            continue;
        }

        has_data = true;
        summary.message_count += 1;

        // Extract model
        if let Some(model) = entry
            .pointer("/message/model")
            .and_then(|v| v.as_str())
        {
            summary.model = model.to_string();
        }

        // Extract token usage
        if let Some(usage) = entry.pointer("/message/usage") {
            summary.total_input_tokens += usage
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            summary.total_output_tokens += usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            summary.cache_creation_tokens += usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            summary.cache_read_tokens += usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
        }

        // Extract tool calls from message content
        if let Some(content) = entry.pointer("/message/content").and_then(|v| v.as_array()) {
            for block in content {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                    if let Some(name) = block.get("name").and_then(|v| v.as_str()) {
                        *tool_counts.entry(name.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    if !has_data && summary.session_id.is_empty() {
        return None;
    }

    // Convert tool counts to sorted records
    let mut tools: Vec<ToolCallRecord> = tool_counts
        .into_iter()
        .map(|(tool_name, call_count)| ToolCallRecord { tool_name, call_count })
        .collect();
    tools.sort_by(|a, b| b.call_count.cmp(&a.call_count));
    summary.tool_calls = tools;
    summary.file_offset = file_len;

    Some((summary, file_len))
}

/// Scan all session files modified within `hours` and return aggregated stats.
pub fn get_session_analytics(hours: u64) -> AggregateTokenStats {
    let files = sessions::find_session_files_within(hours);
    let mut sessions_list = Vec::new();
    let mut total_input = 0u64;
    let mut total_output = 0u64;
    let mut cache_creation = 0u64;
    let mut cache_read = 0u64;
    let mut message_count = 0u64;

    for path in &files {
        match scan_session_tokens(path, 0) {
            Some((summary, _)) => {
                if summary.message_count == 0 && summary.session_id.is_empty() {
                    continue;
                }
                total_input += summary.total_input_tokens;
                total_output += summary.total_output_tokens;
                cache_creation += summary.cache_creation_tokens;
                cache_read += summary.cache_read_tokens;
                message_count += summary.message_count;
                sessions_list.push(summary);
            }
            None => {
                debug!("skipping empty/unreadable session file: {:?}", path);
            }
        }
    }

    // Sort by last_seen descending
    sessions_list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

    AggregateTokenStats {
        total_input,
        total_output,
        cache_creation,
        cache_read,
        session_count: sessions_list.len() as u64,
        message_count,
        sessions: sessions_list,
    }
}

/// Compute cache hit rate from aggregate data.
pub fn get_cache_stats(hours: u64) -> CacheStats {
    let analytics = get_session_analytics(hours);
    let uncached = analytics.total_input.saturating_sub(analytics.cache_creation + analytics.cache_read);
    let total = analytics.cache_read + analytics.cache_creation + uncached;
    let hit_rate = if total > 0 {
        analytics.cache_read as f64 / total as f64
    } else {
        0.0
    };

    CacheStats {
        total_cache_creation: analytics.cache_creation,
        total_cache_read: analytics.cache_read,
        total_uncached_input: uncached,
        hit_rate,
    }
}

/// Get top tools by call count across recent sessions.
pub fn get_tool_stats(hours: u64) -> Vec<ToolStats> {
    let analytics = get_session_analytics(hours);
    let mut tool_totals: HashMap<String, u64> = HashMap::new();

    for session in &analytics.sessions {
        for tool in &session.tool_calls {
            *tool_totals.entry(tool.tool_name.clone()).or_insert(0) += tool.call_count;
        }
    }

    let grand_total: u64 = tool_totals.values().sum();
    let mut stats: Vec<ToolStats> = tool_totals
        .into_iter()
        .map(|(tool_name, call_count)| ToolStats {
            tool_name,
            call_count,
            pct_of_total: if grand_total > 0 {
                (call_count as f64 / grand_total as f64) * 100.0
            } else {
                0.0
            },
        })
        .collect();

    stats.sort_by(|a, b| b.call_count.cmp(&a.call_count));
    stats.truncate(15);
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_test_jsonl(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
        let path = dir.join("test-session.jsonl");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_scan_session_tokens_basic() {
        let dir = tempfile::tempdir().unwrap();
        let content = r#"{"type":"user","sessionId":"abc-123","cwd":"/home/user/project","timestamp":"2026-01-01T00:00:00Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"Hello"}],"usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":20,"cache_read_input_tokens":30},"stop_reason":"end_turn"},"timestamp":"2026-01-01T00:00:01Z"}
{"type":"assistant","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"tool_use","name":"Read","id":"t1"},{"type":"text","text":"Reading file"}],"usage":{"input_tokens":200,"output_tokens":80,"cache_creation_input_tokens":0,"cache_read_input_tokens":150},"stop_reason":"tool_use"},"timestamp":"2026-01-01T00:00:02Z"}
"#;
        let path = create_test_jsonl(dir.path(), content);

        let (summary, offset) = scan_session_tokens(&path, 0).unwrap();
        assert_eq!(summary.session_id, "abc-123");
        assert_eq!(summary.project, "project");
        assert_eq!(summary.model, "claude-sonnet-4-20250514");
        assert_eq!(summary.total_input_tokens, 300);
        assert_eq!(summary.total_output_tokens, 130);
        assert_eq!(summary.cache_creation_tokens, 20);
        assert_eq!(summary.cache_read_tokens, 180);
        assert_eq!(summary.message_count, 2);
        assert_eq!(summary.first_seen, "2026-01-01T00:00:00Z");
        assert_eq!(summary.last_seen, "2026-01-01T00:00:02Z");
        assert!(offset > 0);

        // Tool calls
        assert_eq!(summary.tool_calls.len(), 1);
        assert_eq!(summary.tool_calls[0].tool_name, "Read");
        assert_eq!(summary.tool_calls[0].call_count, 1);
    }

    #[test]
    fn test_scan_session_tokens_incremental() {
        let dir = tempfile::tempdir().unwrap();
        let line1 = r#"{"type":"assistant","sessionId":"s1","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"Hi"}],"usage":{"input_tokens":50,"output_tokens":25},"stop_reason":"end_turn"},"timestamp":"2026-01-01T00:00:00Z"}"#;
        let line2 = r#"{"type":"assistant","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"text","text":"More"}],"usage":{"input_tokens":75,"output_tokens":30},"stop_reason":"end_turn"},"timestamp":"2026-01-01T00:01:00Z"}"#;

        let path = dir.path().join("test.jsonl");
        fs::write(&path, format!("{}\n", line1)).unwrap();

        let (s1, offset1) = scan_session_tokens(&path, 0).unwrap();
        assert_eq!(s1.total_input_tokens, 50);
        assert_eq!(s1.message_count, 1);

        // Append more data
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(format!("{}\n", line2).as_bytes()).unwrap();

        let (s2, _) = scan_session_tokens(&path, offset1).unwrap();
        assert_eq!(s2.total_input_tokens, 75);
        assert_eq!(s2.message_count, 1);
    }

    #[test]
    fn test_scan_session_tokens_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = create_test_jsonl(dir.path(), "");
        assert!(scan_session_tokens(&path, 0).is_none());
    }

    #[test]
    fn test_scan_session_tokens_no_new_data() {
        let dir = tempfile::tempdir().unwrap();
        let content = r#"{"type":"user","sessionId":"s1"}"#;
        let path = create_test_jsonl(dir.path(), content);
        let file_len = fs::metadata(&path).unwrap().len();
        assert!(scan_session_tokens(&path, file_len).is_none());
    }

    #[test]
    fn test_scan_session_tokens_multiple_tools() {
        let dir = tempfile::tempdir().unwrap();
        let content = r#"{"type":"assistant","sessionId":"s1","cwd":"/proj","message":{"model":"claude-sonnet-4-20250514","content":[{"type":"tool_use","name":"Read","id":"t1"},{"type":"tool_use","name":"Bash","id":"t2"},{"type":"tool_use","name":"Read","id":"t3"}],"usage":{"input_tokens":100,"output_tokens":50},"stop_reason":"tool_use"},"timestamp":"2026-01-01T00:00:00Z"}
"#;
        let path = create_test_jsonl(dir.path(), content);

        let (summary, _) = scan_session_tokens(&path, 0).unwrap();
        assert_eq!(summary.tool_calls.len(), 2);
        // Sorted by count descending
        assert_eq!(summary.tool_calls[0].tool_name, "Read");
        assert_eq!(summary.tool_calls[0].call_count, 2);
        assert_eq!(summary.tool_calls[1].tool_name, "Bash");
        assert_eq!(summary.tool_calls[1].call_count, 1);
    }

    #[test]
    fn test_cache_stats_computation() {
        // Test the math directly
        let cache_read = 180u64;
        let cache_creation = 20u64;
        let total_input = 300u64;
        let uncached = total_input.saturating_sub(cache_creation + cache_read);
        let total = cache_read + cache_creation + uncached;
        let hit_rate = cache_read as f64 / total as f64;
        assert_eq!(uncached, 100);
        assert_eq!(total, 300);
        assert!((hit_rate - 0.6).abs() < 0.001);
    }

    #[test]
    fn test_tool_stats_sorting_and_truncation() {
        let mut tool_totals: HashMap<String, u64> = HashMap::new();
        for i in 0..20 {
            tool_totals.insert(format!("Tool{}", i), (20 - i) as u64);
        }

        let grand_total: u64 = tool_totals.values().sum();
        let mut stats: Vec<ToolStats> = tool_totals
            .into_iter()
            .map(|(tool_name, call_count)| ToolStats {
                tool_name,
                call_count,
                pct_of_total: (call_count as f64 / grand_total as f64) * 100.0,
            })
            .collect();
        stats.sort_by(|a, b| b.call_count.cmp(&a.call_count));
        stats.truncate(15);

        assert_eq!(stats.len(), 15);
        assert!(stats[0].call_count >= stats[1].call_count);
    }
}
