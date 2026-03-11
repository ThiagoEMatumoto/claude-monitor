import { $, invoke, escapeHtml } from "./utils.js";

// === State ===

const pluginsCache = { data: null, usage: null, timestamp: 0, TTL: 120000 };
let detailPluginId = null;

// === Exported functions ===

export async function refreshPlugins() {
	const now = Date.now();
	if (pluginsCache.data && now - pluginsCache.timestamp < pluginsCache.TTL) {
		renderPluginsList(pluginsCache.data, pluginsCache.usage);
		return;
	}

	showPluginsSkeleton();

	try {
		const [plugins, usage] = await Promise.all([
			invoke("list_plugins"),
			invoke("get_plugin_usage", { hours: 168 }),
		]);
		pluginsCache.data = plugins;
		pluginsCache.usage = usage;
		pluginsCache.timestamp = Date.now();
		renderPluginsList(plugins, usage);
	} catch (e) {
		console.warn("refreshPlugins failed:", e);
		const list = $("plugins-list");
		if (list) {
			list.innerHTML =
				'<div class="empty-state"><span class="empty-text">Failed to load plugins</span></div>';
		}
	}
}

export function setupPluginEvents() {
	const list = $("plugins-list");
	if (list) {
		list.addEventListener("click", (e) => {
			// Handle toggle click
			const toggle = e.target.closest(".plugin-toggle");
			if (toggle) {
				e.stopPropagation();
				const id = toggle.dataset.pluginId;
				const enabled = toggle.classList.contains("active");
				togglePlugin(id, !enabled);
				return;
			}

			// Handle item click → detail view
			const item = e.target.closest(".plugin-item");
			if (item) {
				showPluginDetail(item.dataset.pluginId);
			}
		});
	}

	const backBtn = $("plugin-back");
	if (backBtn) {
		backBtn.addEventListener("click", hidePluginDetail);
	}
}

// === Internal functions ===

function showPluginsSkeleton() {
	const list = $("plugins-list");
	if (!list) return;
	list.innerHTML = Array(3)
		.fill(
			'<div class="plugin-item"><div class="skeleton" style="width:100%;height:40px;"></div></div>',
		)
		.join("");
}

function renderPluginsList(plugins, usage) {
	const list = $("plugins-list");
	const summary = $("plugins-summary");
	if (!list) return;

	// Build usage map: plugin name → stats
	const usageMap = {};
	if (usage) {
		for (const u of usage) {
			usageMap[u.pluginId] = u;
		}
	}

	if (!plugins || plugins.length === 0) {
		list.innerHTML =
			'<div class="empty-state"><span class="empty-text">No plugins installed</span></div>';
		if (summary) summary.innerHTML = "";
		return;
	}

	const html = plugins
		.map((p) => {
			const stats = usageMap[p.name];
			const health = getHealth(p, stats);
			const dotClass = `plugin-dot ${health}`;
			const calls = stats ? stats.totalCalls7d : 0;
			let metaLine = `${escapeHtml(p.version)}`;
			if (p.marketplace) metaLine += ` · ${escapeHtml(p.marketplace)}`;

			let statsLine = "";
			if (p.mcpTools.length > 0) {
				statsLine = `${p.mcpTools.length} tool${p.mcpTools.length > 1 ? "s" : ""} · ${calls} calls/7d`;
			} else if (p.skills.length > 0) {
				statsLine = `${p.skills.length} skill${p.skills.length > 1 ? "s" : ""} only`;
			} else {
				statsLine = "no tools or skills";
			}

			const toggleClass = p.enabled ? "plugin-toggle active" : "plugin-toggle";
			const toggleLabel = p.enabled ? "ON" : "OFF";

			return `<div class="plugin-item" data-plugin-id="${escapeHtml(p.id)}">
				<div class="${dotClass}"></div>
				<div class="plugin-info">
					<div class="plugin-header">
						<span class="plugin-name">${escapeHtml(p.name)}</span>
						<button class="${toggleClass}" data-plugin-id="${escapeHtml(p.id)}">${toggleLabel}</button>
					</div>
					<div class="plugin-meta">${metaLine}</div>
					<div class="plugin-stats">${statsLine}</div>
				</div>
			</div>`;
		})
		.join("");

	requestAnimationFrame(() => {
		list.innerHTML = html;
	});

	// Summary footer
	if (summary) {
		const activeCount = plugins.filter((p) => p.enabled).length;
		const totalCount = plugins.length;
		const mcpCount = plugins.filter(
			(p) => p.enabled && p.mcpTools.length > 0,
		).length;
		summary.innerHTML = `<span>Active: ${activeCount}/${totalCount} plugins</span>
			<span>${mcpCount} with MCP tools</span>`;
	}
}

