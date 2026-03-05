const { invoke } = window.__TAURI__.core;

export { invoke };

export const $ = (id) => document.getElementById(id);

export function formatTimeUntil(isoStr) {
	const diff = new Date(isoStr) - new Date();
	if (diff <= 0) return "now";
	const h = Math.floor(diff / 3600000);
	const m = Math.floor((diff % 3600000) / 60000);
	if (h >= 24) {
		const d = Math.floor(h / 24);
		return `${d}d ${h % 24}h`;
	}
	return `${h}h ${m}m`;
}

export function formatElapsed(isoStr) {
	if (!isoStr) return "";
	const diff = Date.now() - new Date(isoStr).getTime();
	if (diff < 0) return "now";
	const secs = Math.floor(diff / 1000);
	if (secs < 60) return `${secs}s`;
	const mins = Math.floor(secs / 60);
	if (mins < 60) return `${mins}m`;
	const hours = Math.floor(mins / 60);
	return `${hours}h ${mins % 60}m`;
}

export function escapeHtml(str) {
	const div = document.createElement("div");
	div.textContent = str;
	return div.innerHTML;
}

export function escapeAttr(str) {
	return String(str)
		.replace(/&/g, "&amp;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&#39;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;");
}

export function getLevel(v) {
	if (v >= 75) return "critical";
	if (v >= 50) return "warning";
	return "";
}

export function debounce(fn, ms) {
	let timer;
	return (...args) => {
		clearTimeout(timer);
		timer = setTimeout(() => fn(...args), ms);
	};
}

export function linearRegression(points) {
	const n = points.length;
	if (n < 2) return { slope: 0, intercept: 0 };

	let sumX = 0, sumY = 0, sumXY = 0, sumX2 = 0;
	for (const { x, y } of points) {
		sumX += x;
		sumY += y;
		sumXY += x * y;
		sumX2 += x * x;
	}

	const denom = n * sumX2 - sumX * sumX;
	if (denom === 0) return { slope: 0, intercept: sumY / n };

	const slope = (n * sumXY - sumX * sumY) / denom;
	const intercept = (sumY - slope * sumX) / n;
	return { slope, intercept };
}

let db = null;

export async function getDb() {
	if (!db) {
		const Database = (await import("@tauri-apps/plugin-sql")).default;
		db = await Database.load("sqlite:claude-monitor.db");
	}
	return db;
}

export async function invokeWithRetry(
	cmd,
	args,
	{ retries = 3, baseDelay = 2000 } = {},
) {
	for (let attempt = 0; attempt <= retries; attempt++) {
		try {
			return await invoke(cmd, args);
		} catch (e) {
			const err = String(e);
			const isRateLimit = err.startsWith("429:");
			if (!isRateLimit || attempt === retries) throw e;
			const delay = baseDelay * Math.pow(2, attempt);
			console.warn(
				`${cmd} rate limited, retrying in ${delay}ms (attempt ${attempt + 1}/${retries})`,
			);
			await new Promise((r) => setTimeout(r, delay));
		}
	}
}
