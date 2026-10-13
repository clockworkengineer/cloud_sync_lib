# Dropbox API Setup Guide

This guide provides a step-by-step walkthrough to configure an application in the Dropbox App Console, enable correct permissions (scopes), generate a long-lived refresh token, and configure `cloud_sync_lib`.

---

## Step 1: Create a Dropbox App
1. Go to the [Dropbox App Console](https://www.dropbox.com/developers/apps).
2. Log in with your Dropbox account.
3. Click the **Create app** button.
4. Choose your app settings:
   * **Choose an API**: Select **Scoped access** (required for new apps).
   * **Choose the type of access**: Select **Full Dropbox** (or **App folder** if you only want the daemon to access a specific folder).
   * **Name your app**: Enter a unique name (e.g., `cloud-sync-lib-yourname`).
5. Click **Create app**.

---

## Step 2: Configure Scopes & Permissions
By default, new Dropbox apps do not have file write permissions. You must enable them:
1. In your app dashboard, click on the **Permissions** tab.
2. Under **Files and folders**, check the following boxes:
   * **`files.metadata.write`**
   * **`files.metadata.read`**
   * **`files.content.write`**
   * **`files.content.read`**
3. Scroll to the bottom of the page and click **Submit**.

---

## Step 3: Configure Redirect URIs & Get App Keys
1. Go back to the **Settings** tab.
2. Under **App key**, copy the value. This is your `client_id` for the config file.
3. Under **App secret**, click **Show** and copy the value. This is your `client_secret`.
4. Locate the **Redirect URIs** section:
   * Enter: `http://localhost:8080` (used for local token exchange).
   * Click **Add**.

---

## Step 4: Generate the Refresh Token

An automated helper script is provided in the workspace to capture the authorization code and exchange it for a refresh token automatically.

Run the following command in your terminal:
```bash
python3 scripts/get_dropbox_token.py
```

The script will:
1. Auto-detect your configured `client_id` and `client_secret` under `[dropbox_credentials]` inside `private_config.toml` (or `config.toml`).
2. Open your web browser to sign in and authorize the application.
3. Automatically capture the redirected callback code.
4. Exchange it for a `refresh_token` and save it directly to your configuration files.

---

## Step 5: Configure `private_config.toml`
Open your `private_config.toml` (and `config.toml`) file and update the Dropbox credentials:

```toml
[dropbox_credentials]
client_id = "YOUR_APP_KEY"
client_secret = "YOUR_APP_SECRET"
refresh_token = "YOUR_REFRESH_TOKEN"
```
