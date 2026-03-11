import { $, invoke, escapeHtml } from "./utils.js";

// === Formatting utilities ===

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

// === State ===

const insightsCache = { data: null, timestamp: 0, TTL: 60000 };
const detailCache = {};

// === Exported functions ===

export async function refreshInsights() {
	const now = Date.now();
	if (insightsCache.data && now - insightsCache.timestamp < insightsCache.TTL) {
		renderSummaryCards(insightsCache.data);
		return;
	}

	// Show skeleton while loading
	showSkeleton();

	try {
		const data = await invoke("get_insights_summary");
		insightsCache.data = data;
		insightsCache.timestamp = Date.now();
		renderSummaryCards(data);
	} catch (e) {
		console.warn("refreshInsights failed:", e);
	}
}

function showSkeleton() {
	const ids = ["insight-cost", "insight-cache", "insight-tokens", "insight-tool"];
	for (const id of ids) {
		const el = $(id);
		if (el && el.textContent === "\u2014") {
			el.innerHTML = '<span class="skeleton" style="width: 48px; display: inline-block;">&nbsp;</span>';
		}
	}
}

function renderSummaryCards(data) {
	const cost = $("insight-cost");
	const costSub = $("insight-cost-sub");
	if (cost) cost.textContent = formatCost(data.costTodayUsd);
	if (costSub) costSub.textContent = `Week: ${formatCost(data.costWeekUsd)}`;

	const cache = $("insight-cache");
	const cacheSub = $("insight-cache-sub");
	if (cache) cache.textContent = `${Math.round(data.cacheHitRate * 100)}%`;
	if (cacheSub) {
		cacheSub.textContent = data.topCacheProject
			? `Best: ${data.topCacheProject} (${Math.round(data.topCacheProjectRate * 100)}%)`
			: "No project data";
	}

	const tokens = $("insight-tokens");
	const tokensSub = $("insight-tokens-sub");
	if (tokens) tokens.textContent = formatTokens(data.totalInput + data.totalOutput);
	if (tokensSub) tokensSub.textContent = `${data.sessionCount} sessions, ${data.messageCount} msgs`;

	const tool = $("insight-tool");
	const toolSub = $("insight-tool-sub");
	if (tool) tool.textContent = data.topToolName || "\u2014";
	if (toolSub) {
		toolSub.textContent = data.topToolName
			? `${Math.round(data.topToolPct)}%${data.secondToolName ? ` \u2022 ${data.secondToolName} ${Math.round(data.secondToolPct)}%` : ""}`
			: "No tool data";
	}
}

export function showInsightDetail(type) {
	const summary = $("insights-summary");
	const detail = $("insight-detail");
	if (summary) summary.classList.add("hidden");
	if (detail) detail.classList.remove("hidden");

	const content = $("insight-detail-content");
	if (content) content.innerHTML = '<div class="skeleton" style="width: 100%; height: 80px;"></div>';

	switch (type) {
		case "cost": renderCostDetail(); break;
		case "cache": renderCacheDetail(); break;
		case "tokens": renderTokensDetail(); break;
		case "tools": renderToolsDetail(); break;
	}
}

export function hideInsightDetail() {
	const summary = $("insights-summary");
	const detail = $("insight-detail");
	if (summary) summary.classList.remove("hidden");
	if (detail) detail.classList.add("hidden");
}

// === Detail renderers ===

async function renderCostDetail() {
	const content = $("insight-detail-content");
	if (!content) return;

	try {
		const cacheKey = "cost";
		const cached = detailCache[cacheKey];
		let today, week;

		if (cached && Date.now() - cached.ts < insightsCache.TTL) {
			today = cached.today;
			week = cached.week;
		} else {
			[today, week] = await Promise.all([
				invoke("get_cost_summary", { hours: 24 }),
				invoke("get_cost_summary", { hours: 168 }),
			]);
			detailCache[cacheKey] = { today, week, ts: Date.now() };
		}

		const fragment = document.createDocumentFragment();
		const wrapper = document.createElement("div");

		let html = `<div class="section-title">Cost Estimate (24h)</div>`;
		html += `<div class="stat-pills">
			<span class="stat-pill">Today: ${formatCost(today.totalCostUsd)}</span>
			<span class="stat-pill">This week: ${formatCost(week.totalCostUsd)}</span>
		</div>`;

		if (today.byModel && today.byModel.length > 0) {
			html += today.byModel
				.map((m) => {
					const totalTokens = m.inputTokens + m.outputTokens;
					return `<div class="cost-model-row">
						<span class="cost-tier">${escapeHtml(m.tier)}</span>
						<span class="cost-tokens">${formatTokens(totalTokens)} tokens</span>
						<span class="cost-amount ${costClass(m.costUsd)}">${formatCost(m.costUsd)}</span>
					</div>`;
				})
				.join("");
		} else {
			html += '<div class="empty-state"><span class="empty-text">No cost data</span></div>';
		}

		// Cost trend chart
		html += `<div class="divider"></div>
			<div class="section-title">Cost Trend (7d)</div>
			<canvas id="cost-chart-detail" width="300" height="100"></canvas>`;

		wrapper.innerHTML = html;
		fragment.appendChild(wrapper);

		requestAnimationFrame(() => {
			content.innerHTML = "";
			content.appendChild(fragment);
			renderCostChart("cost-chart-detail", week);
		});
	} catch (e) {
		content.innerHTML = '<div class="empty-state"><span class="empty-text">Failed to load cost data</span></div>';
	}
}

