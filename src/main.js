const { invoke } = window.__TAURI__.core;

// === State ===
let pollTimer = null;
let settingsOpen = false;

// === DOM References ===
const $ = (id) => document.getElementById(id);

// === Initialization ===
async function init() {
	const authenticated = await invoke("is_authenticated");
	if (authenticated) {
		showUsage();
		await refresh();
		startPolling();
	} else {
		showLogin();
	}
	bindEvents();
}

// === Views ===
function showLogin() {
	$("login-screen").classList.remove("hidden");
	$("usage-section").classList.add("hidden");
	$("settings-section").classList.add("hidden");
}

function showUsage() {
	$("login-screen").classList.add("hidden");
	$("usage-section").classList.remove("hidden");
	$("settings-section").classList.add("hidden");
	settingsOpen = false;
}

function toggleSettings() {
	settingsOpen = !settingsOpen;
	$("settings-section").classList.toggle("hidden", !settingsOpen);
	$("usage-section").classList.toggle("hidden", settingsOpen);
}

// === Data Refresh ===
async function refresh() {
	try {
		const data = await invoke("get_usage");
		updateBars(data);
		await saveSnapshot(data);
		await renderChart();
		checkAlerts(data);
	} catch (e) {
		console.error("refresh failed:", e);
		if (String(e).includes("not authenticated")) {
			showLogin();
		}
	}
}

function updateBars(data) {
	setBar("5h", data.fiveHour, data.fiveHourResetsAt);
	setBar("7d", data.sevenDay, data.sevenDayResetsAt);
	setModelBar("sonnet", data.sonnet);
	setModelBar("opus", data.opus);
}

function setBar(id, value, resetsAt) {
	const pct = value ?? 0;
	const fill = $(`bar-${id}`);
	const val = $(`val-${id}`);
	const reset = $(`reset-${id}`);

	fill.style.width = `${Math.min(pct, 100)}%`;
	fill.className = "progress-fill " + getLevel(pct);
	val.textContent = `${Math.round(pct)}%`;

	if (resetsAt) {
		reset.textContent = `Resets in ${formatTimeUntil(resetsAt)}`;
	}
}

function setModelBar(model, value) {
	const pct = value ?? 0;
	const fill = $(`bar-${model}`);
	const val = $(`val-${model}`);
	fill.style.width = `${Math.min(pct, 100)}%`;
	fill.className = "progress-fill " + getLevel(pct);
	val.textContent = `${Math.round(pct)}%`;
}

function getLevel(v) {
	if (v >= 75) return "critical";
	if (v >= 50) return "warning";
	return "";
}

