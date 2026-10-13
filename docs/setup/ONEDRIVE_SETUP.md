# OneDrive API Setup Guide

This guide provides a step-by-step walkthrough to configure an application in the Microsoft Azure Portal, enable OneDrive API access (Microsoft Graph), add permissions, generate a refresh token, and configure `cloud_sync_lib`.

---

## Step 1: Register an Application on Azure Portal
1. Go to the [Microsoft Entra admin center (Azure Portal)](https://entra.microsoft.com/) or [Azure App Registrations](https://portal.azure.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade).
2. Log in with your Microsoft account (personal, work, or school).
3. Click **+ New registration** at the top.
4. Fill in the App Registration details:
   * **Name**: `cloud_sync_lib`
   * **Supported account types**: Select **"Accounts in any organizational directory (Any Microsoft Entra ID tenant - Multitenant) and personal Microsoft accounts (e.g. Skype, Xbox)"** (essential for personal OneDrive support).
   * **Redirect URI**: 
     * Select **Web** in the dropdown.
     * Enter: `http://localhost:8080` (used for local token retrieval).
5. Click **Register** at the bottom.

---

## Step 2: Get Application (Client) ID
1. Once the application is created, you will be taken to the **Overview** page.
2. Copy the **Application (client) ID**. This is your `client_id` for the config file.

---

## Step 3: Create a Client Secret
1. Navigate to **Certificates & secrets** in the left sidebar.
2. Under the **Client secrets** tab, click **+ New client secret**.
3. Enter a description (e.g., `Local Dev Secret`) and choose an expiration period.
4. Click **Add**.
5. **CRITICAL**: Copy the **Value** of the client secret immediately. *(This value will be hidden forever once you leave the page).* This is your `client_secret`.

---

## Step 4: Configure API Permissions
1. Navigate to **API permissions** in the left sidebar.
2. Click **+ Add a permission**.
3. Select **Microsoft Graph**.
4. Choose **Delegated permissions**.
5. Search for and select the following permissions:
   * **`Files.ReadWrite`** (or **`Files.ReadWrite.All`**) - To read, create, and delete files on OneDrive.
   * **`offline_access`** - Required to obtain a `refresh_token`.
6. Click **Add permissions** at the bottom.

---

## Step 5: Generate the Refresh Token

An automated helper script is provided in the workspace to capture the authorization code and exchange it for a refresh token automatically.

Run the following command in your terminal:
```bash
python3 scripts/get_onedrive_token.py
```

The script will:
1. Auto-detect your configured `client_id` and `client_secret` under `[onedrive_credentials]` inside `private_config.toml` (or `config.toml`).
2. Open your web browser to sign in and authorize the application.
3. Automatically capture the redirected callback code.
4. Exchange it for a `refresh_token` and save it directly to your configuration files.

---

## Step 6: Configure `private_config.toml`
Open your `private_config.toml` (and `config.toml`) file and update the OneDrive fields:

```toml
[onedrive_credentials]
client_id = "YOUR_CLIENT_ID"
client_secret = "YOUR_CLIENT_SECRET"
refresh_token = "YOUR_REFRESH_TOKEN"
```
