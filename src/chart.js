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

function roundedRect(ctx, x, y, w, h, radii) {
	const [tl, tr, br, bl] = Array.isArray(radii) ? radii : [radii, radii, radii, radii];
	ctx.moveTo(x + tl, y);
	ctx.lineTo(x + w - tr, y);
	if (tr) ctx.quadraticCurveTo(x + w, y, x + w, y + tr);
	else ctx.lineTo(x + w, y);
	ctx.lineTo(x + w, y + h - br);
	if (br) ctx.quadraticCurveTo(x + w, y + h, x + w - br, y + h);
	else ctx.lineTo(x + w, y + h);
	ctx.lineTo(x + bl, y + h);
	if (bl) ctx.quadraticCurveTo(x, y + h, x, y + h - bl);
	else ctx.lineTo(x, y + h);
	ctx.lineTo(x, y + tl);
	if (tl) ctx.quadraticCurveTo(x, y, x + tl, y);
	else ctx.lineTo(x, y);
	ctx.closePath();
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
				grad.addColorStop(0, "rgba(255, 82, 82, 1)");
				grad.addColorStop(1, "rgba(255, 82, 82, 0.25)");
			} else if (row.peak >= 50) {
				grad.addColorStop(0, "rgba(255, 171, 0, 1)");
				grad.addColorStop(1, "rgba(255, 171, 0, 0.25)");
			} else {
				grad.addColorStop(0, "rgba(0, 230, 118, 1)");
				grad.addColorStop(1, "rgba(0, 230, 118, 0.25)");
			}

			ctx.fillStyle = grad;
			ctx.beginPath();
			roundedRect(ctx, x, y, barW, h, [3, 3, 0, 0]);
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
			ctx.font = "11px system-ui";
			ctx.textAlign = "center";
			ctx.fillText(String(e).slice(0, 40), canvas.width / 2, canvas.height / 2);
		} catch (_) {}
	}
}
