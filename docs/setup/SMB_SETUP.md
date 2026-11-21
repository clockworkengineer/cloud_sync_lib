# SMB / CIFS Network Share Setup Guide

Cloud Sync supports synchronizing directly with SMB / CIFS network shares (Windows File Sharing, Samba servers, Synology/QNAP/TrueNAS appliances), matching the network share capabilities of **FreeFileSync**, **GoodSync**, and **Rclone (`smb`)**.

---

## 1. Overview

- On **Windows**, Cloud Sync resolves UNC network share paths (`\\server\share\...`) directly with atomic temporary file staging (`.smb_tmp`) to prevent partial writes over LAN/VPN links.
- On **Linux / macOS / Android**, Cloud Sync targets either `/mnt/smb/<server>/<share>` or an explicit `mount_path` override pointing to your CIFS mount point.
- Supports server-side rename (`SERVER_SIDE_MOVE`), server-side copy (`SERVER_SIDE_COPY`), and MD5 checksum verification (`CHECKSUM_MD5`).

---

## 2. Configuration (`config.toml`)

Add the `[smb_credentials]` section to your `config.toml`:

```toml
[smb_credentials]
# Hostname or IP address of the SMB server / NAS
server = "192.168.1.50"

# SMB share name
share = "backups"

# Optional authentication details
username = "nas_user"
password = "nas_password"
workgroup = "WORKGROUP"

# Optional explicit mount path or UNC path override
# mount_path = "\\\\192.168.1.50\\backups"

# Optional subfolder inside the share
destination_folder = "workstation_sync"

# Optional sync mode ("two_way", "one_way", "one_way_no_deletions")
sync_mode = "two_way"

# Optional transparent compression (CLZ1) and AES-256-GCM encryption
# compress = true
# encryption_password = "nas-encryption-password"
```
