# Raspberry Pi SFTP Server Setup Guide

A complete guide to configuring your Raspberry Pi as a secure, dedicated SFTP (SSH File Transfer Protocol) server to receive backups and sync files from this Cloud Sync daemon.

---

## 1. Enable SSH on Raspberry Pi

By default, SSH is disabled on Raspberry Pi OS. You can enable it through either the desktop interface or terminal:

### Method A: Via Command Line (Recommended)
1. Open a terminal on your Pi (or connect directly via keyboard/monitor).
2. Run the Raspberry Pi Configuration Tool:
   ```bash
   sudo raspi-config
   ```
3. Navigate to **Interface Options** > **SSH**.
4. Select **Yes** to enable the SSH server.
5. Exit the tool and reboot the Pi if prompted.

---

## 2. Set Up a Dedicated SFTP User

For security, it is highly recommended to create a dedicated user restricted to SFTP, rather than using the main admin account.

### Step 1: Create the User
Create a new user named `syncuser`:
```bash
sudo adduser syncuser
```
*(Enter a secure password when prompted).*

### Step 2: Create the Sync Directory
Create the target backup directory where files will be synchronized:
```bash
sudo mkdir -p /srv/sftp/syncuser/uploads
sudo chown syncuser:syncuser /srv/sftp/syncuser/uploads
```

---

## 3. Secure & Restrict User to SFTP (Chroot Jail)

To prevent the SFTP user from accessing shell terminals or viewing system files, restrict them to their home directory.

### Step 1: Edit SSH Configuration
Open the SSH daemon configuration file on the Pi:
```bash
sudo nano /etc/ssh/sshd_config
```

### Step 2: Append SFTP Directives
Go to the very bottom of the file and add:

```text
# SFTP Restriction configuration
Match User syncuser
    ForceCommand internal-sftp
    ChrootDirectory /srv/sftp/syncuser
    PermitTunnel no
    AllowAgentForwarding no
    AllowTcpForwarding no
    X11Forwarding no
```

*Note: The chroot directory `/srv/sftp/syncuser` must be owned by `root` and not writeable by the user for security. The actual files must be written under the `/uploads` directory.*

### Step 3: Configure Root Permissions on the Chroot Directory
```bash
sudo chown root:root /srv/sftp/syncuser
sudo chmod 755 /srv/sftp/syncuser
```

### Step 4: Restart SSH Service
Apply the configuration changes:
```bash
sudo systemctl restart ssh
```

---

## 4. Connection Configuration

Identify your Raspberry Pi's local IP address by running `hostname -I` on the Pi. Then update your `private_config.toml` on your local host:

```toml
[sftp_credentials]
host = "192.168.1.50"             # Your Raspberry Pi IP Address
port = 22
username = "syncuser"
password = "syncuser_password"
private_key_path = ""
destination_folder = "uploads"    # Target uploads folder inside the chroot jail
enabled = true
sync = true
```
