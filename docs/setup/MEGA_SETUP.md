# MEGA Provider Setup Guide

This guide describes how to configure the **MEGA (mega.nz)** cloud storage integration. 

Unlike other storage providers that use OAuth 2.0 or dedicated API tokens, MEGA requires authentication using account credentials (email and password) to derive client-side keys for end-to-end encryption.

---

## Step 1: Prepare your MEGA Account

1. Register or log into your account at [MEGA](https://mega.nz/).
2. Complete any pending email verification banner, phone confirmation, or terms-of-service acknowledgment prompt in the browser.
3. (Optional) Create a specific sub-folder inside your MEGA drive where you want the daemon to sync files (e.g. `PiSync`). If not specified, the daemon will default to syncing with your Cloud Drive root.

---

## Step 2: Configuration

To enable the MEGA provider, add the `[mega_credentials]` section to your configuration file (typically `private_config.toml` or `config.toml`).

### Configuration Schema

```toml
[mega_credentials]
email = "your-email@example.com"
password = "your-mega-password"
destination_folder = "PiSync" # Optional sub-folder name (creates it if missing)
enabled = true                # Set to false to disable this provider
sync = true                   # Set to false to disable deletion syncing
```

### Options

* **`email`** (String, Required): Your MEGA account login email address.
* **`password`** (String, Required): Your MEGA account login password.
* **`destination_folder`** (String, Optional): The folder name in your MEGA Cloud Drive to sync files into. If omitted, the root folder is used.
* **`enabled`** (Boolean, Optional): Defaults to `true`. Toggle to `false` to disable MEGA synchronization without deleting credentials.
* **`sync`** (Boolean, Optional): Defaults to `true`. Toggle to `false` if you want a one-way upload/download sync (i.e. do not replicate deletions on the remote server).

---

## Step 3: Verifying Connectivity

You can verify that your MEGA credentials and network path are working properly without launching the daemon using the standalone diagnostic binary:

```bash
cargo run --bin test_mega -p cloud_sync_daemon --features mega
```

---

## Step 4: Troubleshooting `EBLOCKED (-16)` & Verification Holds

If the verifier or daemon reports:
```text
StorageError::Authentication { message: "MEGA error: blocked" }
ErrorCode::EBLOCKED (-16)
```

1. **Why It Happens**: MEGA returns `-16` (`EBLOCKED`) with `"ach": 1` when the account has an unverified email address, an administrative hold, or pending terms confirmation that must be resolved via the official web UI.
2. **Remediation**:
   - Log into [https://mega.nz](https://mega.nz) using your credentials in a web browser.
   - Click the verification link in your confirmation email or accept the banner prompts in the web dashboard.
   - Re-run `cargo run --bin test_mega -p cloud_sync_daemon --features mega`.
3. **Automated Simulation Fallback**: While the account is in a held or blocked state, `cloud_sync_lib` automatically redirects read/write operations to `./cloud_simulation/mega`, preventing synchronization failures from blocking your other active cloud storage providers.

---

## Step 5: Simulation Fallback (Offline Mode)

If credentials are omitted, marked `enabled = false`, or experience authentication errors, the daemon redirects read/write operations to a local mock folder specified by the `mega_root` setting:

```toml
mega_root = "./cloud_simulation/mega"
```

Files copied into your watched folder will be mirrored directly to this local path to simulate the cloud integration.
