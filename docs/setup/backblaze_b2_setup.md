# Backblaze B2 Storage Setup Guide

This guide describes how to configure the Backblaze B2 backend in `cloud_sync_lib`, set up buckets and application keys in the Backblaze Web Portal, and run local testing.

---

## Configuration in `private_config.toml`

Open your `private_config.toml` (and `config.toml`) file and add the `[b2_credentials]` section:

```toml
[b2_credentials]
# The name of the target Backblaze B2 bucket
bucket = "your-b2-bucket-name"
# Backblaze B2 Key ID (e.g., 0047bca883...)
key_id = "YOUR_B2_KEY_ID"
# Backblaze B2 Application Key (e.g., K004...)
application_key = "YOUR_APPLICATION_KEY"
# Custom endpoint URL (optional, used for mock endpoints during testing)
endpoint = "https://api.backblazeb2.com"
# Optional destination folder/prefix path inside the bucket
destination_folder = "MySyncFolder"
# Enable/disable the Backblaze B2 sync client
enabled = true
# Enable/disable deletion syncing
sync = true
```

---

## Setup 1: Official Backblaze B2 Portal (Production)

To connect to official Backblaze B2 using its native API, you need a Backblaze account, a B2 bucket, and an Application Key:

### 1. Create a Backblaze B2 Bucket
1. Log in to the [Backblaze Portal](https://www.backblaze.com/b2/sign-in.html).
2. Navigate to **B2 Cloud Storage** > **Buckets** in the sidebar.
3. Click **Create a Bucket**.
4. Enter a unique bucket name.
5. Set bucket type to **Private** (recommended for secure backups).
6. Enable encryption if desired (default settings are usually fine).
7. Click **Create a Bucket**.

### 2. Generate Application Keys
1. Navigate to **B2 Cloud Storage** > **Application Keys** in the sidebar.
2. Scroll down and click **Add a New Application Key**.
3. Name the key (e.g. `gcs-sync-agent`).
4. Under **Allow Access to Bucket(s)**, select your newly created bucket (recommended for security) or select **All** if needed.
5. Set **Type of Access** to **Read and Write**.
6. Click **Create New Key**.
7. Copy the **keyId** and **applicationKey** immediately. *Note: The applicationKey will not be displayed again.*

---

## Local Development & Testing

Since Backblaze B2 utilizes a custom HTTPS JSON REST API, offline integration testing is handled via the provider's **Simulation Mode**.

When `key_id` or `application_key` are omitted (or set to empty strings) in `private_config.toml`, the daemon automatically defaults to simulation mode, redirecting synchronization traffic to the local directory:
`./cloud_simulation/b2`

---

## Viewing Storage Contents

### 1. Backblaze Web Console
For a quick view of your files, navigate to **Buckets** in the Backblaze portal, and click **Browse Files** next to your bucket name.

### 2. Desktop GUI Clients (Windows / macOS)
* **[Cyberduck](https://cyberduck.io/)**: Create a connection, choose **Backblaze B2** as the protocol, enter your `keyId` as the Access Key, and your `applicationKey` as the Secret Key.
* **[Mountain Duck](https://mountainduck.io/)**: Mounts B2 storage directly as a local disk drive using your Application Keys.
