import { $, invoke, debounce } from "./utils.js";
import { refresh, startPolling, stopPolling, setBasePollInterval, setupChartRangeToggle } from "./polling.js";
import { setupSessionListener, setupSessionClickHandler, refreshRecentSessions, setupSessionSearch } from "./sessions-ui.js";
import { loadSettings, createDebouncedSave, setupTurboToggle, setupNotifSoundToggle, exportData } from "./settings.js";
import { refreshInsights, setupInsightEvents } from "./analytics-ui.js";
import { refreshPlugins, setupPluginEvents } from "./plugins-ui.js";

// === State ===
let settingsOpen = false;
let activeTab = "dashboard";
let recentRefreshTimer = null;

// === Initialization ===
async function init() {
	const authenticated = await invoke("is_authenticated");
	if (authenticated) {
		showUsage();
		const result = await refresh();
		if (result === "not_authenticated") {
			showLogin();
			return;
		}
		startPolling();
	} else {
		showLogin();
	}
	bindEvents();
	setupSessionListener();
	setupSessionClickHandler("sessions-list");
	setupSessionClickHandler("recent-sessions-list");
	refreshRecentSessions();
	recentRefreshTimer = setInterval(refreshRecentSessions, 60000);
	setupTurboToggle();
	setupNotifSoundToggle();
	setupChartRangeToggle();
	setupInsightEvents();
	setupPluginEvents();
}

// === Views ===
function showLogin() {
	$("login-screen").classList.remove("hidden");
	$("tab-bar").classList.add("hidden");
	$("usage-section").classList.add("hidden");
	$("sessions-page").classList.add("hidden");
	$("settings-section").classList.add("hidden");
}

function showUsage() {
	$("login-screen").classList.add("hidden");
	$("tab-bar").classList.remove("hidden");
	$("settings-section").classList.add("hidden");
	settingsOpen = false;
	switchTab(activeTab);
}

const switchTab = debounce(function switchTabInner(tab) {
	activeTab = tab;
	document.querySelectorAll(".tab").forEach((t) => {
		t.classList.toggle("active", t.dataset.tab === tab);
	});
	$("usage-section").classList.toggle("hidden", tab !== "dashboard");
	$("sessions-page").classList.toggle("hidden", tab !== "activity");
	$("insights-page").classList.toggle("hidden", tab !== "insights");
	$("plugins-page").classList.toggle("hidden", tab !== "plugins");
	$("settings-section").classList.add("hidden");
	settingsOpen = false;
	if (tab === "insights") refreshInsights();
	if (tab === "plugins") refreshPlugins();
}, 150);

async function toggleSettings() {
	settingsOpen = !settingsOpen;
	$("settings-section").classList.toggle("hidden", !settingsOpen);
	$("usage-section").classList.toggle(
		"hidden",
		settingsOpen || activeTab !== "dashboard",
	);
	$("sessions-page").classList.toggle(
		"hidden",
		settingsOpen || activeTab !== "activity",
	);
	$("insights-page").classList.toggle(
		"hidden",
		settingsOpen || activeTab !== "insights",
	);
	$("plugins-page").classList.toggle(
		"hidden",
		settingsOpen || activeTab !== "plugins",
	);
	if (settingsOpen) {
		const result = await loadSettings();
		if (result.pollInterval) setBasePollInterval(result.pollInterval);
	}
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

	$("btn-refresh").addEventListener("click", async () => {
		const btn = $("btn-refresh");
		btn.classList.add("refreshing");
		await refresh();
		btn.classList.remove("refreshing");
	});
	$("btn-settings").addEventListener("click", toggleSettings);
	$("btn-close").addEventListener("click", () => {
		invoke("hide_window");
	});

	document.querySelectorAll(".tab").forEach((t) => {
		t.addEventListener("click", () => switchTab(t.dataset.tab));
	});

	const debouncedSave = createDebouncedSave((interval) => {
		setBasePollInterval(interval);
		startPolling(interval);
		showUsage();
		refresh();
	});
	$("btn-save-settings").addEventListener("click", debouncedSave);

	$("btn-logout").addEventListener("click", async () => {
		try {
			await invoke("logout");
		} catch (e) {
			console.warn("logout failed:", e);
		}
		stopPolling();
		showLogin();
	});
}

// === Keyboard Shortcuts ===
function setupKeyboardShortcuts() {
	window.addEventListener("keydown", async (e) => {
		if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;

		switch (e.key) {
			case "Escape":
				invoke("hide_window");
				break;
			case "r":
			case "R":
				if (!e.ctrlKey && !e.metaKey) {
					const btn = $("btn-refresh");
					btn.classList.add("refreshing");
					await refresh();
					btn.classList.remove("refreshing");
				}
				break;
			case "1":
				switchTab("dashboard");
				break;
			case "2":
				switchTab("activity");
				break;
			case "3":
				switchTab("insights");
				break;
			case "4":
				switchTab("plugins");
				break;
		}
	});
}

// === Start ===
document.addEventListener("DOMContentLoaded", () => {
	init();
	loadSettings().then((result) => {
		if (result.pollInterval) setBasePollInterval(result.pollInterval);
	});
	setupKeyboardShortcuts();
	setupSessionSearch();

	const exportBtn = $("btn-export");
	if (exportBtn) {
		exportBtn.addEventListener("click", () => exportData("csv"));
	}
});
