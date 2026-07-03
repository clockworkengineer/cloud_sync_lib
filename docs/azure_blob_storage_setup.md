# Azure Blob Storage Setup Guide

This guide describes how to configure the Azure Blob Storage backend in `cloud_sync_lib`, set up resources in the Azure Portal, and run the Azurite local emulator for offline development and testing.

---

## Configuration in `private_config.toml`

Open your `private_config.toml` (and `config.toml`) file and add the `[azure_blob_credentials]` section:

```toml
[azure_blob_credentials]
# The name of the target blob container
container = "your-container-name"
# Azure Storage Account Name
account_name = "youraccountname"
# Azure Storage Account Access Key
account_key = "YOUR_ACCOUNT_ACCESS_KEY"
# Custom endpoint URL (omit for real Azure Cloud; required for Azurite emulator)
endpoint = "http://127.0.0.1:10000/youraccountname"
# Optional destination folder/prefix path inside the container
destination_folder = "MySyncFolder"
# Enable/disable the Azure Blob sync client
enabled = true
# Enable/disable deletion syncing
sync = true
```

---

## Setup 1: Official Microsoft Azure Portal

To connect to official Azure Blob Storage, you need an Azure Subscription, a Storage Account, and a Blob Container:

### 1. Create a Storage Account
1. Log in to the [Azure Portal](https://portal.azure.com/).
2. Search for **Storage accounts** and click **Create**.
3. Select your **Subscription** and **Resource group** (create one if necessary).
4. Enter a globally unique **Storage account name** (must be lowercase letters and numbers only, 3-24 characters).
5. Choose your preferred **Region** and select **Standard** performance (recommended for sync backups).
6. Under Redundancy, select **Locally-redundant storage (LRS)** for a low-cost testing option.
7. Click **Review + create** and then **Create**.

### 2. Create a Blob Container
1. Once the Storage Account deployment is complete, go to the resource.
2. Under the **Data storage** menu in the sidebar, click **Containers**.
3. Click **+ Container** at the top.
4. Enter a name (e.g. `sync-container`). Keep access level as **Private (no anonymous access)**.
5. Click **Create**.

### 3. Retrieve Access Keys
1. In your Storage Account sidebar menu, scroll down to **Security + networking** and click **Access keys**.
2. Click **Show** next to **Key 1** or **Key 2**.
3. Copy the **Storage account name** and either the **Key** value.

---

## Local Development: Running Azurite (Local Emulator)

Azurite is Microsoft's official, open-source storage emulator. It is fully compatible with Azure Blob Storage APIs.

### 1. Start Azurite via Docker
Run the following docker command to start the Azurite Blob Service container on port `10000`:

```bash
docker run -d --name local-azurite \
  -p 10000:10000 \
  -v ~/azurite_data:/data \
  --restart unless-stopped \
  mcr.microsoft.com/azure-storage/azurite azurite-blob --blobHost 0.0.0.0 --blobPort 10000
```

### 2. Connection Configuration
Azurite uses a default well-known account name and key for local development:
* **Account Name**: `devstoreaccount1`
* **Account Key**: `Eby8vdM0gThJrgY4RbrYGZUXmin47115CuvNytYiG8pxUXSBn9126Ek85GBv39i1EdjK71S1Yu08dBQ1g==`

Configure your `private_config.toml` as follows:

```toml
[azure_blob_credentials]
container = "sync-container"
account_name = "devstoreaccount1"
account_key = "Eby8vdM0gThJrgY4RbrYGZUXmin47115CuvNytYiG8pxUXSBn9126Ek85GBv39i1EdjK71S1Yu08dBQ1g=="
endpoint = "http://127.0.0.1:10000/devstoreaccount1"
destination_folder = "MySyncFolder"
enabled = true
```

*(Note: You will need to create the container `sync-container` inside Azurite, which the library can automatically initialize or do via a desktop explorer tool).*

---

## Viewing Storage Contents

### 1. Microsoft Azure Storage Explorer (Recommended)
This is the official, free desktop application for managing Azure cloud storage:
1. Download and install [Azure Storage Explorer](https://azure.microsoft.com/en-us/products/storage-explorer/).
2. Open the application.
3. Click the plug icon on the left panel (**Connect to Azure Storage**).
4. Select **Storage account or service**.
5. Select **Connection string** or **Account name and key**.
   - To connect to the local Azurite emulator, choose **Local storage emulator** or enter the connection string:
     `DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM0gThJrgY4RbrYGZUXmin47115CuvNytYiG8pxUXSBn9126Ek85GBv39i1EdjK71S1Yu08dBQ1g==;BlobEndpoint=http://127.0.0.1:10000/devstoreaccount1;`
6. Browse and manage folders/blobs in a visual hierarchy.

### 2. Azure Portal
For the live cloud storage, navigate to your Storage Account -> **Containers** -> Select your container. The Azure Portal has a built-in **Storage Explorer (preview)** tab in the sidebar or directly within the container view.
