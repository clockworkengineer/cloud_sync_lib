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
Since OneDrive uses standard OAuth 2.0, you can use a manual browser flow to get your refresh token:

### 1. Construct the Authorization URL
Open a browser and navigate to the following URL (replace `YOUR_CLIENT_ID` with your actual Application Client ID):

```text
https://login.microsoftonline.com/common/oauth2/v2.0/authorize?
client_id=YOUR_CLIENT_ID&
scope=https://graph.microsoft.com/Files.ReadWrite.All%20offline_access&
response_type=code&
redirect_uri=http://localhost:8080&
response_mode=query
```

### 2. Authorize and copy the code
1. Complete the login and grant permissions.
2. The browser will redirect to a page that fails to load (e.g., `http://localhost:8080/?code=M.R3_551...`).
3. Copy the entire value after `code=` in the browser's address bar.

### 3. Exchange the Code for a Refresh Token
Run this curl command in your terminal (replacing placeholders with your values):

```bash
curl -X POST https://login.microsoftonline.com/common/oauth2/v2.0/token \
  -d client_id="YOUR_CLIENT_ID" \
  -d client_secret="YOUR_CLIENT_SECRET" \
  -d code="YOUR_AUTHORIZATION_CODE" \
  -d redirect_uri="http://localhost:8080" \
  -d grant_type="authorization_code"
```

Google/Microsoft will return a JSON response containing the `"refresh_token"`.

---

## Step 6: Configure `private_config.toml`
Open your `private_config.toml` (and `config.toml`) file and update the OneDrive fields:

```toml
[onedrive_credentials]
client_id = "YOUR_CLIENT_ID"
client_secret = "YOUR_CLIENT_SECRET"
refresh_token = "YOUR_REFRESH_TOKEN"
```
