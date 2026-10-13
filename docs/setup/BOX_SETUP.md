# Box Provider Setup Guide

This guide describes how to configure Box cloud storage integration by creating a Box Developer App and obtaining OAuth 2.0 credentials.

---

## Step 1: Create a Box Developer Application

To connect to Box via OAuth 2.0, you must register a custom integration application in the Box Developer Console.

1. Log into the [Box Developer Console](https://developer.box.com/).
2. Click **Create New App**.
3. Select **Custom App** and click **Next**.
4. Choose **User Authentication (OAuth 2.0)** as your authentication method.
5. Provide an application name (e.g. `PiCloudSyncClient`) and click **Create App**.

---

## Step 2: Configure App Settings & Redirect URIs

Once the app is created, configure the security settings to allow the client to authenticate.

1. In the application settings dashboard under **Configuration**:
   - Locate the **OAuth 2.0 Redirect URIs** section.
   - Add a redirect URI. For local sync configurations, this is typically:
     ```text
     http://localhost:8080/oauth/callback
     ```
2. Scroll down to **Application Scopes** and ensure the following permissions are checked:
   - **Read and write all files and folders stored in Box**
3. Click **Save Changes** at the top right of the page.

---

## Step 3: Retrieve Client Credentials & Refresh Token

1. Scroll to the **OAuth 2.0 Credentials** section.
2. Copy the **Client ID** and **Client Secret**.
3. Obtain the long-lived **Refresh Token** via the automated local script:
   ```bash
   python3 scripts/get_box_token.py
   ```

The script will:
1. Auto-detect your configured `client_id` and `client_secret` under `[box_credentials]` inside `private_config.toml` (or `config.toml`).
2. Open your web browser to sign in and authorize the application.
3. Automatically capture the redirected callback code.
4. Exchange it for a `refresh_token` and save it directly to your configuration files.

---

## Step 4: Connection Configuration

Update your `private_config.toml` (or `.toml`) file with your Box credentials:

```toml
[box_credentials]
client_id = "your_box_client_id"
client_secret = "your_box_client_secret"
refresh_token = "your_box_refresh_token"
destination_folder = "MySyncFolder"   # Optional: Folder name in Box root
enabled = true
sync = true
```

*Note: If the configured `destination_folder` (e.g. `MySyncFolder`) does not exist inside your Box account, the sync daemon will automatically create it in the root folder during the first upload.*

---

## Step 5: Test Connection

Run the connection check utility to verify that the credentials and permissions are configured correctly:

```bash
cargo run --bin test_nextcloud # Verify other backends, or use the Box client directly
```
