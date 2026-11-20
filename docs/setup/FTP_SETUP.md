# FTP / FTPS Setup Guide (RFC 959 & RFC 4217)

Cloud Sync includes native, zero-dependency support for FTP (RFC 959) and FTPS (RFC 4217 with TLS/SSL), enabling seamless synchronization with shared web hosts (cPanel, Plesk), local NAS devices (Synology, QNAP, TrueNAS), security cameras, and embedded servers.

---

## 1. Prerequisites

- An accessible FTP or FTPS server.
- Server hostname or IP address.
- FTP Port (typically `21` for standard FTP/FTPS control, or custom).
- Username and password (or anonymous credentials if permitted).

---

## 2. Configuration (`config.toml`)

Add or update the `[ftp_credentials]` section in your `config.toml`:

```toml
[ftp_credentials]
# Host address or domain name
host = "ftp.example.com"

# Port (default is 21 if omitted)
port = 21

# Username
username = "my_ftp_user"

# Password (can also be retrieved securely via system keyring)
password = "my_secure_ftp_password"

# Set to true for FTPS (TLS/SSL encryption), false for plain FTP
secure = false

# Optional destination folder on the remote FTP root
destination_folder = "backups/cloud_sync"

# Optional sync mode: "two_way", "one_way", or "one_way_no_deletions"
sync_mode = "two_way"

# Optional client-side encryption password (AES-256-GCM)
# encryption_password = "your-master-secret"

# Optional bandwidth limits (KB/s)
# max_upload_rate = 500
# max_download_rate = 1000
```

---

## 3. Features Supported

| Feature | Support | Description |
| :--- | :---: | :--- |
| **Passive Mode (PASV)** | ✅ | Firewall-friendly data connection establishment. |
| **Binary Mode (TYPE I)** | ✅ | Guaranteed bit-for-bit lossless binary transfer. |
| **Directory Navigation (MKD/RMD)** | ✅ | Recursive directory creation and removal. |
| **Server-Side Move (RNFR/RNTO)** | ✅ | Instant server-side rename without re-transferring bytes. |
| **Two-Way Synchronization** | ✅ | Full bidirectional syncing with timestamp comparison. |
| **Zero-Knowledge Encryption** | ✅ | Optional AES-256-GCM wrapper before upload. |
| **Client-Side Throttling** | ✅ | Integrated with Token Bucket & Bandwidth Schedules. |

---

## 4. Testing Connection

You can verify your FTP connectivity and run integrity checks via the daemon control command:

```bash
# Check FTP provider status
cargo run -p cloud_sync_daemon -- status

# Run integrity verification against FTP remote
cargo run -p cloud_sync_daemon -- verify

# Preview sync actions (dry-run)
cargo run -p cloud_sync_daemon -- preview
```
