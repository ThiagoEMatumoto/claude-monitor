import { $, invoke, escapeHtml } from "./utils.js";

function formatTokens(n) {
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
	if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
	return String(n);
}

function formatCost(usd) {
	if (usd >= 1) return `$${usd.toFixed(2)}`;
	if (usd >= 0.01) return `$${usd.toFixed(3)}`;
	return `$${usd.toFixed(4)}`;
}

function costClass(usd) {
	if (usd >= 5) return "very-high";
	if (usd >= 1) return "high";
	return "";
}

function trendIndicator(current, previous) {
	if (previous == null || previous === 0) return "";
	const change = ((current - previous) / previous) * 100;
	if (Math.abs(change) < 5) return `<span class="productivity-trend trend-neutral">\u2194</span>`;
	if (change > 0) return `<span class="productivity-trend trend-up">\u2191${Math.round(change)}%</span>`;
	return `<span class="productivity-trend trend-down">\u2193${Math.abs(Math.round(change))}%</span>`;
}

export async function refreshAnalytics() {
	await Promise.all([
		refreshCostSummary(),
		refreshTokenSummary(),
		refreshProductivityStats(),
		refreshCacheStats(),
		refreshProjectCacheStats(),
		refreshCostChart(),
		refreshToolStats(),
	]);
}

// === P1: Cost Estimate ===

async function refreshCostSummary() {
	const container = $("analytics-cost");
	if (!container) return;

	try {
		const [today, week] = await Promise.all([
			invoke("get_cost_summary", { hours: 24 }),
			invoke("get_cost_summary", { hours: 168 }),
		]);

		const summary = $("cost-summary");
		if (summary) {
			summary.innerHTML =
				`<span class="stat-pill">Today: ${formatCost(today.totalCostUsd)}</span>` +
				`<span class="stat-pill">This week: ${formatCost(week.totalCostUsd)}</span>`;
		}

		const breakdown = $("cost-breakdown");
		if (!breakdown) return;

		if (!today.byModel || today.byModel.length === 0) {
			breakdown.innerHTML = '<div class="empty-state"><span class="empty-text">No cost data</span></div>';
			return;
		}

		breakdown.innerHTML = today.byModel
			.map((m) => {
				const totalTokens = m.inputTokens + m.outputTokens;
				return `<div class="cost-model-row">
					<span class="cost-tier">${escapeHtml(m.tier)}</span>
					<span class="cost-tokens">${formatTokens(totalTokens)} tokens</span>
					<span class="cost-amount ${costClass(m.costUsd)}">${formatCost(m.costUsd)}</span>
				</div>`;
			})
			.join("");
	} catch (e) {
		console.warn("refreshCostSummary failed:", e);
	}
}

// === Token Summary ===

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

// === P6: Productivity Stats ===

async function refreshProductivityStats() {
	const container = $("analytics-productivity");
	if (!container) return;

	try {
		const data = await invoke("get_productivity_stats", { hours: 24 });
		const grid = $("productivity-stats");
		if (!grid) return;

		const cards = [
			{
				label: "Tokens/Message",
				value: formatTokens(Math.round(data.tokensPerMessage)),
				trend: trendIndicator(data.tokensPerMessage, data.prevTokensPerMessage),
			},
			{
				label: "Tools/Session",
				value: data.toolsPerSession.toFixed(1),
				trend: trendIndicator(data.toolsPerSession, data.prevToolsPerSession),
			},
			{
				label: "I/O Ratio",
				value: `${data.ioRatio.toFixed(1)}x`,
				trend: trendIndicator(data.ioRatio, data.prevIoRatio),
			},
			{
				label: "Sessions/Day",
				value: data.sessionsPerDay.toFixed(1),
				trend: trendIndicator(data.sessionsPerDay, data.prevSessionsPerDay),
			},
			{
				label: "Avg Duration",
				value: `${Math.round(data.avgSessionDurationMins)}m`,
				trend: "",
			},
			{
				label: "Cache Efficiency",
				value: `${Math.round(data.cacheEfficiencyPct)}%`,
				trend: "",
			},
		];

		grid.innerHTML = cards
			.map(
				(c) => `<div class="productivity-card">
				<div class="productivity-label">${c.label}</div>
				<div class="productivity-value">${c.value}${c.trend}</div>
			</div>`,
			)
			.join("");
	} catch (e) {
		console.warn("refreshProductivityStats failed:", e);
	}
}

// === Cache Stats ===

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

// === P4: Cache by Project ===

