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
Dropbox access tokens expire after 4 hours. You must request a **refresh token** (using `token_access_type=offline`) for long-term sync:

### 1. Construct the Authorization URL
Open a browser and navigate to the following URL (replace `YOUR_APP_KEY` with your App Key / Client ID):

```text
https://www.dropbox.com/oauth2/authorize?
client_id=YOUR_APP_KEY&
response_type=code&
redirect_uri=http://localhost:8080&
token_access_type=offline
```

### 2. Authorize and copy the code
1. Click **Continue** and authorize the app.
2. The browser will redirect to a page that fails to load (e.g., `http://localhost:8080/?code=A1B2C3D...`).
3. Copy the value after `code=` in the browser's address bar.

### 3. Exchange the Code for a Refresh Token
Run this curl command in your terminal (replacing placeholders with your values):

```bash
curl -X POST https://api.dropboxapi.com/oauth2/token \
  -d code="YOUR_AUTHORIZATION_CODE" \
  -d grant_type="authorization_code" \
  -d redirect_uri="http://localhost:8080" \
  -u "YOUR_APP_KEY:YOUR_APP_SECRET"
```

This will return a JSON response containing the `"refresh_token"`.

---

## Step 5: Configure `private_config.toml`
Open your `private_config.toml` (and `config.toml`) file and update the Dropbox credentials:

```toml
[dropbox_credentials]
client_id = "YOUR_APP_KEY"
client_secret = "YOUR_APP_SECRET"
refresh_token = "YOUR_REFRESH_TOKEN"
```
