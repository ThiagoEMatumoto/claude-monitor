<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="Claude Monitor" width="80" />
</p>

<h1 align="center">Claude Monitor</h1>

<p align="center">
  System tray application that tracks your Claude Pro / Max usage in real time.
  <br />
  Know exactly how much capacity you have left before hitting rate limits.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/tauri-v2-blue" alt="Tauri v2" />
  <img src="https://img.shields.io/badge/rust-1.75+-orange" alt="Rust" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License" />
</p>

---

## The Problem

Claude Pro and Max plans have rate limits — a **5-hour session window** and a **7-day rolling window** — but there's no easy way to see how much you've used. You only find out when you hit the limit and get throttled mid-conversation.

Claude Monitor sits in your system tray, polls the Anthropic usage API, and shows you exactly where you stand — with color-coded bars, per-model breakdowns, desktop notifications, and usage history.

## Features

- **System tray integration** — Lives in your taskbar with a compact `session% | weekly%` display
- **Real-time usage bars** — Session (5h) and Weekly (7d) limits with color-coded progress (green → yellow → red)
- **Per-model breakdown** — See individual Sonnet and Opus utilization
- **Reset countdowns** — Know exactly when your limits reset (`resets in 2h 34m`)
- **7-day usage history** — Bar chart showing daily peak usage over the past week
- **Desktop notifications** — Get alerted before you hit critical thresholds
- **Configurable alerts** — Set custom warning (default 75%) and critical (default 90%) thresholds
- **Adjustable polling** — Configure refresh interval from 60s to 30min (default 5min)
- **OAuth login** — Secure PKCE-based authentication with claude.ai
- **Local data** — All history stored in local SQLite, nothing leaves your machine
- **Lightweight** — ~23MB binary, minimal CPU/RAM footprint
- **Draggable popup** — Frameless glassmorphism design, drag from the header

## Screenshots

<details>
<summary>Tray menu (click to expand)</summary>

The tray icon shows a compact menu with session/weekly percentages, reset times, and per-model usage:

```
🟢 Session: 12%
     resets in 3h 42m
🟢 Weekly: 28%
     resets in 4d 8h
─────────────────
     Sonnet: 31%
     Opus: 15%
─────────────────
Show Details…
─────────────────
Quit
```

</details>

<details>
<summary>Details popup (click to expand)</summary>

The popup window shows full usage bars, per-model breakdown, 7-day history chart, and alert status — all in a dark glassmorphism design.

</details>

## Installation

### Prerequisites

- [Rust](https://rustup.rs/) 1.75+
- [Node.js](https://nodejs.org/) 18+ (for frontend dependencies)
- System libraries for Tauri v2:

**Ubuntu/Debian:**

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

**Fedora:**

```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libxdo-devel libappindicator-gtk3-devel librsvg2-devel
```

**Arch:**

```bash
sudo pacman -S webkit2gtk-4.1 base-devel curl wget file openssl \
  xdotool libappindicator-gtk3 librsvg
```

### Build from source

```bash
git clone https://github.com/ThiagoEMatumoto/claude-monitor.git
cd claude-monitor
npm install
npx tauri build
```

The binary will be at `src-tauri/target/release/claude-monitor`.

### macOS

After building, the `.app` bundle is at:

```
src-tauri/target/release/bundle/macos/Claude Monitor.app
```

To install, move it to your Applications folder:

```bash
cp -r "src-tauri/target/release/bundle/macos/Claude Monitor.app" /Applications/
```

Alternatively, open the generated DMG at `src-tauri/target/release/bundle/dmg/` and drag the app to Applications.

To run from the terminal:

```bash
open "/Applications/Claude Monitor.app"
```

### Linux

Copy the binary to your PATH:

```bash
cp src-tauri/target/release/claude-monitor ~/.local/bin/
```

### Run on startup (systemd)

Create `~/.config/systemd/user/claude-monitor.service`:

```ini
[Unit]
Description=Claude Monitor
After=graphical-session.target

[Service]
ExecStart=%h/.local/bin/claude-monitor
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

Then enable it:

```bash
systemctl --user daemon-reload
systemctl --user enable --now claude-monitor
```

## Usage

1. **First launch** — Click the tray icon → "Show Details…" → "Login with Claude"
2. Your browser will open for OAuth authentication with claude.ai
3. After login, usage data starts polling automatically every 5 minutes
4. **Tray icon** — Right-click for the quick usage menu
5. **Details popup** — Click "Show Details…" for the full dashboard
6. **Settings** — Click the gear icon to configure alert thresholds and polling interval
7. **Dismiss popup** — Press `Escape` or click the tray icon again

## Platform Support

| Platform            | Status           | Notes                                                              |
| ------------------- | ---------------- | ------------------------------------------------------------------ |
| **Linux (X11)**     | Fully supported  | Primary development platform. GNOME, KDE, XFCE tested.             |
| **Linux (Wayland)** | Supported        | Runs via XWayland. Tray requires AppIndicator extension on GNOME.  |
| **macOS**           | Builds, untested | Tauri v2 supports macOS natively. Should work but not validated.   |
| **Windows**         | Builds, untested | Tauri v2 supports Windows natively. Should work but not validated. |

> **Note for GNOME users:** You need the [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/) for system tray icons to appear.

## Architecture

```
claude-monitor/
├── src/                    # Frontend (vanilla HTML/CSS/JS)
│   ├── index.html          # Popup window structure
│   ├── style.css           # Glassmorphism dark theme
│   └── main.js             # Polling, charts, alerts, tray menu updates
├── src-tauri/              # Backend (Rust + Tauri v2)
│   ├── src/
│   │   ├── main.rs         # App setup, tray icon, window management
│   │   ├── commands.rs     # Tauri IPC commands (get_usage, login, etc.)
│   │   └── claude.rs       # Anthropic OAuth + Usage API client
│   ├── icons/              # App icons (Claude sparkle)
│   └── capabilities/       # Tauri permission definitions
```

**Data flow:**

1. Frontend polls `get_usage` command every N seconds
2. Rust backend calls `api.anthropic.com/api/oauth/usage` with OAuth token
3. Response updates both the popup UI and the tray menu text
4. Snapshots are saved to local SQLite for the history chart
5. Alert thresholds are checked and desktop notifications sent if exceeded

## Configuration

Settings are stored in SQLite (`~/.local/share/com.claude-monitor.app/claude-monitor.db`):

| Setting                    | Default | Description                      |
| -------------------------- | ------- | -------------------------------- |
| `alert_threshold_warning`  | 75%     | Yellow alert threshold           |
| `alert_threshold_critical` | 90%     | Red alert + desktop notification |
| `poll_interval_secs`       | 300     | Seconds between API polls        |
| `notif_sound_enabled`      | 1       | Play sound on notifications (1=on, 0=off) |

OAuth credentials are stored in `~/.config/claude-monitor/credentials.json`.

## Tech Stack

- **[Tauri v2](https://v2.tauri.app/)** — Lightweight alternative to Electron
- **Rust** — Backend, OAuth flow, API client
- **Vanilla JS** — Frontend, no framework overhead
- **SQLite** — Local usage history via `tauri-plugin-sql`
- **Anthropic OAuth** — PKCE-based login with `user:inference` scope

## License

MIT
