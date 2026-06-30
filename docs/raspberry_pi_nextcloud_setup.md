# Raspberry Pi Nextcloud Server Setup Guide

A complete guide to configuring your Raspberry Pi as a self-hosted Nextcloud server to receive backups and sync files from this Cloud Sync daemon.

---

## 1. Install Docker on Raspberry Pi

The simplest and most reliable way to run Nextcloud on a Raspberry Pi is using Docker.

### Step 1: Install Docker
SSH into your Raspberry Pi and run the convenience script:
```bash
curl -sSL https://get.docker.com | sh
```

### Step 2: Add Current User to Docker Group
Allows running Docker commands without prefixing with `sudo`:
```bash
sudo usermod -aG docker $USER
```
*(Log out and log back in for changes to take effect).*

---

## 2. Deploy Nextcloud Container

We will deploy Nextcloud along with an SQLite database (suitable for personal backup/sync operations).

### Run the Container Command:
```bash
docker run -d --name nextcloud-pi \
  -p 8080:80 \
  -v nextcloud_data:/var/www/html \
  --restart unless-stopped \
  nextcloud
```

*Note: Nextcloud will now be running on port `8080` of your Raspberry Pi.*

---

## 3. Initial Nextcloud Setup

1. Open your web browser and navigate to your Raspberry Pi's IP address on port `8080` (e.g. `http://192.168.1.150:8080`).
2. Create an **admin** username and password.
3. Click **Install**.
4. Once loaded, go to **Personal Settings** > **Security** > **Devices & sessions** (at the bottom) to create an **App Password** (e.g. `PiSyncClient`). Copy the generated password.

---

## 4. Connection Configuration

Identify your Raspberry Pi's local IP address by running `hostname -I` on the Pi. Then update your `private_config.toml` on your local host:

```toml
[nextcloud_credentials]
url = "http://192.168.1.150:8080"    # Your Raspberry Pi Nextcloud URL
username = "admin"                  # Your admin username
app_password = "xxxx-xxxx-xxxx-xxxx" # The generated App Password
destination_folder = "Backups"      # Folder name under Files in Nextcloud
enabled = true
sync = true
```