function getHealth(plugin, stats) {
	if (plugin.mcpTools.length === 0) return "skill-only";
	if (!stats || stats.totalCalls7d === 0) return "unused";
	if (stats.avgCallsPerDay > 5) return "active";
	return "low";
}

function showPluginDetail(pluginId) {
	detailPluginId = pluginId;
	const listContainer = $("plugins-list");
	const summaryEl = $("plugins-summary");
	const detail = $("plugin-detail");
	if (listContainer) listContainer.classList.add("hidden");
	if (summaryEl) summaryEl.classList.add("hidden");
	if (detail) detail.classList.remove("hidden");

	const plugin = pluginsCache.data?.find((p) => p.id === pluginId);
	if (!plugin) return;

	const usageMap = {};
	if (pluginsCache.usage) {
		for (const u of pluginsCache.usage) {
			usageMap[u.pluginId] = u;
		}
	}
	const stats = usageMap[plugin.name];

	renderPluginDetailContent(plugin, stats);
}

function hidePluginDetail() {
	detailPluginId = null;
	const listContainer = $("plugins-list");
	const summaryEl = $("plugins-summary");
	const detail = $("plugin-detail");
	if (listContainer) listContainer.classList.remove("hidden");
	if (summaryEl) summaryEl.classList.remove("hidden");
	if (detail) detail.classList.add("hidden");
}

