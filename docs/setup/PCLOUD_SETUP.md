# pCloud Storage Setup Guide

This guide describes how to configure the pCloud backend in `cloud_sync_lib`, register an application in the pCloud Developer Console, and run local testing.

---

## Configuration in `private_config.toml`

Open your `private_config.toml` (and `config.toml`) file and add the `[pcloud_credentials]` section:

```toml
[pcloud_credentials]
# The user's OAuth2 access token for the pCloud API
access_token = "YOUR_PCLOUD_ACCESS_TOKEN"
# Custom endpoint URL (optional, defaults to https://api.pcloud.com or https://eapi.pcloud.com for European accounts)
endpoint = "https://api.pcloud.com"
# Optional destination folder/prefix path inside the account (creates it if missing)
destination_folder = "MySyncFolder"
# Enable/disable the pCloud sync client
enabled = true
# Enable/disable deletion syncing
sync = true
```

---

## Setup 1: Registering a pCloud Application (Production)

To connect to the official pCloud REST API, you need a pCloud account, a registered App, and an access token:

### 1. Register an App
1. Log in to the [pCloud App Console](https://docs.pcloud.com/).
2. Click **Create New App**.
3. Set your **App Name** (e.g. `ClockworkSync`).
4. Select **App Folder** access (recommended to restrict the app to its own subdirectory) or **Full pCloud** access.
5. Click **Create**.
6. Copy the **Client ID** and **Client Secret**.

### 2. Generate an Access Token
For command-line or daemon clients, you can generate a long-lived Access Token using the authorization code flow:
1. Navigate to:
   `https://docs.pcloud.com/oauth2/authorize?client_id=YOUR_CLIENT_ID&response_type=token`
2. Log in and authorize your app.
3. The browser will redirect to a URL containing the `access_token` fragment in the address bar. Copy this token and insert it into your `private_config.toml`.

---

## Local Development & Testing

Since pCloud utilizes a custom HTTPS JSON REST API, offline integration testing is handled via the provider's **Simulation Mode**.

When `access_token` is omitted (or set to an empty string) in `private_config.toml`, the daemon automatically defaults to simulation mode, redirecting synchronization traffic to the local directory:
`./cloud_simulation/pcloud`

---

## Viewing Storage Contents

### 1. pCloud Web Portal
For a quick view of your files, log in to the official [pCloud Web Console](https://my.pcloud.com/).

### 2. Desktop GUI Clients (Windows / macOS / Linux)
* **pCloud Drive**: The official pCloud desktop application mounts your cloud storage as a virtual disk on your system, allowing you to browse synced files locally.
