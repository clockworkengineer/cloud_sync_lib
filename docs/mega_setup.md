# MEGA Provider Setup Guide

This guide describes how to configure the **MEGA (mega.nz)** cloud storage integration. 

Unlike other storage providers that use OAuth 2.0 or dedicated API tokens, MEGA requires authentication using account credentials (email and password) to derive client-side keys for end-to-end encryption.

---

## Step 1: Prepare your MEGA Account

1. Register or log into your account at [MEGA](https://mega.nz/).
2. (Optional) Create a specific sub-folder inside your MEGA drive where you want the daemon to sync files (e.g. `PiSync`). If not specified, the daemon will default to syncing with your Cloud Drive root.

---

## Step 2: Configuration

To enable the MEGA provider, add the `[mega_credentials]` section to your configuration file (typically `private_config.toml` or `config.toml`).

### Configuration Schema

Add the following block to your configuration file:

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

## Step 3: Simulation Fallback (Offline Mode)

If you do not specify credentials, or if you disable the credentials block, the daemon automatically falls back to **Simulation Mode**.

In simulation mode, the daemon redirects read/write operations to a local mock folder specified by the `mega_root` setting:

```toml
mega_root = "./cloud_simulation/mega"
```

Files copied into your watched folder will be mirrored directly to this local path to simulate the cloud integration.
