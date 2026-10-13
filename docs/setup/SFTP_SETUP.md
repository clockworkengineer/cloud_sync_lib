# SFTP Provider Setup Guide

This guide describes how to configure and verify synchronization using the SFTP (SSH File Transfer Protocol) storage provider.

---

## Setup 1: Authentication Methods

The SFTP provider supports two authentication methods:

### Method A: Key-Based Authentication (Recommended)
This is the most secure method.

1. Ensure your SSH public key is added to the remote server's `authorized_keys` file (typically at `~/.ssh/authorized_keys`).
2. Identify the absolute path to your local private key (e.g., `/home/username/.ssh/id_rsa`).
3. Configure `private_config.toml` using `private_key_path` and leave `password` empty.

### Method B: Password-Based Authentication
1. Enter your SSH/SFTP password in `private_config.toml` under the `password` field.
2. Leave `private_key_path` empty.

---

## Setup 2: Connection Configuration

Open `private_config.toml` (or `config.toml`) and configure your connection credentials:

```toml
[sftp_credentials]
host = "192.168.1.200"            # Remote server host IP or domain
port = 22                         # Remote SSH/SFTP port (default is 22)
username = "sftp_user"            # Your SSH/SFTP username
password = "my_secure_password"   # Optional: Fill if using password authentication
private_key_path = ""             # Optional: Path to private key if using key authentication
destination_folder = "SyncBackup" # Prefix directory on the remote server
enabled = true
sync = true
```

---

## Local Development: Running a Local SFTP Server

To test SFTP synchronization offline without hitting a remote server, you can spin up a secure local SFTP container using Docker.

### 1. Start SFTP Server via Docker
Run the following command to start an SFTP server listening on port `2222` with username `user` and password `pass`:

```bash
docker run -d --name local-sftp \
  -p 2222:22 \
  -v ~/sftp_data:/home/user/upload \
  atmoz/sftp user:pass:1001
```

### 2. Connection Configuration for Local Testing
Configure your `private_config.toml` to connect to the local container:

```toml
[sftp_credentials]
host = "127.0.0.1"
port = 2222
username = "user"
password = "pass"
private_key_path = ""
destination_folder = "upload"     # Maps to the /home/user/upload folder
enabled = true
sync = true
```
