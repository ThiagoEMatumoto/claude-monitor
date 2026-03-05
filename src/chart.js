import { $, getDb } from "./utils.js";

let lastChartHash = null;
let chartRangeDays = 7;

export function getChartRangeDays() {
	return chartRangeDays;
}

export function setChartRangeDays(days) {
	chartRangeDays = days;
	lastChartHash = null;
}

export function resetChartCache() {
	lastChartHash = null;
}

export async function renderChart() {
	try {
		const d = await getDb();
		const rows = await d.select(
			"SELECT date(timestamp) as day, MAX(seven_day) as peak FROM snapshots WHERE timestamp >= datetime('now', $1) GROUP BY date(timestamp) ORDER BY day",
			[`-${chartRangeDays} days`],
		);

		const hash = JSON.stringify(rows);
		if (hash === lastChartHash) return;
		lastChartHash = hash;

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
		try {
			const canvas = $("chart");
			const ctx = canvas.getContext("2d");
			ctx.clearRect(0, 0, canvas.width, canvas.height);
			ctx.fillStyle = "rgba(255,82,82,0.3)";
			ctx.font = "12px system-ui";
			ctx.textAlign = "center";
			ctx.fillText("Chart error", canvas.width / 2, canvas.height / 2);
		} catch (_) {}
	}
}