function renderCostChart(canvasId, weekData) {
	const canvas = $(canvasId);
	if (!canvas) return;

	const dailyCost = weekData.totalCostUsd / 7;
	const ctx = canvas.getContext("2d");
	const W = canvas.width;
	const H = canvas.height;
	const pad = { top: 16, right: 12, bottom: 20, left: 40 };

	ctx.clearRect(0, 0, W, H);

	const chartW = W - pad.left - pad.right;
	const chartH = H - pad.top - pad.bottom;

	const now = new Date();
	const days = [];
	for (let i = 6; i >= 0; i--) {
		const d = new Date(now);
		d.setDate(d.getDate() - i);
		days.push(d.toLocaleDateString("en", { weekday: "short" }).charAt(0));
	}

	const points = [];
	let accumulated = 0;
	for (let i = 0; i < 7; i++) {
		accumulated += dailyCost;
		points.push({ x: i, y: accumulated, label: days[i] || "" });
	}

	const maxY = Math.max(accumulated, 0.01);

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

	const lastPoint = points[points.length - 1];
	const lastX = pad.left + (lastPoint.x / 6) * chartW;
	ctx.lineTo(lastX, pad.top + chartH);
	ctx.lineTo(pad.left, pad.top + chartH);
	ctx.closePath();
	ctx.fillStyle = "rgba(124, 77, 255, 0.1)";
	ctx.fill();

	ctx.fillStyle = "rgba(255,255,255,0.3)";
	ctx.font = "10px system-ui";
	ctx.textAlign = "center";
	points.forEach((p) => {
		const x = pad.left + (p.x / 6) * chartW;
		ctx.fillText(p.label, x, H - 4);
	});

	ctx.fillStyle = "#7c4dff";
	points.forEach((p) => {
		const x = pad.left + (p.x / 6) * chartW;
		const y = pad.top + chartH - (p.y / maxY) * chartH;
		ctx.beginPath();
		ctx.arc(x, y, 3, 0, Math.PI * 2);
		ctx.fill();
	});
}

async function renderCacheDetail() {
	const content = $("insight-detail-content");
	if (!content) return;

	try {
		const [cacheStats, projectStats] = await Promise.all([
			invoke("get_cache_stats", { hours: 24 }),
			invoke("get_project_cache_stats", { hours: 24 }),
		]);

		const pct = Math.round(cacheStats.hitRate * 100);
		const fillClass = pct >= 60 ? "" : pct >= 30 ? " warning" : " critical";

		let html = `<div class="section-title">Cache Efficiency</div>`;
		html += `<div class="usage-row compact">
			<span class="label-text">Hit Rate</span>
			<div class="progress-bar small">
				<div class="progress-fill${fillClass}" style="width: ${Math.min(pct, 100)}%"></div>
			</div>
			<span class="label-value">${pct}%</span>
		</div>`;
		html += `<div class="usage-meta">read: ${formatTokens(cacheStats.totalCacheRead)} | created: ${formatTokens(cacheStats.totalCacheCreation)} | uncached: ${formatTokens(cacheStats.totalUncachedInput)}</div>`;

		if (projectStats && projectStats.length > 0) {
			html += `<div class="divider"></div><div class="section-title">Cache by Project</div>`;
			html += projectStats
				.map((p) => {
					const pctP = Math.round(p.hitRate * 100);
					const fillColor = pctP >= 60 ? "var(--green)" : pctP >= 30 ? "var(--yellow)" : "var(--red)";
					return `<div class="project-cache-row">
						<span class="project-cache-name">${escapeHtml(p.project)}</span>
						<div class="project-cache-bar">
							<div class="project-cache-fill" style="width: ${pctP}%; background: ${fillColor}"></div>
						</div>
						<span class="project-cache-pct">${pctP}%</span>
						<span class="project-cache-saved">${formatTokens(p.tokensSaved)} saved</span>
					</div>`;
				})
				.join("");
		}

		requestAnimationFrame(() => { content.innerHTML = html; });
	} catch (e) {
		content.innerHTML = '<div class="empty-state"><span class="empty-text">Failed to load cache data</span></div>';
	}
}

