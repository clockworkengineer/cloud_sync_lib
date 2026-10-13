# `cloud_sync_ui`

Axum-based web dashboard interface and REST API gateway for `cloud_sync_lib`.

---

## 1. Overview

`cloud_sync_ui` provides a decoupled, real-time web dashboard for inspecting and controlling the `cloud_sync_daemon` background process. It acts as both:
1. An embedded HTTP dashboard server serving a single-page management application (`index.html`).
2. A REST & Server-Sent Events (SSE) API gateway translating web requests into daemon TCP socket commands.

The crate can be run standalone as a web server, or embedded directly into desktop wrappers such as `cloud_sync_tauri`.

---

## 2. Features

- **Decoupled Architecture**: Communicates with the daemon over local TCP socket (`127.0.0.1:8081` by default), allowing the web dashboard and sync engine to be started, stopped, or restarted independently.
- **Embedded Frontend**: Contains the complete frontend assets (HTML, CSS, JS) compiled directly into the binary, requiring no external web server or asset folder.
- **Real-Time Log Stream**: Dedicated `/api/logs` endpoint streaming plaintext daemon logs with seamless offline disk fallback.
- **Interactive Conflict Resolution**: UI cards allowing users to inspect and resolve 3-way synchronization collisions with "Keep Local", "Keep Remote", or "Keep Newer" decisions.
- **Server-Sent Events (SSE)**: Streams live file synchronization activity to connected clients at `/api/events`.

---

## 3. API Catalog

The server listens by default on `http://127.0.0.1:8082`.

### 3.1 Web Pages & Diagnostics
| Method | Endpoint | Description |
| :--- | :--- | :--- |
| `GET` | `/` | Serves the single-page management dashboard. |
| `GET` | `/api/status` | Returns daemon running state, active storage backends, paused state, and catalog paths as JSON. |
| `GET` | `/api/logs` | Returns active log file content as `text/plain; charset=utf-8`. |
| `GET` | `/api/events` | Server-Sent Events (SSE) stream broadcasting sync progress and errors. |

### 3.2 Daemon Control
| Method | Endpoint | Description | Daemon Command |
| :--- | :--- | :--- | :--- |
| `POST` | `/api/start` | Spawns the daemon process if offline. | (Process spawn) |
| `POST` | `/api/stop` | Terminates the background daemon cleanly. | `stop` |
| `POST` | `/api/pause` | Pauses file watcher and periodic sync cycles. | `pause` |
| `POST` | `/api/resume` | Resumes automated synchronization. | `resume` |
| `POST` | `/api/sync` | Triggers an immediate one-shot sync cycle. | `sync` |
| `POST` | `/api/reload` | Hot-reloads configuration files. | `reload` |
| `POST` | `/api/clear` | Clears recent event history in dashboard memory. | (Internal state) |

### 3.3 Conflict Resolution
| Method | Endpoint | Description |
| :--- | :--- | :--- |
| `GET` | `/api/conflicts` | Returns active synchronization conflicts list. |
| `POST` | `/api/conflicts/resolve` | Accepts JSON body `{ "path": "...", "resolution": "use_local" \| "use_remote" }` to resolve a conflict. |

---

## 4. Configuration & Environment Variables

| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `CLOUDSYNC_UI_ADDR` | `127.0.0.1:8082` | HTTP bind address for the web dashboard. |
| `CLOUDSYNC_DAEMON_ADDR` | `127.0.0.1:8081` | TCP address of the target `cloud_sync_daemon` control socket. |

---

## 5. Running Standalone

Launch using Cargo:
```bash
cargo run --bin cloud_sync_ui
```

Or using the Windows windowless batch launcher:
```cmd
launch_web_ui.cmd
```

Once running, navigate to `http://127.0.0.1:8082` in any modern web browser.
