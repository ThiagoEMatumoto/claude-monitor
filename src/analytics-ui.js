import { $, invoke, escapeHtml } from "./utils.js";

function formatTokens(n) {
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
	if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
	return String(n);
}

export async function refreshAnalytics() {
	await Promise.all([
		refreshTokenSummary(),
		refreshCacheStats(),
		refreshToolStats(),
	]);
}

async function refreshTokenSummary() {
	const container = $("analytics-tokens");
	if (!container) return;

	try {
		const data = await invoke("get_session_analytics", { hours: 24 });
		const summary = $("token-summary");
		if (summary) {
			summary.innerHTML =
				`<span class="stat-pill">Today: ${formatTokens(data.totalInput)} in / ${formatTokens(data.totalOutput)} out</span>` +
				`<span class="stat-pill">${data.sessionCount} sessions, ${data.messageCount} messages</span>`;
		}

		const list = $("token-sessions-list");
		if (!list) return;

		if (!data.sessions || data.sessions.length === 0) {
			list.innerHTML = '<div class="empty-state"><span class="empty-text">No session data</span></div>';
			return;
		}

		list.innerHTML = data.sessions
			.slice(0, 15)
			.map((s) => {
				return `<div class="analytics-row">
					<div class="analytics-project">${escapeHtml(s.project || "unknown")}</div>
					<div class="analytics-model">${escapeHtml(s.model || "?")}</div>
					<div class="analytics-tokens">${formatTokens(s.totalInputTokens)} in / ${formatTokens(s.totalOutputTokens)} out</div>
				</div>`;
			})
			.join("");
	} catch (e) {
		console.warn("refreshTokenSummary failed:", e);
	}
}

async function refreshCacheStats() {
	const container = $("analytics-cache");
	if (!container) return;

	try {
		const data = await invoke("get_cache_stats", { hours: 24 });
		const pct = Math.round(data.hitRate * 100);
		const fill = $("bar-cache");
		const val = $("val-cache");

		if (fill) {
			fill.style.width = `${Math.min(pct, 100)}%`;
			fill.className = "progress-fill" + (pct >= 60 ? "" : pct >= 30 ? " warning" : " critical");
		}
		if (val) {
			val.textContent = `${pct}%`;
		}

		const detail = $("cache-detail");
		if (detail) {
			detail.textContent = `read: ${formatTokens(data.totalCacheRead)} | created: ${formatTokens(data.totalCacheCreation)} | uncached: ${formatTokens(data.totalUncachedInput)}`;
		}
	} catch (e) {
		console.warn("refreshCacheStats failed:", e);
	}
}

async function refreshToolStats() {
	const container = $("analytics-tools");
	if (!container) return;

	try {
		const data = await invoke("get_tool_stats", { hours: 24 });
		const list = $("tool-stats-list");
		if (!list) return;

		if (!data || data.length === 0) {
			list.innerHTML = '<div class="empty-state"><span class="empty-text">No tool usage data</span></div>';
			return;
		}

		const maxCount = data[0]?.callCount || 1;

		list.innerHTML = data
			.slice(0, 10)
			.map((t) => {
				const barWidth = Math.max((t.callCount / maxCount) * 100, 2);
				return `<div class="tool-row">
					<span class="tool-name">${escapeHtml(t.toolName)}</span>
					<div class="tool-bar-container">
						<div class="tool-bar-fill" style="width: ${barWidth}%"></div>
					</div>
					<span class="tool-count">${t.callCount} (${Math.round(t.pctOfTotal)}%)</span>
				</div>`;
			})
			.join("");
	} catch (e) {
		console.warn("refreshToolStats failed:", e);
	}
}
