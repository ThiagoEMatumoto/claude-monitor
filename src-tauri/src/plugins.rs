use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::analytics;

// === Data structures ===

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub marketplace: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub blocked: bool,
    pub install_path: String,
    pub installed_at: String,
    pub mcp_tools: Vec<String>,
    pub skills: Vec<String>,
    pub hooks: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUsageStats {
    pub plugin_id: String,
    pub total_calls_7d: u64,
    pub calls_by_tool: Vec<analytics::ToolCallRecord>,
    pub last_used: Option<String>,
    pub avg_calls_per_day: f64,
    pub health: PluginHealth,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginHealth {
    Active,
    Low,
    Unused,
    SkillOnly,
}

// === JSON file structures ===

#[derive(Deserialize)]
struct InstalledPluginsFile {
    plugins: HashMap<String, Vec<PluginInstallEntry>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginInstallEntry {
    install_path: String,
    version: String,
    installed_at: String,
}

#[derive(Deserialize)]
struct SettingsFile {
    #[serde(default, rename = "enabledPlugins")]
    enabled_plugins: HashMap<String, bool>,
}

#[derive(Deserialize)]
struct BlocklistFile {
    plugins: Vec<BlocklistEntry>,
}

#[derive(Deserialize)]
struct BlocklistEntry {
    plugin: String,
}

#[derive(Deserialize)]
struct PluginJson {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Deserialize)]
struct McpJson {
    #[serde(default, rename = "mcpServers")]
    mcp_servers: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct HooksJson {
    #[serde(default)]
    hooks: HashMap<String, serde_json::Value>,
}

// === Helper paths ===

fn claude_home() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".claude")
}

fn settings_path() -> PathBuf {
    claude_home().join("settings.json")
}

// === Public API ===

/// List all installed plugins with their metadata.
pub fn list_plugins() -> Vec<PluginInfo> {
    let claude = claude_home();

    // 1. Read installed_plugins.json
    let installed_path = claude.join("plugins/installed_plugins.json");
    let installed: InstalledPluginsFile = match read_json(&installed_path) {
        Some(v) => v,
        None => {
            warn!("could not read installed_plugins.json");
            return vec![];
        }
    };

    // 2. Read settings.json for enabled state
    let settings: Option<SettingsFile> = read_json(&settings_path());
    let enabled_map = settings
        .map(|s| s.enabled_plugins)
        .unwrap_or_default();

    // 3. Read blocklist
    let blocklist_path = claude.join("plugins/blocklist.json");
    let blocklist: Option<BlocklistFile> = read_json(&blocklist_path);
    let blocked_set: std::collections::HashSet<String> = blocklist
        .map(|b| b.plugins.into_iter().map(|e| e.plugin).collect())
        .unwrap_or_default();

    // 4. Build plugin info for each installed plugin
    let mut plugins = Vec::new();

    for (plugin_id, entries) in &installed.plugins {
        // Use the first (and usually only) entry
        let entry = match entries.first() {
            Some(e) => e,
            None => continue,
        };

        // Parse plugin_id: "name@marketplace"
        let (name, marketplace) = parse_plugin_id(plugin_id);

        let install_path = PathBuf::from(&entry.install_path);

        // Read plugin.json metadata
        let plugin_json_path = install_path.join(".claude-plugin/plugin.json");
        let meta: Option<PluginJson> = read_json(&plugin_json_path);
        let description = meta
            .as_ref()
            .map(|m| m.description.clone())
            .unwrap_or_default();

        // Read .mcp.json for tool names
        let mcp_tools = read_mcp_tools(&install_path);

        // Read skills
        let skills = read_skills(&install_path);

        // Read hooks
        let hooks = read_hooks(&install_path);

        let enabled = enabled_map.get(plugin_id).copied().unwrap_or(false);
        let blocked = blocked_set.contains(plugin_id);

        plugins.push(PluginInfo {
            id: plugin_id.clone(),
            name: meta.map(|m| m.name).unwrap_or_else(|| name.clone()),
            marketplace,
            version: entry.version.clone(),
            description,
            enabled,
            blocked,
            install_path: entry.install_path.clone(),
            installed_at: entry.installed_at.clone(),
            mcp_tools,
            skills,
            hooks,
        });
    }

    // Sort: enabled first, then alphabetically
    plugins.sort_by(|a, b| {
        b.enabled.cmp(&a.enabled).then_with(|| a.name.cmp(&b.name))
    });

    plugins
}