function renderPluginDetailContent(plugin, stats) {
	const content = $("plugin-detail-content");
	if (!content) return;

	const toggleClass = plugin.enabled ? "plugin-toggle active" : "plugin-toggle";
	const toggleLabel = plugin.enabled ? "Enabled" : "Disabled";

	let html = `
		<div class="plugin-detail-header">
			<span class="plugin-detail-name">${escapeHtml(plugin.name)}</span>
			<button class="${toggleClass}" data-plugin-id="${escapeHtml(plugin.id)}" id="detail-toggle">${toggleLabel}</button>
		</div>
		<div class="plugin-detail-desc">${escapeHtml(plugin.description)}</div>
		<div class="plugin-meta">v${escapeHtml(plugin.version)} · ${escapeHtml(plugin.marketplace)}</div>
	`;

	// MCP Tools breakdown
	if (plugin.mcpTools.length > 0) {
		html += '<div class="divider"></div><div class="section-title">MCP Servers</div>';
		html += plugin.mcpTools
			.map((t) => {
				const toolCalls = getToolCallsForServer(plugin.name, t, stats);
				return `<div class="tool-row">
					<span class="tool-name">${escapeHtml(t)}</span>
					<span class="tool-count">${toolCalls} calls</span>
				</div>`;
			})
			.join("");
	}

	// Tool call breakdown
	if (stats && stats.callsByTool && stats.callsByTool.length > 0) {
		html += '<div class="divider"></div><div class="section-title">Tool Calls (7d)</div>';
		const maxCount = stats.callsByTool[0]?.callCount || 1;
		html += stats.callsByTool
			.map((t) => {
				// Show just the tool part after __
				const shortName = t.toolName.split("__").pop() || t.toolName;
				const barWidth = Math.max((t.callCount / maxCount) * 100, 2);
				return `<div class="tool-row">
					<span class="tool-name">${escapeHtml(shortName)}</span>
					<div class="tool-bar-container">
						<div class="tool-bar-fill" style="width: ${barWidth}%"></div>
					</div>
					<span class="tool-count">${t.callCount}</span>
				</div>`;
			})
			.join("");
	}

	// Skills
	if (plugin.skills.length > 0) {
		html += '<div class="divider"></div><div class="section-title">Skills</div>';
		html += `<div class="stat-pills">${plugin.skills.map((s) => `<span class="stat-pill">${escapeHtml(s)}</span>`).join("")}</div>`;
	}

	// Hooks
	if (plugin.hooks.length > 0) {
		html += '<div class="divider"></div><div class="section-title">Hooks</div>';
		html += `<div class="stat-pills">${plugin.hooks.map((h) => `<span class="stat-pill">${escapeHtml(h)}</span>`).join("")}</div>`;
	}

	// Usage summary
	if (stats) {
		html += '<div class="divider"></div><div class="section-title">Usage (7d)</div>';
		const barPct = Math.min((stats.totalCalls7d / Math.max(stats.totalCalls7d, 100)) * 100, 100);
		html += `<div class="usage-row compact">
			<span class="label-text">${stats.totalCalls7d} calls</span>
			<div class="progress-bar small">
				<div class="progress-fill" style="width: ${barPct}%"></div>
			</div>
		</div>`;
		if (stats.lastUsed) {
			html += `<div class="plugin-meta">Last used: ${formatTimeAgo(stats.lastUsed)}</div>`;
		}
		html += `<div class="plugin-meta">Avg: ${stats.avgCallsPerDay.toFixed(1)} calls/day</div>`;
	}

	// Warning about toggle
	html +=
		'<div class="plugin-notice">Changes take effect on next Claude Code session</div>';

	requestAnimationFrame(() => {
		content.innerHTML = html;

		// Bind toggle in detail view
		const detailToggle = document.getElementById("detail-toggle");
		if (detailToggle) {
			detailToggle.addEventListener("click", () => {
				const id = detailToggle.dataset.pluginId;
				const enabled = detailToggle.classList.contains("active");
				togglePlugin(id, !enabled);
			});
		}
	});
}

function getToolCallsForServer(pluginName, serverName, stats) {
	if (!stats || !stats.callsByTool) return 0;
	// Match tools that contain the server name pattern
	return stats.callsByTool
		.filter((t) => {
			const prefix = `mcp__plugin_${pluginName}_${serverName}__`;
			return t.toolName.startsWith(prefix);
		})
		.reduce((sum, t) => sum + t.callCount, 0);
}

async function togglePlugin(pluginId, enabled) {
	try {
		await invoke("set_plugin_enabled", { pluginId, enabled });

		// Update local cache
		if (pluginsCache.data) {
			const plugin = pluginsCache.data.find((p) => p.id === pluginId);
			if (plugin) plugin.enabled = enabled;
		}

		// Re-render current view
		if (detailPluginId === pluginId) {
			const plugin = pluginsCache.data?.find((p) => p.id === pluginId);
			if (plugin) {
				const usageMap = {};
				if (pluginsCache.usage) {
					for (const u of pluginsCache.usage) {
						usageMap[u.pluginId] = u;
					}
				}
				renderPluginDetailContent(plugin, usageMap[plugin.name]);
			}
		} else {
			renderPluginsList(pluginsCache.data, pluginsCache.usage);
		}
	} catch (e) {
		console.error("togglePlugin failed:", e);
	}
}

function formatTimeAgo(isoStr) {
	if (!isoStr) return "never";
	const diff = Date.now() - new Date(isoStr).getTime();
	const mins = Math.floor(diff / 60000);
	if (mins < 1) return "just now";
	if (mins < 60) return `${mins}m ago`;
	const hours = Math.floor(mins / 60);
	if (hours < 24) return `${hours}h ago`;
	const days = Math.floor(hours / 24);
	return `${days}d ago`;
}