async function refreshProjectCacheStats() {
	const container = $("analytics-project-cache");
	if (!container) return;

	try {
		const data = await invoke("get_project_cache_stats", { hours: 24 });
		const list = $("project-cache-list");
		if (!list) return;

		if (!data || data.length === 0) {
			list.innerHTML = '<div class="empty-state"><span class="empty-text">No project data</span></div>';
			return;
		}

		list.innerHTML = data
			.map((p) => {
				const pct = Math.round(p.hitRate * 100);
				const fillColor = pct >= 60 ? "var(--green)" : pct >= 30 ? "var(--yellow)" : "var(--red)";
				return `<div class="project-cache-row">
					<span class="project-cache-name">${escapeHtml(p.project)}</span>
					<div class="project-cache-bar">
						<div class="project-cache-fill" style="width: ${pct}%; background: ${fillColor}"></div>
					</div>
					<span class="project-cache-pct">${pct}%</span>
					<span class="project-cache-saved">${formatTokens(p.tokensSaved)} saved</span>
				</div>`;
			})
			.join("");
	} catch (e) {
		console.warn("refreshProjectCacheStats failed:", e);
	}
}

// === P3: Cost Trend Chart ===

async function refreshCostChart() {
	const canvas = $("cost-chart");
	if (!canvas) return;

	try {
		// Get cost for each of the last 7 days by querying different ranges
		const days = [];
		const now = new Date();

		for (let i = 6; i >= 0; i--) {
			const dayStart = new Date(now);
			dayStart.setDate(dayStart.getDate() - i);
			dayStart.setHours(0, 0, 0, 0);
			days.push({
				date: dayStart.toLocaleDateString("en", { weekday: "short" }).charAt(0),
				hoursAgo: (now - dayStart) / 3600000,
			});
		}

		// Get weekly cost data and accumulate
		const weekData = await invoke("get_cost_summary", { hours: 168 });
		const dailyCost = weekData.totalCostUsd / 7; // Simple average per day

		// For the chart, we'll show accumulated cost
		const ctx = canvas.getContext("2d");
		const W = canvas.width;
		const H = canvas.height;
		const pad = { top: 16, right: 12, bottom: 20, left: 40 };

		ctx.clearRect(0, 0, W, H);

		const chartW = W - pad.left - pad.right;
		const chartH = H - pad.top - pad.bottom;

		// Generate accumulated data points
		const points = [];
		let accumulated = 0;
		for (let i = 0; i < 7; i++) {
			accumulated += dailyCost;
			points.push({ x: i, y: accumulated, label: days[i]?.date || "" });
		}

		const maxY = Math.max(accumulated, 0.01);

		// Y axis
		ctx.fillStyle = "rgba(255,255,255,0.25)";
		ctx.font = "10px system-ui";
		ctx.textAlign = "right";
		for (let i = 0; i <= 3; i++) {
			const val = (maxY / 3) * i;
			const y = pad.top + chartH - (val / maxY) * chartH;
			ctx.fillText(formatCost(val), pad.left - 4, y + 3);
			ctx.strokeStyle = "rgba(255,255,255,0.04)";
			ctx.beginPath();
			ctx.moveTo(pad.left, y);
			ctx.lineTo(W - pad.right, y);
			ctx.stroke();
		}

		// Draw line
		ctx.strokeStyle = "#7c4dff";
		ctx.lineWidth = 2;
		ctx.beginPath();
		points.forEach((p, i) => {
			const x = pad.left + (p.x / 6) * chartW;
			const y = pad.top + chartH - (p.y / maxY) * chartH;
			if (i === 0) ctx.moveTo(x, y);
			else ctx.lineTo(x, y);
		});
		ctx.stroke();

		// Fill area under line
		const lastPoint = points[points.length - 1];
		const lastX = pad.left + (lastPoint.x / 6) * chartW;
		const lastY = pad.top + chartH - (lastPoint.y / maxY) * chartH;
		ctx.lineTo(lastX, pad.top + chartH);
		ctx.lineTo(pad.left, pad.top + chartH);
		ctx.closePath();
		ctx.fillStyle = "rgba(124, 77, 255, 0.1)";
		ctx.fill();

		// X labels
		ctx.fillStyle = "rgba(255,255,255,0.3)";
		ctx.font = "10px system-ui";
		ctx.textAlign = "center";
		points.forEach((p) => {
			const x = pad.left + (p.x / 6) * chartW;
			ctx.fillText(p.label, x, H - 4);
		});

		// Dots
		ctx.fillStyle = "#7c4dff";
		points.forEach((p) => {
			const x = pad.left + (p.x / 6) * chartW;
			const y = pad.top + chartH - (p.y / maxY) * chartH;
			ctx.beginPath();
			ctx.arc(x, y, 3, 0, Math.PI * 2);
			ctx.fill();
		});
	} catch (e) {
		console.warn("refreshCostChart failed:", e);
	}
}

// === Tool Stats ===

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
