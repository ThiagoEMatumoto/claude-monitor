import { $, invoke, formatElapsed, escapeHtml, escapeAttr } from "./utils.js";

let notifiedSessionIds = new Set();
let sessionRefreshTimer = null;

export async function setupSessionListener() {
	try {
		const { listen } = await import("@tauri-apps/api/event");
		await listen("sessions-changed", (event) => {
			handleSessionsUpdate(event.payload);
		});
	} catch (e) {
		console.warn("session listener setup failed:", e);
	}

	try {
		const sessions = await invoke("get_waiting_sessions");
		handleSessionsUpdate(sessions);
	} catch (e) {
		console.warn("initial session fetch failed:", e);
	}

	sessionRefreshTimer = setInterval(async () => {
		try {
			const sessions = await invoke("get_waiting_sessions");
			renderWaitingSessions(sessions);
		} catch (_) {}
	}, 30000);
}

function handleSessionsUpdate(sessions) {
	renderWaitingSessions(sessions);
	updateTraySessionCount(sessions.length);
	updateSessionBadge(sessions.length);

	for (const session of sessions) {
		if (!notifiedSessionIds.has(session.sessionId)) {
			notifiedSessionIds.add(session.sessionId);
			sendSessionNotification(session);
			invoke("play_sound").catch(() => {});
		}
	}

	const currentIds = new Set(sessions.map((s) => s.sessionId));
	for (const id of notifiedSessionIds) {
		if (!currentIds.has(id)) {
			notifiedSessionIds.delete(id);
		}
	}
}

function renderWaitingSessions(sessions) {
	const list = $("sessions-list");
	const empty = $("sessions-empty");

	if (!sessions || sessions.length === 0) {
		list.innerHTML = "";
		list.classList.add("hidden");
		empty.classList.remove("hidden");
		return;
	}

	empty.classList.add("hidden");
	list.classList.remove("hidden");

	const icons = { question: "?", approval: "!", completed: "\u2713" };

	list.innerHTML = sessions
		.map((s) => {
			const elapsed = formatElapsed(s.idleSince);
			const icon = icons[s.sessionType] || "\u2022";
			return `<div class="session-item ${s.sessionType} clickable" data-session-id="${escapeAttr(s.sessionId)}" data-cwd="${escapeAttr(s.cwd)}">
			<div class="session-icon">${icon}</div>
			<div class="session-info">
				<div class="session-project">${escapeHtml(s.project)}</div>
				<div class="session-text">${escapeHtml(s.lastText)}</div>
			</div>
			<div class="session-time">${elapsed}</div>
		</div>`;
		})
		.join("");
}

function updateSessionBadge(count) {
	const badge = $("tab-badge");
	if (count > 0) {
		badge.textContent = count;
		badge.classList.remove("hidden");
	} else {
		badge.classList.add("hidden");
	}
}

function updateTraySessionCount(count) {
	invoke("update_tray_sessions", { count }).catch((e) =>
		console.warn("updateTraySessionCount failed:", e),
	);
}

async function sendSessionNotification(session) {
	const titles = {
		question: `Claude has a question \u2014 ${session.project}`,
		approval: `Claude needs approval \u2014 ${session.project}`,
		completed: `Claude finished \u2014 ${session.project}`,
	};
	const title =
		titles[session.sessionType] || `Claude waiting \u2014 ${session.project}`;
	const body = session.lastText || "";

	try {
		const { sendNotification, isPermissionGranted, requestPermission } =
			await import("@tauri-apps/plugin-notification");
		let granted = await isPermissionGranted();
		if (!granted) granted = (await requestPermission()) === "granted";
		if (granted) sendNotification({ title, body });
	} catch (e) {
		console.warn("session notification failed:", e);
	}
}

export function setupSessionClickHandler(listElementId) {
	const list = $(listElementId);
	if (!list) return;
	list.addEventListener("click", async (e) => {
		const item = e.target.closest(".session-item.clickable");
		if (!item) return;
		const { sessionId, cwd } = item.dataset;
		if (!sessionId) return;
		try {
			await invoke("resume_session", { sessionId, cwd: cwd || "" });
		} catch (err) {
			console.error("resume_session failed:", err);
		}
	});
}

export function renderRecentSessions(sessions) {
	const list = $("recent-sessions-list");
	const empty = $("recent-empty");

	if (!sessions || sessions.length === 0) {
		list.innerHTML = "";
		list.classList.add("hidden");
		empty.classList.remove("hidden");
		return;
	}

	empty.classList.add("hidden");
	list.classList.remove("hidden");

	const statusIcons = { active: "\u25B6", waiting: "\u23F8", idle: "\u25CB" };

	list.innerHTML = sessions
		.map((s) => {
			const elapsed = formatElapsed(s.lastModified);
			const icon = statusIcons[s.status] || "\u25CB";
			return `<div class="session-item ${s.status} clickable" data-session-id="${escapeAttr(s.sessionId)}" data-cwd="${escapeAttr(s.cwd)}">
			<div class="session-icon">${icon}</div>
			<div class="session-info">
				<div class="session-project">${escapeHtml(s.project)}</div>
				<div class="session-text">${escapeHtml(s.lastText)}</div>
			</div>
			<div class="session-time">${elapsed}</div>
		</div>`;
		})
		.join("");
}

export async function refreshRecentSessions() {
	try {
		const sessions = await invoke("get_recent_sessions");
		renderRecentSessions(sessions);
	} catch (e) {
		console.warn("refreshRecentSessions failed:", e);
	}
}

export function setupSessionSearch() {
	const input = $("session-search");
	if (!input) return;
	input.addEventListener("input", () => {
		const query = input.value.toLowerCase();
		const items = document.querySelectorAll(
			"#recent-sessions-list .session-item, #sessions-list .session-item",
		);
		items.forEach((item) => {
			const text = item.textContent.toLowerCase();
			item.style.display = text.includes(query) ? "" : "none";
		});
	});
}
