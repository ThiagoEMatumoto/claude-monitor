import { $, invoke, invokeWithRetry, getDb, formatTimeUntil, getLevel, linearRegression } from "./utils.js";
import { renderChart, setChartRangeDays } from "./chart.js";

let pollTimer = null;
let basePollInterval = 300;
let activePollInterval = 300;
let lastNotifTime = 0;
let rateLimitedUntil = 0;

export function setBasePollInterval(interval) {
	basePollInterval = interval;
	activePollInterval = interval;
}

export function getBasePollInterval() {
	return basePollInterval;
}

export function startPolling(intervalSecs) {
	const secs = intervalSecs ?? basePollInterval;
	if (pollTimer) clearInterval(pollTimer);
	pollTimer = setInterval(refresh, secs * 1000);
}

export function stopPolling() {
	if (pollTimer) clearInterval(pollTimer);
	pollTimer = null;
}

export async function refresh() {
	if (Date.now() < rateLimitedUntil) {
		console.warn(`rate limited, skipping refresh until ${new Date(rateLimitedUntil).toLocaleTimeString()}`);
		return "rate_limited";
	}
	try {
		const data = await invokeWithRetry("get_usage");
		updateBars(data);
		await saveSnapshot(data);
		await renderChart();
		await checkAlerts(data);
		await updateBurnRates(data);
		updateTrayMenu(data);
		adaptPolling(data);
	} catch (e) {
		console.error("refresh failed:", e);
		if (String(e).includes("429")) {
			rateLimitedUntil = Date.now() + 5 * 60 * 1000;
			console.warn("rate limited by API, backing off for 5 minutes");
			startPolling(basePollInterval);
			return "rate_limited";
		}
		if (String(e).includes("not authenticated")) {
			return "not_authenticated";
		}
	}
	return "ok";
}

function adaptPolling(data) {
	const maxUsage = Math.max(data.fiveHour ?? 0, data.sevenDay ?? 0);
	let target;
	if (maxUsage >= 90) target = 30;
	else if (maxUsage >= 75) target = 60;
	else target = basePollInterval;

	if (target !== activePollInterval) {
		activePollInterval = target;
		startPolling(target);
	}
}

function updateBars(data) {
	setBar("5h", data.fiveHour, data.fiveHourResetsAt);
	setBar("7d", data.sevenDay, data.sevenDayResetsAt);
	setModelBar("sonnet", data.sonnet);
	setModelBar("opus", data.opus);
	updateCredits(data);
}

function updateCredits(data) {
	const section = $("credits-section");
	if (!section) return;

	if (!data.extraUsageEnabled) {
		section.classList.add("hidden");
		return;
	}

	section.classList.remove("hidden");
	const used = data.usedCredits ?? 0;
	const limit = data.monthlyLimit ?? 0;
	const pct = data.extraUsagePct ?? 0;

	$("credits-used").textContent = `$${used.toFixed(2)}`;
	$("credits-limit").textContent = `$${limit.toFixed(2)}`;

	const fill = $("bar-credits");
	fill.style.width = `${Math.min(pct * 100, 100)}%`;
	fill.className = "progress-fill " + getLevel(pct * 100);

	$("val-credits").textContent = `${Math.round(pct * 100)}%`;
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

function updateTrayMenu(data) {
	invoke("update_tray_menu", {
		fiveHour: data.fiveHour ?? 0,
		sevenDay: data.sevenDay ?? 0,
		fiveHourReset: data.fiveHourResetsAt
			? `in ${formatTimeUntil(data.fiveHourResetsAt)}`
			: null,
		sevenDayReset: data.sevenDayResetsAt
			? `in ${formatTimeUntil(data.sevenDayResetsAt)}`
			: null,
		sonnet: data.sonnet ?? null,
		opus: data.opus ?? null,
	}).catch((e) => console.warn("updateTrayMenu failed:", e));

	const maxUsage = Math.max(data.fiveHour ?? 0, data.sevenDay ?? 0);
	invoke("update_tray_icon", { maxUsage }).catch((e) =>
		console.warn("updateTrayIcon failed:", e),
	);
}

async function saveSnapshot(data) {
	try {
		const d = await getDb();
		await d.execute(
			"INSERT INTO snapshots (five_hour, seven_day, sonnet, opus, extra_usage_pct, used_credits) VALUES ($1, $2, $3, $4, $5, $6)",
			[
				data.fiveHour ?? 0,
				data.sevenDay ?? 0,
				data.sonnet ?? 0,
				data.opus ?? 0,
				data.extraUsagePct ?? null,
				data.usedCredits ?? null,
			],
		);
		await d.execute(
			"DELETE FROM snapshots WHERE timestamp < datetime('now', '-30 days')",
		);
	} catch (e) {
		console.warn("saveSnapshot failed:", e);
	}
}

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
		alertText.textContent = `All good \u2014 alert at ${warn}%`;
	}
}

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

