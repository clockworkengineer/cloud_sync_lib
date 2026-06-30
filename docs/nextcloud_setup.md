# Nextcloud Provider Setup Guide

This guide describes how to configure and verify synchronization using the Nextcloud storage provider.

---

## Setup 1: Authentication & App Passwords

For security, it is highly recommended to use a Nextcloud **App Password** rather than your main account password.

1. Log into your Nextcloud web dashboard.
2. Go to **Personal Settings** > **Security**.
3. Under the **Devices & sessions** section at the bottom, enter an app name (e.g., `CloudSyncDaemon`).
4. Click **Create new app password**.
5. Copy the generated password. *(Note: This password will only be shown once).*

---

## Setup 2: Connection Configuration

Open your `private_config.toml` (or `config.toml`) and configure your connection credentials:

```toml
[nextcloud_credentials]
url = "https://nextcloud.example.com"    # Your Nextcloud server base URL
username = "my_username"                  # Your Nextcloud username
app_password = "xxxx-xxxx-xxxx-xxxx"      # The generated App Password
destination_folder = "Backups"            # Optional: folder under Files
enabled = true
sync = true
```

*Note: The sync client automatically connects to the correct Nextcloud WebDAV API endpoint at `/remote.php/dav/files/{username}/` using your base server URL.*

---

## Local Development: Running a Local Nextcloud Server

To test Nextcloud synchronization locally without hitting a remote server, you can spin up a local instance using Docker.

### 1. Start Nextcloud via Docker
Run the following command to start a Nextcloud container listening on port `8080` with default admin credentials:

```bash
docker run -d --name local-nextcloud \
  -p 8080:80 \
  -e NEXTCLOUD_ADMIN_USER=admin \
  -e NEXTCLOUD_ADMIN_PASSWORD=admin \
  nextcloud
```

### 2. Connection Configuration for Local Testing
Configure your `private_config.toml` to connect to the local container:

```toml
[nextcloud_credentials]
url = "http://127.0.0.1:8080"
username = "admin"
app_password = "admin"                    # Or generate an app password via dashboard
destination_folder = "Backups"
enabled = true
sync = true
```
