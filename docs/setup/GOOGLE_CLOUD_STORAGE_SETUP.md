# Google Cloud Storage (GCS) Setup Guide

This guide describes how to configure the Google Cloud Storage backend in `cloud_sync_lib`, set up a bucket and service account key in the Google Cloud Console, and run the `fake-gcs-server` emulator for local testing.

---

## Configuration in `private_config.toml`

Open your `private_config.toml` (and `config.toml`) file and add the `[gcs_credentials]` section:

```toml
[gcs_credentials]
# The name of the target Google Cloud Storage bucket
bucket = "your-gcs-bucket-name"
# Path to the Service Account JSON credentials file
service_account_key_path = "/path/to/service-account-key.json"
# Custom endpoint URL (omit for real Google Cloud; required for local fake-gcs-server)
endpoint = "http://127.0.0.1:4443"
# Optional destination folder/prefix path inside the bucket
destination_folder = "MySyncFolder"
# Enable/disable the GCS sync client
enabled = true
# Enable/disable deletion syncing
sync = true
```

---

## Setup 1: Official Google Cloud Console (Production)

To connect to official Google Cloud Storage, you need a Google Cloud Project, an enabled Storage API, a Storage Bucket, and a Service Account key file:

### 1. Create a GCP Project & Enable API
1. Open the [Google Cloud Console](https://console.cloud.google.com/).
2. Select or create a project at the top of the page.
3. Open the navigation menu, go to **APIs & Services** > **Library**.
4. Search for **Google Cloud Storage JSON API** and click **Enable**.

### 2. Create a Cloud Storage Bucket
1. Navigate to **Cloud Storage** > **Buckets** in the sidebar.
2. Click **Create** at the top.
3. Enter a globally unique bucket name (e.g. `my-cloud-sync-bucket`).
4. Select a Location type (e.g. **Region** for low cost and high performance).
5. Choose a default storage class (e.g. **Standard** for active synchronization).
6. Under control access, uncheck **Enforce public access prevention on this bucket** (or keep it checked for private security, recommended).
7. Click **Create**.

### 3. Create a Service Account & Generate JSON Key
1. Go to **IAM & Admin** > **Service Accounts** in the sidebar.
2. Click **Create Service Account** at the top.
3. Enter a name (e.g. `gcs-sync-agent`) and click **Create and Continue**.
4. Under roles, select **Storage Object Admin** (allows upload, download, delete, and list operations on objects). Click **Continue** and **Done**.
5. Click on the newly created Service Account from the list.
6. Navigate to the **Keys** tab at the top.
7. Click **Add Key** > **Create new key**. Select **JSON** format and click **Create**.
8. Save the downloaded JSON file securely on your machine (e.g. `~/.gcp/gcs-key.json`) and configure the path in `private_config.toml`.

---

## Local Development: Running `fake-gcs-server` Emulator

To test GCS synchronization offline without using real Google Cloud credits, you can run the popular `fake-gcs-server` emulator.

### 1. Start fake-gcs-server via Docker
Run the following docker command to start the GCS emulator on port `4443`:

```bash
docker run -d --name local-gcs \
  -p 4443:4443 \
  -v ~/gcs_data:/data \
  --restart unless-stopped \
  fsouza/fake-gcs-server -scheme http -port 4443
```

### 2. Create a Bucket (Local)
You can create a local bucket named `test-bucket` inside the emulator by running a simple curl request:

```bash
curl -X POST http://127.0.0.1:4443/storage/v1/b?project=test-project \
  -H "Content-Type: application/json" \
  -d '{"name": "test-bucket"}'
```

### 3. Connection Configuration
Configure your `private_config.toml` to point to the local emulator. When targeting an emulator endpoint with `http`, the service account key check can be bypassed (or configured with dummy values):

```toml
[gcs_credentials]
bucket = "test-bucket"
service_account_key_path = ""
endpoint = "http://127.0.0.1:4443"
destination_folder = "MySyncFolder"
enabled = true
```

---

## Viewing Storage Contents

### 1. Google Cloud Console
For production buckets, navigate to **Cloud Storage** > **Buckets** > Click on your bucket name to browse files and view metadata.

### 2. Desktop GUI Clients (Windows / macOS)
* **[Cyberduck](https://cyberduck.io/)**: Create a connection, select **Google Cloud Storage** as the protocol, and upload your service account JSON file under authentication.

### 3. Command Line (gcloud CLI)
Run standard gsutil or gcloud command line operations to manage bucket files:
```bash
gcloud storage ls gs://<your-bucket-name> --recursive
```
