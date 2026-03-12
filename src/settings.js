import { $, invoke, getDb, debounce } from "./utils.js";
import { setResetNotifMinutes } from "./polling.js";

let notifSoundEnabled = true;

export function isNotifSoundEnabled() {
	return notifSoundEnabled;
}

export async function loadSettings() {
	try {
		const d = await getDb();
		const rows = await d.select("SELECT key, value FROM config");
		const result = {};
		rows.forEach((r) => {
			if (r.key === "alert_threshold_warning") $("cfg-warning").value = r.value;
			if (r.key === "alert_threshold_critical")
				$("cfg-critical").value = r.value;
			if (r.key === "poll_interval_secs") {
				$("cfg-interval").value = r.value;
				result.pollInterval = parseInt(r.value) || 300;
			}
			if (r.key === "reset_notif_minutes") {
				$("cfg-reset-notif").value = r.value;
				setResetNotifMinutes(parseInt(r.value) || 0);
			}
			if (r.key === "notif_sound_enabled") {
				notifSoundEnabled = r.value !== "0";
			}
		});
		return result;
	} catch (e) {
		console.warn("loadSettings failed:", e);
		return {};
	}
}

export function createDebouncedSave(onSaved) {
	return debounce(async () => {
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
			// P7: Reset notification setting
			const resetNotif = parseInt($("cfg-reset-notif").value) || 0;
			await d.execute(
				"INSERT OR REPLACE INTO config (key, value) VALUES ('reset_notif_minutes', $1)",
				[String(resetNotif)],
			);
			setResetNotifMinutes(resetNotif);
			onSaved(interval);
		} catch (e) {
			console.warn("save settings failed:", e);
		}
	}, 500);
}

export async function setupTurboToggle() {
	try {
		const enabled = await invoke("is_turbo_enabled");
		const btn = $("btn-turbo");
		if (enabled) {
			btn.textContent = "Enabled";
			btn.classList.add("active");
		} else {
			btn.textContent = "Enable";
			btn.classList.remove("active");
		}

		btn.addEventListener("click", async () => {
			const isActive = btn.classList.contains("active");
			try {
				if (isActive) {
					await invoke("disable_turbo_mode");
					btn.textContent = "Enable";
					btn.classList.remove("active");
				} else {
					await invoke("enable_turbo_mode");
					btn.textContent = "Enabled";
					btn.classList.add("active");
				}
			} catch (e) {
				console.error("turbo toggle failed:", e);
			}
		});
	} catch (e) {
		console.warn("turbo setup failed:", e);
	}
}

export async function setupNotifSoundToggle() {
	const btn = $("btn-notif-sound");
	if (!btn) return;

	function updateBtn() {
		btn.textContent = notifSoundEnabled ? "Enabled" : "Disabled";
		btn.classList.toggle("active", notifSoundEnabled);
	}
	updateBtn();

	btn.addEventListener("click", async () => {
		notifSoundEnabled = !notifSoundEnabled;
		updateBtn();
		try {
			const d = await getDb();
			await d.execute(
				"INSERT OR REPLACE INTO config (key, value) VALUES ('notif_sound_enabled', $1)",
				[notifSoundEnabled ? "1" : "0"],
			);
		} catch (e) {
			console.warn("notif sound toggle save failed:", e);
		}
	});
}

export async function exportData(format = "csv") {
	try {
		const d = await getDb();
		const rows = await d.select(
			"SELECT timestamp, five_hour, seven_day, sonnet, opus FROM snapshots ORDER BY timestamp",
		);
		let content, mime, ext;
		if (format === "json") {
			content = JSON.stringify(rows, null, 2);
			mime = "application/json";
			ext = "json";
		} else {
			const header = "timestamp,five_hour,seven_day,sonnet,opus";
			const lines = rows.map(
				(r) =>
					`${r.timestamp},${r.five_hour},${r.seven_day},${r.sonnet ?? ""},${r.opus ?? ""}`,
			);
			content = [header, ...lines].join("\n");
			mime = "text/csv";
			ext = "csv";
		}
		const blob = new Blob([content], { type: mime });
		const url = URL.createObjectURL(blob);
		const a = document.createElement("a");
		a.href = url;
		a.download = `claude-monitor-export.${ext}`;
		a.click();
		URL.revokeObjectURL(url);
	} catch (e) {
		console.error("export failed:", e);
	}
}