async function updateBurnRates(data) {
	const section = $("burn-rate-section");
	if (!section) return;

	try {
		const d = await getDb();
		const rows = await d.select(
			"SELECT timestamp, five_hour, seven_day FROM snapshots WHERE timestamp >= datetime('now', '-2 hours') ORDER BY timestamp ASC",
		);

		if (rows.length < 2) {
			section.classList.add("hidden");
			return;
		}

		section.classList.remove("hidden");
		const container = section.querySelector(".burn-rates");

		const metrics = [
			{ key: "five_hour", label: "Session", resetsAt: data.fiveHourResetsAt, current: data.fiveHour },
			{ key: "seven_day", label: "Weekly", resetsAt: data.sevenDayResetsAt, current: data.sevenDay },
		];

		const parts = [];
		let shouldNotify = false;

		for (const m of metrics) {
			const points = rows.map((r) => ({
				x: new Date(r.timestamp + "Z").getTime() / 3600000,
				y: r[m.key],
			}));
			const { slope } = linearRegression(points);
			const rate = slope; // pct per hour

			if (Math.abs(rate) < 0.1) continue;

			const sign = rate > 0 ? "+" : "";
			let forecast = "";
			let cls = "burn-safe";

			if (rate > 0 && m.current < 100) {
				const hoursToLimit = (100 - m.current) / rate;
				const exhaustionTime = Date.now() + hoursToLimit * 3600000;
				const resetTime = m.resetsAt ? new Date(m.resetsAt).getTime() : null;

				if (resetTime && exhaustionTime < resetTime) {
					const minsLeft = Math.round(hoursToLimit * 60);
					forecast = minsLeft <= 60
						? `Limit in ~${minsLeft}min`
						: `Limit in ~${Math.round(hoursToLimit)}h`;
					cls = minsLeft <= 30 ? "burn-danger" : "burn-warning";
					if (minsLeft <= 30) shouldNotify = true;
				} else {
					forecast = "Safe";
				}
			}

			parts.push(
				`<span class="burn-item ${cls}">${m.label}: ${sign}${Math.round(rate)}%/h${forecast ? " \u2014 " + forecast : ""}</span>`,
			);
		}

		container.innerHTML = parts.join("");

		if (shouldNotify) {
			sendBurnRateNotification();
		}
	} catch (e) {
		console.warn("updateBurnRates failed:", e);
		section.classList.add("hidden");
	}
}

let lastBurnNotifTime = 0;

async function sendBurnRateNotification() {
	const now = Date.now();
	if (now - lastBurnNotifTime < 30 * 60 * 1000) return;
	lastBurnNotifTime = now;

	try {
		const { sendNotification, isPermissionGranted, requestPermission } =
			await import("@tauri-apps/plugin-notification");
		let granted = await isPermissionGranted();
		if (!granted) granted = (await requestPermission()) === "granted";
		if (granted) {
			sendNotification({
				title: "Claude Usage Warning",
				body: "At current burn rate, you'll hit the limit within 30 minutes",
			});
		}
	} catch (e) {
		console.warn("burn rate notification failed:", e);
	}
}

export function setupChartRangeToggle() {
	document.querySelectorAll(".range-btn").forEach((btn) => {
		btn.addEventListener("click", () => {
			document
				.querySelectorAll(".range-btn")
				.forEach((b) => b.classList.remove("active"));
			btn.classList.add("active");
			setChartRangeDays(parseInt(btn.dataset.range));
			renderChart();
		});
	});
}