async function renderTokensDetail() {
	const content = $("insight-detail-content");
	if (!content) return;

	try {
		const [data, prodStats] = await Promise.all([
			invoke("get_session_analytics", { hours: 24 }),
			invoke("get_productivity_stats", { hours: 24 }),
		]);

		let html = `<div class="section-title">Token Usage (24h)</div>`;
		html += `<div class="stat-pills">
			<span class="stat-pill">${formatTokens(data.totalInput)} in / ${formatTokens(data.totalOutput)} out</span>
			<span class="stat-pill">${data.sessionCount} sessions, ${data.messageCount} messages</span>
		</div>`;

		// Productivity grid
		html += `<div class="divider"></div><div class="section-title">Productivity</div>`;
		const cards = [
			{ label: "Tokens/Message", value: formatTokens(Math.round(prodStats.tokensPerMessage)), trend: trendIndicator(prodStats.tokensPerMessage, prodStats.prevTokensPerMessage) },
			{ label: "Tools/Session", value: prodStats.toolsPerSession.toFixed(1), trend: trendIndicator(prodStats.toolsPerSession, prodStats.prevToolsPerSession) },
			{ label: "I/O Ratio", value: `${prodStats.ioRatio.toFixed(1)}x`, trend: trendIndicator(prodStats.ioRatio, prodStats.prevIoRatio) },
			{ label: "Sessions/Day", value: prodStats.sessionsPerDay.toFixed(1), trend: trendIndicator(prodStats.sessionsPerDay, prodStats.prevSessionsPerDay) },
			{ label: "Avg Duration", value: `${Math.round(prodStats.avgSessionDurationMins)}m`, trend: "" },
			{ label: "Cache Efficiency", value: `${Math.round(prodStats.cacheEfficiencyPct)}%`, trend: "" },
		];
		html += `<div class="productivity-grid">${cards.map((c) => `<div class="productivity-card">
			<div class="productivity-label">${c.label}</div>
			<div class="productivity-value">${c.value}${c.trend}</div>
		</div>`).join("")}</div>`;

		// Top sessions
		if (data.sessions && data.sessions.length > 0) {
			html += `<div class="divider"></div><div class="section-title">Top Sessions</div>`;
			html += `<div class="analytics-list">${data.sessions
				.slice(0, 15)
				.map((s) => `<div class="analytics-row">
					<div class="analytics-project">${escapeHtml(s.project || "unknown")}</div>
					<div class="analytics-model">${escapeHtml(s.model || "?")}</div>
					<div class="analytics-tokens">${formatTokens(s.totalInputTokens)} in / ${formatTokens(s.totalOutputTokens)} out</div>
				</div>`)
				.join("")}</div>`;
		}

		requestAnimationFrame(() => { content.innerHTML = html; });
	} catch (e) {
		content.innerHTML = '<div class="empty-state"><span class="empty-text">Failed to load token data</span></div>';
	}
}

async function renderToolsDetail() {
	const content = $("insight-detail-content");
	if (!content) return;

	try {
		const data = await invoke("get_tool_stats", { hours: 24 });

		let html = `<div class="section-title">Top Tools (24h)</div>`;

		if (!data || data.length === 0) {
			html += '<div class="empty-state"><span class="empty-text">No tool usage data</span></div>';
		} else {
			const maxCount = data[0]?.callCount || 1;
			html += data
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
		}

		requestAnimationFrame(() => { content.innerHTML = html; });
	} catch (e) {
		content.innerHTML = '<div class="empty-state"><span class="empty-text">Failed to load tool data</span></div>';
	}
}

// === Event setup ===

export function setupInsightEvents() {
	const grid = $("insights-summary");
	if (grid) {
		grid.addEventListener("click", (e) => {
			const card = e.target.closest(".insight-card");
			if (!card) return;
			const type = card.dataset.detail;
			if (type) showInsightDetail(type);
		});
	}

	const backBtn = $("insight-back");
	if (backBtn) {
		backBtn.addEventListener("click", hideInsightDetail);
	}
}
