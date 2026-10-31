# Troubleshooting & Operations Guide

**Project**: `cloud_sync_lib`  
**Applies to**: `cloud_sync_daemon`, `cloud_sync_ui`, `cloud_sync_egui`, and cloud providers

---

## 1. Quick Diagnostic Flowchart

```
                          [ Issue Encountered ]
                                    │
       ┌────────────────────────────┼────────────────────────────┐
       ▼                            ▼                            ▼
[ Provider Shows [Simulated] ] [ Daemon Fails to Start ]    [ GUI Console Window Popup ]
       │                            │                            │
       ▼                            ▼                            ▼
Missing / expired OAuth      Port 8081 / 8082 collision   Use scripts/launch_egui.cmd /
tokens or placeholder IDs.   or lock on config file.      launch_web_ui.cmd to
See Section 2 & 3.           See Section 5.               run windowless. See Sec 4.
```

---

## 2. Cloud Provider Authentication & OAuth Refresh

When cloud provider tokens expire, API calls fail with `401 Unauthorized` or `invalid_grant`, and backends may fall back to `Simulated` mode.

### 2.1 Google Drive Token Renewal
1. Ensure your Google Cloud Console project has the **Google Drive API** enabled with redirect URI `http://localhost:8080`.
2. Run the token renewal script from the project root:
   ```bash
   python scripts/get_refresh_token.py
   ```
3. Enter your **Client ID** and **Client Secret** (or leave blank if defaults in the script match your application).
4. The script opens your default browser. Grant permissions on the Google OAuth consent screen.
5. The script receives the OAuth authorization code on `http://localhost:8080`, requests the tokens, and outputs:
   ```text
   Successfully retrieved tokens!
   Refresh Token: 1//0gxxxxxxxxxxxxxxxxxxxxxxxxxxx
   ```
6. Update your `private_config.toml` (or `config.toml`):
   ```toml
   [providers.google_drive]
   enabled = true
   client_id = "your_actual_client_id.apps.googleusercontent.com"
   client_secret = "your_actual_client_secret"
   refresh_token = "1//0gxxxxxxxxxxxxxxxxxxxxxxxxxxx"
   ```

### 2.2 Dropbox Token Renewal
1. In the Dropbox App Console, ensure permissions include `files.content.write` and `files.content.read`, with redirect URI `http://localhost:8080`.
2. Run the helper script:
   ```bash
   python scripts/get_dropbox_token.py
   ```
3. Complete authorization in your browser and copy the resulting `refresh_token` into `private_config.toml`:
   ```toml
   [providers.dropbox]
   enabled = true
   client_id = "your_dropbox_app_key"
   client_secret = "your_dropbox_app_secret"
   refresh_token = "your_generated_refresh_token"
   ```

### 2.3 Microsoft OneDrive Token Renewal
1. Register an application in Azure Portal (App Registrations) with redirect URI `http://localhost:8080` and permissions `Files.ReadWrite.All`, `offline_access`.
2. Run:
   ```bash
   python scripts/get_onedrive_token.py
   ```
3. Paste the returned `refresh_token` into `private_config.toml`:
   ```toml
   [providers.onedrive]
   enabled = true
   client_id = "your_application_client_id"
   client_secret = "your_application_client_secret"
   refresh_token = "your_onedrive_refresh_token"
   ```

### 2.4 Box Token Renewal
1. In the Box Developer Console, configure OAuth 2.0 with redirect URI `http://localhost:8080`.
2. Run:
   ```bash
   python scripts/get_box_token.py
   ```
3. Update `private_config.toml`:
   ```toml
   [providers.box]
   enabled = true
   client_id = "your_box_client_id"
   client_secret = "your_box_client_secret"
   refresh_token = "your_box_refresh_token"
   ```

### 2.5 MEGA Provider: Resolving `EBLOCKED (-16)` & Verification Holds

When using the MEGA provider, login may authenticate successfully via the `cs` endpoint, but subsequent operations fail with:
```text
StorageError::Authentication { message: "MEGA error: blocked" }
ErrorCode::EBLOCKED (-16)
```

#### Why This Occurs:
MEGA returns `ErrorCode::EBLOCKED` (`-16`) when an account is subject to an administrative hold, unconfirmed email registration, or terms of service verification. The API response returns `"ach": 1` (Account Check Required), which blocks programmatic API sessions from reading or writing nodes.