function formatTimeUntil(isoStr) {
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

// === SQLite History ===
let db = null;

async function getDb() {
	if (!db) {
		const Database = (await import("@tauri-apps/plugin-sql")).default;
		db = await Database.load("sqlite:claude-monitor.db");
	}
	return db;
}

async function saveSnapshot(data) {
	try {
		const d = await getDb();
		const fiveHourPct = data.fiveHour ?? 0;
		const sevenDayPct = data.sevenDay ?? 0;
		const sonnetPct = data.sonnet ?? 0;
		const opusPct = data.opus ?? 0;
		await d.execute(
			"INSERT INTO snapshots (five_hour, seven_day, sonnet, opus) VALUES ($1, $2, $3, $4)",
			[fiveHourPct, sevenDayPct, sonnetPct, opusPct],
		);
		// Cleanup old data (keep 30 days)
		await d.execute(
			"DELETE FROM snapshots WHERE timestamp < datetime('now', '-30 days')",
		);
	} catch (e) {
		console.warn("saveSnapshot failed:", e);
	}
}

// === Chart ===
async function renderChart() {
	try {
		const d = await getDb();
		const rows = await d.select(
			`SELECT
         date(timestamp) as day,
         MAX(seven_day) as peak
       FROM snapshots
       WHERE timestamp >= datetime('now', '-7 days')
       GROUP BY date(timestamp)
       ORDER BY day`,
		);

		const canvas = $("chart");
		const ctx = canvas.getContext("2d");
		const W = canvas.width;
		const H = canvas.height;
		const pad = { top: 20, right: 16, bottom: 24, left: 36 };

		ctx.clearRect(0, 0, W, H);

		if (rows.length === 0) {
			ctx.fillStyle = "rgba(255,255,255,0.2)";
			ctx.font = "12px system-ui";
			ctx.textAlign = "center";
			ctx.fillText("No history yet", W / 2, H / 2);
			return;
		}

		const chartW = W - pad.left - pad.right;
		const chartH = H - pad.top - pad.bottom;
		const barW = Math.min(24, (chartW / rows.length) * 0.6);
		const gap = (chartW - barW * rows.length) / (rows.length + 1);

		// Y axis labels
		ctx.fillStyle = "rgba(255,255,255,0.25)";
		ctx.font = "10px system-ui";
		ctx.textAlign = "right";
		for (const pct of [0, 25, 50, 75, 100]) {
			const y = pad.top + chartH - (pct / 100) * chartH;
			ctx.fillText(`${pct}%`, pad.left - 6, y + 3);
			ctx.strokeStyle = "rgba(255,255,255,0.04)";
			ctx.beginPath();
			ctx.moveTo(pad.left, y);
			ctx.lineTo(W - pad.right, y);
			ctx.stroke();
		}

		// Bars
		rows.forEach((row, i) => {
			const x = pad.left + gap + i * (barW + gap);
			const h = (row.peak / 100) * chartH;
			const y = pad.top + chartH - h;

			const grad = ctx.createLinearGradient(x, y, x, y + h);
			if (row.peak >= 75) {
				grad.addColorStop(0, "#ff5252");
				grad.addColorStop(1, "#ff525240");
			} else if (row.peak >= 50) {
				grad.addColorStop(0, "#ffab00");
				grad.addColorStop(1, "#ffab0040");
			} else {
				grad.addColorStop(0, "#00e676");
				grad.addColorStop(1, "#00e67640");
			}

			ctx.fillStyle = grad;
			ctx.beginPath();
			ctx.roundRect(x, y, barW, h, [3, 3, 0, 0]);
			ctx.fill();

			// Day label
			const dayLabel = new Date(row.day + "T12:00:00")
				.toLocaleDateString("en", { weekday: "short" })
				.charAt(0);
			ctx.fillStyle = "rgba(255,255,255,0.3)";
			ctx.font = "10px system-ui";
			ctx.textAlign = "center";
			ctx.fillText(dayLabel, x + barW / 2, H - 6);
		});
	} catch (e) {
		console.warn("renderChart failed:", e);
	}
}

// === Alerts ===
async function checkAlerts(data) {
	const alertBar = $("alert-bar");
	const alertText = $("alert-text");

	let warn = 75;
	let crit = 90;

	try {
		const d = await getDb();
		const cfgRows = await d.select(
			"SELECT key, value FROM config WHERE key IN ('alert_threshold_warning', 'alert_threshold_critical')",
		);
		cfgRows.forEach((r) => {
			if (r.key === "alert_threshold_warning") warn = parseInt(r.value);
			if (r.key === "alert_threshold_critical") crit = parseInt(r.value);
		});
	} catch (e) {
		console.warn("checkAlerts config read failed:", e);
	}

	const maxUsage = Math.max(data.fiveHour ?? 0, data.sevenDay ?? 0);

	if (maxUsage >= crit) {
		alertBar.className = "alert-bar";
		alertText.textContent = `Critical: ${Math.round(maxUsage)}% usage`;
		sendNotification(
			"Claude Usage Critical",
			`You've used ${Math.round(maxUsage)}% of your limit`,
		);
	} else if (maxUsage >= warn) {
		alertBar.className = "alert-bar";
		alertText.textContent = `Warning: ${Math.round(maxUsage)}% usage`;
	} else {
		alertBar.className = "alert-bar ok";
		alertText.textContent = `All good — alert at ${warn}%`;
	}
}

let lastNotifTime = 0;

async function sendNotification(title, body) {
	const now = Date.now();
	if (now - lastNotifTime < 30 * 60 * 1000) return;
	lastNotifTime = now;

	try {
		const { sendNotification, isPermissionGranted, requestPermission } =
			await import("@tauri-apps/plugin-notification");
		let granted = await isPermissionGranted();
		if (!granted) granted = (await requestPermission()) === "granted";
		if (granted) sendNotification({ title, body });
	} catch (e) {
		console.warn("notification failed:", e);
	}
}

// === Polling ===
function startPolling(intervalSecs = 300) {
	if (pollTimer) clearInterval(pollTimer);
	pollTimer = setInterval(refresh, intervalSecs * 1000);
}

// === Event Bindings ===
function bindEvents() {
	$("btn-login").addEventListener("click", async () => {
		try {
			await invoke("login");
			showUsage();
			await refresh();
			startPolling();
		} catch (e) {
			console.error("Login failed:", e);
		}
	});

	$("btn-refresh").addEventListener("click", refresh);
	$("btn-settings").addEventListener("click", toggleSettings);

	$("btn-save-settings").addEventListener("click", async () => {
		try {
			const d = await getDb();
			await d.execute(
				"UPDATE config SET value = $1 WHERE key = 'alert_threshold_warning'",
				[$("cfg-warning").value],
			);
			await d.execute(
				"UPDATE config SET value = $1 WHERE key = 'alert_threshold_critical'",
				[$("cfg-critical").value],
			);
			const interval = parseInt($("cfg-interval").value);
			await d.execute(
				"UPDATE config SET value = $1 WHERE key = 'poll_interval_secs'",
				[String(interval)],
			);
			startPolling(interval);
			showUsage();
			await refresh();
		} catch (e) {
			console.warn("save settings failed:", e);
		}
	});

	$("btn-logout").addEventListener("click", async () => {
		try {
			await invoke("logout");
		} catch (e) {
			console.warn("logout failed:", e);
		}
		if (pollTimer) clearInterval(pollTimer);
		showLogin();
	});
}

// === Load settings into form ===
async function loadSettings() {
	try {
		const d = await getDb();
		const rows = await d.select("SELECT key, value FROM config");
		rows.forEach((r) => {
			if (r.key === "alert_threshold_warning") $("cfg-warning").value = r.value;
			if (r.key === "alert_threshold_critical")
				$("cfg-critical").value = r.value;
			if (r.key === "poll_interval_secs") $("cfg-interval").value = r.value;
		});
	} catch (e) {
		console.warn("loadSettings failed:", e);
	}
}

// === Start ===
document.addEventListener("DOMContentLoaded", () => {
	init();
	loadSettings();
});
