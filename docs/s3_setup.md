# Amazon S3 & S3-Compatible Storage Setup Guide

This guide describes how to configure the Amazon S3 backend in `cloud_sync_lib`, set up IAM permissions on AWS, configure S3-compatible alternatives (Cloudflare R2, MinIO), and run a local MinIO server for testing.

---

## Configuration in `private_config.toml`

Open your `private_config.toml` (and `config.toml`) file and add the `[s3_credentials]` section:

```toml
[s3_credentials]
# The name of the target S3 bucket
bucket = "your-bucket-name"
# The region where the bucket is located (e.g. us-east-1)
region = "us-east-1"
# AWS / Provider access credentials
access_key_id = "YOUR_ACCESS_KEY_ID"
secret_access_key = "YOUR_SECRET_ACCESS_KEY"
# Custom endpoint URL (omit for real AWS S3; required for R2, MinIO, B2)
endpoint = "https://<account-id>.r2.cloudflarestorage.com"
# Optional destination folder/prefix path inside the bucket
destination_folder = "MySyncFolder"
# Enable/disable the S3 sync client
enabled = true
```

---

## Setup 1: Official Amazon S3 (AWS)

To connect to official Amazon S3, you must create a bucket and an IAM User with API keys:

### 1. Create an S3 Bucket
1. Open the [Amazon S3 Console](https://console.aws.amazon.com/s3/).
2. Click **Create bucket**.
3. Choose a unique name and select your preferred **Region**.
4. Configure ownership and block public access settings (recommended to keep blocked). Click **Create bucket**.

### 2. Create an IAM User & Generate Keys
1. Open the [IAM Console](https://console.aws.amazon.com/iam/).
2. Go to **Users** in the left sidebar and click **Create user**.
3. Name the user (e.g. `cloud_sync_user`) and proceed.
4. Under **Set permissions**, choose **Attach policies directly**.
5. Search for and select **`AmazonS3FullAccess`** (or create a custom policy restricting access to your specific bucket).
6. Click **Next** and **Create user**.
7. Select the newly created user, go to the **Security credentials** tab, and click **Create access key**.
8. Select **Application running outside AWS** and copy your **Access Key ID** and **Secret Access Key** immediately.

---

## Setup 2: Cloudflare R2 (S3-Compatible)

Cloudflare R2 offers zero-egress fees and is fully S3-compatible:

1. Log in to your Cloudflare Dashboard and navigate to **R2**.
2. Click **Create bucket** and name it.
3. In the right sidebar of the R2 homepage, click **Manage R2 API Tokens**.
4. Click **Create API token** and set permissions to **Admin Read & Write**.
5. Copy the **Access Key ID**, **Secret Access Key**, and the **Jurisdiction-specific endpoint URL** (looks like `https://<account-id>.r2.cloudflarestorage.com`).
6. Configure `private_config.toml` using `region = "auto"`, your endpoint URL, and keys.

---

## Local Development: Running a Local MinIO Server

To test S3 synchronization offline without hitting cloud endpoints, you can run a MinIO container.

### 1. Start MinIO via Docker
Run this command to start MinIO on port `9000` (API) and `9001` (Web Console):

```bash
docker run -d --name local-minio \
  -p 9000:9000 \
  -p 9001:9001 \
  -e MINIO_ROOT_USER=minioadmin \
  -e MINIO_ROOT_PASSWORD=minioadmin \
  -v ~/minio_data:/data \
  --restart unless-stopped \
  minio/minio server /data --console-address ":9001"
```

### 2. Create a Bucket
1. Open `http://localhost:9001` in your browser.
2. Log in with user `minioadmin` and password `minioadmin`.
3. Go to **Buckets** > **Create Bucket** and name it `test-bucket`.

### 3. Connection Configuration
Configure your `private_config.toml` to target your local MinIO instance:

```toml
[s3_credentials]
bucket = "test-bucket"
region = "us-east-1"
access_key_id = "minioadmin"
secret_access_key = "minioadmin"
endpoint = "http://localhost:9000"
destination_folder = "MySyncFolder"
enabled = true
```