#### Resolution Steps:
1. Open a browser and log in directly to [https://mega.nz](https://mega.nz) using your configured MEGA email and password.
2. Complete any pending email verification banner, phone confirmation, or terms-of-service acknowledgment prompt.
3. Once the web dashboard is accessible without prompts, run the standalone verifier to confirm access:
   ```bash
   cargo run --bin test_mega -p cloud_sync_daemon --features mega
   ```
4. If the account remains held, `cloud_sync_lib` will automatically fall back to `LocalSimulation` in `./cloud_simulation/mega` so other enabled backends and local file syncing continue uninterrupted.

---

## 3. Resolving `[Simulated]` Provider Status

### 3.1 What Does `[Simulated]` Mean?
In the Web Dashboard and desktop UI, you may see backends labeled with `[Simulated]` (e.g., `Google Drive [Simulated]`).

`cloud_sync_lib` uses an automated fallback design (`SimulatedFallback`):
- When a provider is marked `enabled = true` in the configuration, but credentials are empty, contain placeholder text (e.g., `"your_client_id"`, `"placeholder"`, `"dummy"`), or encounter authentication holds (`e.is_auth()`), the daemon **does not crash**.
- Instead, it falls back to an isolated directory simulation backend (`./cloud_simulation/<provider>`) so local syncing logic and other active providers operate safely.

### 3.2 How to Switch to Real Cloud Syncing
1. Open `private_config.toml` (or `config.toml`).
2. Verify that credentials are fully populated with genuine tokens and secrets.
3. Reload or restart the daemon:
   ```bash
   echo "reload" | nc 127.0.0.1 8081
   # or click "Reload Config" in the Web UI
   ```
4. Verify that the `[Simulated]` tag disappears from the provider pills list.

---

## 4. Windows MSVC Build & OpenSSL Prerequisites

When compiling crates with the `sftp` feature on Windows (`cloud_sync_lib` or `test_sftp`), the build may depend on `openssl-sys` or `ssh2`:

### 4.1 Missing Perl Warning (`openssl-sys`)
If `cargo build --features sftp` fails with:
```text
Error configuring OpenSSL build: Command 'perl' not found. Is perl installed?
```
`openssl-src` requires Perl to execute `./Configure` on Windows.

#### Resolution:
- Install Strawberry Perl or Git for Windows (which provides Perl in `C:\Program Files\Git\usr\bin\perl.exe`).
- Alternatively, set `OPENSSL_DIR` to point to a pre-compiled OpenSSL v3 installation (e.g. from vcpkg: `vcpkg install openssl:x64-windows-static`).

---

## 5. Port Conflicts & Socket Errors

### 5.1 Common Port Conflicts
The default network ports are:
* **8081**: Daemon TCP Control Socket (`127.0.0.1:8081`)
* **8082**: Axum Web UI HTTP Server (`127.0.0.1:8082`)
* **8080**: OAuth token callback listener (`http://localhost:8080`)

If another process occupies these ports, you will see an error:
```text
Only one usage of each socket address (protocol/network address/port) is normally permitted. (os error 10048)
```

### 5.2 Finding and Terminating Conflicting Processes (Windows)
```powershell
# 1. Locate the Process ID (PID) occupying port 8081 or 8082
netstat -ano | findstr 8081
netstat -ano | findstr 8082

# 2. Terminate the process by PID
taskkill /PID <PID> /F
```

---

## 6. Circuit Breaker Trips & Sustained Outage Handling

When a cloud provider experiences downtime or API timeouts, `CircuitBreakerLayer` trips into `Open` state:
- **Symptom in Logs**: `WARN ... Circuit breaker tripped to OPEN for provider '<NAME>' after N failures`.
- **Behavior**: Subsequent sync calls fail immediately with `StorageError::CircuitBreakerOpen` without wasting network connections or retries.
- **Recovery**: The circuit will remain open for `recovery_timeout` (default: 15–30 seconds), then enter `HalfOpen` to test if the remote service has recovered. No manual restart is required.

---

## 7. State Desynchronization & Catalog Recovery

### 7.1 Symptoms of Corrupted State
* Repeated conflict warnings on every sync cycle for files that have not changed.
* Deleted local files continually reappearing from remote backups or vice-versa.
* Error message in `daemon.log`: `Error loading state database: unexpected end of file`.

### 7.2 Recovery Procedures

#### Step 1: Automatic `.bak` Fallback
The daemon automatically checks for `.sync_state_<backend>.bak` if the `.bin` database is corrupt. If you suspect only the primary `.bin` was damaged, delete the `.bin` and copy `.bak`:
```powershell
Copy-Item sync_state\.sync_state_Google_Drive.bak sync_state\.sync_state_Google_Drive.bin
```

#### Step 2: Fresh Full Resynchronization
If catalog state is desynchronized across machines:
1. Stop the daemon:
   ```bash
   echo "stop" | nc 127.0.0.1 8081
   ```
2. Remove catalog files from the isolated `sync_state/` directory:
   ```powershell
   Remove-Item sync_state\.sync_state_* -Force
   ```
3. Restart the daemon. The engine will scan both the local watched directory and the remote cloud storage, reconstruct a clean state catalog, and apply your `conflict_policy` (e.g. `newer_wins`).

---

## 8. Useful Diagnostic Commands

| Goal | Command |
| :--- | :--- |
| **Check Daemon Status** | `echo "status" \| nc 127.0.0.1 8081` |
| **Fetch Live Logs** | `echo "get_log" \| nc 127.0.0.1 8081` |
| **Trigger Immediate Sync** | `echo "sync" \| nc 127.0.0.1 8081` |
| **Reload Configuration** | `echo "reload" \| nc 127.0.0.1 8081` |
| **Check All Backends** | `cargo run --bin check_backends -p cloud_sync_daemon` |
| **Test MEGA Backend** | `cargo run --bin test_mega -p cloud_sync_daemon --features mega` |
| **Tail Log File in PowerShell** | `Get-Content daemon.log -Tail 50 -Wait` |
| **View Web UI Log Stream** | Open `http://127.0.0.1:8082/api/logs` in browser |
