# IPFS / Pinning Service Setup Guide

This guide describes how to configure the IPFS Pinning Service (e.g. Pinata) backend in `cloud_sync_lib`, obtain API credentials, and run local testing.

---

## Configuration in `private_config.toml`

Open your `private_config.toml` (and `config.toml`) file and add the `[ipfs_credentials]` section:

```toml
[ipfs_credentials]
# Pinata or IPFS Pinning Service JWT Bearer Token
jwt_token = "YOUR_PINNING_SERVICE_JWT_TOKEN"
# Custom Pinning API endpoint URL (optional, defaults to Pinata's API https://api.pinata.cloud)
endpoint = "https://api.pinata.cloud"
# Gateway URL to fetch pinned content (optional, defaults to https://gateway.pinata.cloud/ipfs/)
gateway_url = "https://gateway.pinata.cloud/ipfs/"
# Enable/disable the IPFS Pinning Service client
enabled = true
# Enable/disable deletion (unpinning) syncing
sync = true
```

---

## Setup 1: Official Pinata Setup (Production)

To connect to official Pinata pinning API, you need a Pinata account and a JWT token:

### 1. Create a Pinata Account
1. Sign up or log in to the [Pinata Portal](https://www.pinata.cloud/).

### 2. Generate API Keys & JWT Token
1. In the Pinata dashboard, navigate to **API Keys** in the sidebar.
2. Click **New Key**.
3. Under **Key Permissions**, select **Admin** (recommended to allow pinning, unpinning, and querying files) or customize scopes specifically.
4. Name the key (e.g. `ClockworkSyncKey`).
5. Click **Create Key**.
6. Copy the **JWT** (this is a long token used as a bearer authorization token). *Note: This will not be shown again.*

---

## Local Development & Testing

Since IPFS Pinning Services utilize a custom REST API, offline integration testing is handled via the provider's **Simulation Mode**.

When `jwt_token` is omitted (or set to an empty string) in `private_config.toml`, the daemon automatically defaults to simulation mode, redirecting synchronization traffic to the local directory:
`./cloud_simulation/ipfs`

---

## Viewing Storage Contents

### 1. Pinata File Manager
For a quick view of your pinned items, their CIDs (Content Identifiers), and names, navigate to the **Files** section in the Pinata Web Console.

### 2. IPFS Gateway Resolution
Since IPFS is content-addressed, you can view or download any of your pinned files from any public gateway using its CID. For example:
`https://ipfs.io/ipfs/YOUR_FILE_CID` or `https://gateway.pinata.cloud/ipfs/YOUR_FILE_CID`