/// Get usage statistics for plugins based on tool call data from session JSONL files.
pub fn get_plugin_usage(hours: u64) -> Vec<PluginUsageStats> {
    let all_tools = analytics::get_tool_stats(hours);
    let sessions = analytics::get_session_analytics(hours);

    // Group tool calls by plugin
    let mut plugin_tools: HashMap<String, Vec<analytics::ToolCallRecord>> = HashMap::new();
    let mut plugin_last_seen: HashMap<String, String> = HashMap::new();

    // First pass: aggregate tool stats
    for tool in &all_tools {
        if let Some(plugin_name) = extract_plugin_from_tool(&tool.tool_name) {
            plugin_tools
                .entry(plugin_name.clone())
                .or_default()
                .push(analytics::ToolCallRecord {
                    tool_name: tool.tool_name.clone(),
                    call_count: tool.call_count,
                });
        }
    }

    // Second pass: find last_used timestamps from sessions
    for session in &sessions.sessions {
        for tool in &session.tool_calls {
            if let Some(plugin_name) = extract_plugin_from_tool(&tool.tool_name) {
                let ts = &session.last_seen;
                let entry = plugin_last_seen
                    .entry(plugin_name)
                    .or_insert_with(|| ts.clone());
                if ts > entry {
                    *entry = ts.clone();
                }
            }
        }
    }

    let days = (hours as f64 / 24.0).max(1.0);

    // Build stats for all known plugins (from tool data)
    let mut stats: Vec<PluginUsageStats> = plugin_tools
        .into_iter()
        .map(|(plugin_id, tools)| {
            let total: u64 = tools.iter().map(|t| t.call_count).sum();
            let avg = total as f64 / days;
            let health = if avg > 5.0 {
                PluginHealth::Active
            } else if avg >= 1.0 {
                PluginHealth::Low
            } else {
                PluginHealth::Unused
            };

            PluginUsageStats {
                plugin_id: plugin_id.clone(),
                total_calls_7d: total,
                calls_by_tool: tools,
                last_used: plugin_last_seen.get(&plugin_id).cloned(),
                avg_calls_per_day: avg,
                health,
            }
        })
        .collect();

    stats.sort_by(|a, b| b.total_calls_7d.cmp(&a.total_calls_7d));
    stats
}

/// Toggle a plugin's enabled state in settings.json.
pub fn set_plugin_enabled(plugin_id: &str, enabled: bool) -> Result<(), String> {
    let path = settings_path();

    // Read current settings as raw JSON to preserve all fields
    let content = fs::read_to_string(&path).map_err(|e| format!("read settings: {}", e))?;
    let mut settings: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("parse settings: {}", e))?;

    // Ensure enabledPlugins exists
    let obj = settings
        .as_object_mut()
        .ok_or("settings is not an object")?;

    let enabled_plugins = obj
        .entry("enabledPlugins")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    let ep_obj = enabled_plugins
        .as_object_mut()
        .ok_or("enabledPlugins is not an object")?;

    if enabled {
        ep_obj.insert(
            plugin_id.to_string(),
            serde_json::Value::Bool(true),
        );
    } else {
        ep_obj.remove(plugin_id);
    }

    // Atomic write: write to temp file then rename
    let tmp_path = path.with_extension("json.tmp");
    let json_str = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("serialize settings: {}", e))?;
    fs::write(&tmp_path, &json_str).map_err(|e| format!("write temp: {}", e))?;
    fs::rename(&tmp_path, &path).map_err(|e| format!("rename: {}", e))?;

    debug!("set plugin {} enabled={}", plugin_id, enabled);
    Ok(())
}

