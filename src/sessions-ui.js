import { $, invoke, formatElapsed, escapeHtml, escapeAttr, sendTauriNotification } from "./utils.js";

let notifiedSessionIds = new Set();
let sessionRefreshTimer = null;

export async function setupSessionListener() {
	try {
		const listen = window.__TAURI__.event.listen;
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

	list.innerHTML = sessions
		.map((s) => {
			const elapsed = formatElapsed(s.idleSince);

			// P5: Build context tags
			let tags = "";
			if (s.pendingTool) {
				tags += `<span class="session-tag tool">${escapeHtml(s.pendingTool)}</span>`;
			}
			if (s.pendingFiles && s.pendingFiles.length > 0) {
				tags += s.pendingFiles
					.slice(0, 2)
					.map((f) => `<span class="session-tag file">${escapeHtml(f)}</span>`)
					.join("");
			}
			if (!s.pendingTool && s.sessionType === "question") {
				tags += `<span class="session-tag question-tag">question</span>`;
			}
			if (!s.pendingTool && s.sessionType === "completed") {
				tags += `<span class="session-tag completed-tag">done</span>`;
			}

			const tagsHtml = tags ? `<div class="session-tags">${tags}</div>` : "";

			return `<div class="session-item ${s.sessionType} clickable" data-session-id="${escapeAttr(s.sessionId)}" data-cwd="${escapeAttr(s.cwd)}">
			<div class="session-dot ${s.sessionType}"></div>
			<div class="session-info">
				<div class="session-header">
					<span class="session-project">${escapeHtml(s.project)}</span>
					<span class="session-time">${elapsed}</span>
				</div>
				${tagsHtml}
				<div class="session-text">${escapeHtml(s.lastText)}</div>
			</div>
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
	await sendTauriNotification(title, body);
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

	list.innerHTML = sessions
		.map((s) => {
			const elapsed = formatElapsed(s.lastModified);
			return `<div class="session-item ${s.status} clickable" data-session-id="${escapeAttr(s.sessionId)}" data-cwd="${escapeAttr(s.cwd)}">
			<div class="session-dot ${s.status}"></div>
			<div class="session-info">
				<div class="session-header">
					<span class="session-project">${escapeHtml(s.project)}</span>
					<span class="session-time">${elapsed}</span>
				</div>
				<div class="session-text">${escapeHtml(s.lastText)}</div>
			</div>
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
