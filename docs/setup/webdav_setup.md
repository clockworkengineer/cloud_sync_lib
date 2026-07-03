# WebDAV API Setup Guide

This guide provides instructions to configure the WebDAV storage backend (supporting Nextcloud, ownCloud, Box, Synology/QNAP NAS, etc.) in `cloud_sync_lib` and how to run a local WebDAV server for development and testing.

---

## Configuration in `private_config.toml`

Open your `private_config.toml` (and `config.toml`) file and add the `[webdav_credentials]` section:

```toml
[webdav_credentials]
# WebDAV server endpoint URL (include full protocol and path)
url = "https://your-nextcloud.com/remote.php/dav/files/username/"
# WebDAV connection credentials
username = "your_username"
password = "your_password_or_app_token"
# Optional prefix destination folder
destination_folder = "MySyncFolder"
# Enable/disable the WebDAV sync client
enabled = true
```

---

## Standard WebDAV Provider Setup Details

### 1. Nextcloud / ownCloud
Instead of your main account password, it is highly recommended to generate an **App Password**:
1. Log in to your Nextcloud/ownCloud Web Interface.
2. Go to **Settings** > **Personal** > **Security**.
3. Under **Devices & sessions**, type an app name (e.g., `cloud_sync_lib`) and click **Create new app password**.
4. Use your standard username and the newly generated app password in the configuration.
5. Nextcloud's WebDAV URL format is typically:
   `https://<your-instance>/remote.php/dav/files/<username>/`

### 2. Box
Box supports WebDAV connections for premium/enterprise accounts:
1. URL: `https://dav.box.com/dav`
2. Username: Your Box account email address.
3. Password: Your Box account password (or an App password if Single Sign-On is enabled).

### 3. Synology NAS
1. Install and enable the **WebDAV Server** package in DSM Package Center.
2. Enable either HTTP (port 5005) or HTTPS (port 5006).
3. URL: `https://<nas-ip>:5006/<shared-folder-name>/`
4. Credentials: Your NAS DSM user credentials.

---

## Local Development: Running a Mock WebDAV Server

For offline development and manual testing, you can spin up a local WebDAV server using Docker in a single command.

### Option A: Using Apache WebDAV Image (Simple & Lightweight)
Run the following command to start a WebDAV server on port `8080` with username `user` and password `pass`:

```bash
docker run -d --name local-webdav \
  -p 8080:80 \
  -e USERNAME=user \
  -e PASSWORD=pass \
  -v $(pwd)/cloud_simulation/webdav:/var/lib/dav/data \
  bytemark/webdav
```

Configure your `private_config.toml` to connect to it locally:
```toml
[webdav_credentials]
url = "http://localhost:8080"
username = "user"
password = "pass"
destination_folder = "MySyncFolder"
enabled = true
```

### Option B: Using Local Nextcloud Instance
To test against a full Nextcloud instance:

```bash
docker run -d --name local-nextcloud \
  -p 8080:80 \
  nextcloud
```
1. Access `http://localhost:8080` to complete the initial setup (create admin account).
2. Use the WebDAV URL: `http://localhost:8080/remote.php/dav/files/<admin-username>/`