// === Internal helpers ===

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn parse_plugin_id(id: &str) -> (String, String) {
    match id.split_once('@') {
        Some((name, mkt)) => (name.to_string(), mkt.to_string()),
        None => (id.to_string(), "unknown".to_string()),
    }
}

fn read_mcp_tools(install_path: &Path) -> Vec<String> {
    let mcp_path = install_path.join(".mcp.json");
    let mcp: Option<McpJson> = read_json(&mcp_path);
    mcp.map(|m| m.mcp_servers.keys().cloned().collect())
        .unwrap_or_default()
}

fn read_skills(install_path: &Path) -> Vec<String> {
    let skills_dir = install_path.join("skills");
    if !skills_dir.is_dir() {
        return vec![];
    }
    match fs::read_dir(&skills_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect(),
        Err(_) => vec![],
    }
}

fn read_hooks(install_path: &Path) -> Vec<String> {
    let hooks_path = install_path.join("hooks/hooks.json");
    let hooks: Option<HooksJson> = read_json(&hooks_path);
    hooks
        .map(|h| h.hooks.keys().cloned().collect())
        .unwrap_or_default()
}

/// Extract plugin name from a tool call name.
/// Pattern: `mcp__plugin_{marketplace}_{server}__{tool}` → plugin name from installed_plugins
/// We match by extracting the marketplace-server pair and looking for it.
fn extract_plugin_from_tool(tool_name: &str) -> Option<String> {
    // Only match plugin tools: mcp__plugin_*
    let rest = tool_name.strip_prefix("mcp__plugin_")?;

    // Split on double underscore to separate server from tool
    // Pattern: {plugin-name}_{server-name}__{tool-name}
    // But plugin-name can contain hyphens and underscores...
    // Real examples:
    //   mcp__plugin_claude-mem_mcp-search__search → claude-mem
    //   mcp__plugin_compound-engineering_context7__query-docs → compound-engineering
    //
    // The pattern is: everything before the server name (which is separated by _ and followed by __)
    // Strategy: find the last `__` to split server+tool, then find the server name before that
    let double_under_pos = rest.find("__")?;
    let before_tool = &rest[..double_under_pos];

    // before_tool is like "claude-mem_mcp-search" or "compound-engineering_context7"
    // The plugin name is everything up to the last underscore-separated segment
    let last_underscore = before_tool.rfind('_')?;
    let plugin_name = &before_tool[..last_underscore];

    Some(plugin_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plugin_id() {
        let (name, mkt) = parse_plugin_id("claude-mem@thedotmack");
        assert_eq!(name, "claude-mem");
        assert_eq!(mkt, "thedotmack");
    }

    #[test]
    fn test_parse_plugin_id_no_at() {
        let (name, mkt) = parse_plugin_id("standalone-plugin");
        assert_eq!(name, "standalone-plugin");
        assert_eq!(mkt, "unknown");
    }

    #[test]
    fn test_extract_plugin_claude_mem() {
        let result = extract_plugin_from_tool("mcp__plugin_claude-mem_mcp-search__search");
        assert_eq!(result, Some("claude-mem".to_string()));
    }

    #[test]
    fn test_extract_plugin_compound_engineering() {
        let result =
            extract_plugin_from_tool("mcp__plugin_compound-engineering_context7__query-docs");
        assert_eq!(result, Some("compound-engineering".to_string()));
    }

    #[test]
    fn test_extract_plugin_not_a_plugin() {
        assert_eq!(extract_plugin_from_tool("mcp__context7__resolve-library-id"), None);
        assert_eq!(extract_plugin_from_tool("mcp__slack__slack_send_message"), None);
        assert_eq!(extract_plugin_from_tool("Read"), None);
        assert_eq!(extract_plugin_from_tool("Bash"), None);
    }

    #[test]
    fn test_extract_plugin_get_observations() {
        let result =
            extract_plugin_from_tool("mcp__plugin_claude-mem_mcp-search__get_observations");
        assert_eq!(result, Some("claude-mem".to_string()));
    }
}
