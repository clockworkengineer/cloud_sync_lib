# Google Drive API Setup Guide

This guide provides a step-by-step walkthrough to configure a Google Cloud project, enable the Google Drive API, configure OAuth 2.0 consent settings, add test users, generate a refresh token, and configure `cloud_sync_lib`.

---

## Step 1: Create a Google Cloud Project
1. Open the [Google Cloud Console](https://console.cloud.google.com/).
2. Click the project dropdown menu in the top-left corner and click **New Project**.
3. Name your project (e.g., `Cloud Sync Library`) and click **Create**.
4. Make sure your newly created project is selected in the top dropdown.

---

## Step 2: Enable the Google Drive API
1. Search for **Google Drive API** in the top search bar.
2. Click on the **Google Drive API** page from the search results.
3. Click the blue **Enable** button.

---

## Step 3: Configure the OAuth Consent Screen
Before you can generate credentials, you need to configure the consent screen:
1. Navigate to **APIs & Services** > **OAuth consent screen** in the left sidebar.
2. Choose **External** as the User Type and click **Create**.
3. Fill in the **App Information**:
   * **App name**: `cloud_sync_lib`
   * **User support email**: Choose your email address.
   * **Developer contact info**: Enter your email address.
   * Click **Save and Continue**.
4. **Scopes**:
   * Click **Add or Remove Scopes**.
   * Search for `drive` and select **`https://www.googleapis.com/auth/drive`** (this scope allows seeing, editing, creating, and deleting Google Drive files).
   * Click **Update** at the bottom, then click **Save and Continue**.
5. **Test Users (Crucial step to avoid the "Access Blocked" error)**:
   * Under **Test users**, click **Add Users**.
   * Enter the **Gmail address** of the account you plan to log in and sync files with.
   * Click **Add** and then **Save and Continue**.

---

## Step 4: Create OAuth 2.0 Credentials
1. Navigate to **APIs & Services** > **Credentials** in the left sidebar.
2. Click **+ Create Credentials** at the top and select **OAuth client ID**.
3. Select **Web application** (or **Desktop app**) as the Application type. *(Web application is recommended if using the OAuth Playground).*
4. Under **Authorized redirect URIs**, click **+ Add URI** and add:
   * `https://developers.google.com/oauthplayground` (used if generating tokens via the Playground).
   * `http://localhost` (used for local redirects).
5. Click **Create**.
6. Copy the **Client ID** and **Client Secret** that appear.

---

## Step 5: Generate the Refresh Token (Using OAuth Playground)
1. Navigate to the [Google OAuth 2.0 Playground](https://developers.google.com/oauthplayground/).
2. Click the **Gear icon (settings)** in the top right corner.
3. Check the box for **"Use your own OAuth credentials"**.
4. Enter your custom **OAuth Client ID** and **OAuth Client Secret**, then close the settings popup.
5. In the left panel under **Step 1: Select & authorize APIs**:
   * Find **Drive API v3** in the list, expand it, and check **`https://www.googleapis.com/auth/drive`**.
   * Click the blue **Authorize APIs** button.
6. Sign in with the Google Account you added as a **Test User** in Step 3.
7. Bypass the warning by clicking **Advanced** > **Go to cloud_sync_lib (unsafe)**, then click **Allow**.
8. In the left panel under **Step 2**:
   * Click **Exchange authorization code for tokens**.
9. Copy the **Refresh token** from the output.

---

## Step 6: Configure and Verify
1. Put the credentials into your local `config.toml` file:
   ```toml
   [google_credentials]
   client_id = "YOUR_CLIENT_ID"
   client_secret = "YOUR_CLIENT_SECRET"
   refresh_token = "YOUR_REFRESH_TOKEN"
   ```
2. Run the tests in the workspace to verify the library can write/read files on Google Drive:
   ```bash
   cargo test -- --nocapture
   ```
