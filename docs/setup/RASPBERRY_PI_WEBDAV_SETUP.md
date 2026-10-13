# Raspberry Pi WebDAV Server Setup Guide

This guide describes how to configure a lightweight WebDAV server on a Raspberry Pi and connect the `cloud_sync_lib` daemon to it for local network cloud backups.

---

## Method 1: Using Docker (Recommended)
Docker is the cleanest way to set up WebDAV on a Raspberry Pi because it isolates dependencies and is easily updated or removed.

### 1. Install Docker on your Raspberry Pi
Run the official installation script:
```bash
curl -sSL https://get.docker.com | sh
sudo usermod -aG docker $USER
```
*(Log out and log back in to apply group permissions).*

### 2. Create the Storage Directory
Create a directory on your Pi (or on an external USB drive mounted to the Pi) to store the synchronized files:
```bash
mkdir -p ~/webdav_storage
```

### 3. Run the WebDAV Container
Run the lightweight Apache-based WebDAV container, replacing `your_username` and `your_password` with your desired credentials:
```bash
docker run -d --name pi-webdav \
  -p 8080:80 \
  -e USERNAME=your_username \
  -e PASSWORD=your_password \
  -v ~/webdav_storage:/var/lib/dav/data \
  --restart unless-stopped \
  bytemark/webdav
```

---

## Method 2: Native Installation (Using Apache)
If you prefer not to use Docker, you can configure the native Apache WebDAV module (`mod_dav`) directly on Raspberry Pi OS.

### 1. Install Apache
```bash
sudo apt update
sudo apt install apache2 -y
```

### 2. Enable WebDAV Modules
```bash
sudo a2enmod dav
sudo a2enmod dav_fs
```

### 3. Create Storage & Configuration Directories
```bash
sudo mkdir -p /var/www/webdav
sudo chown -R www-data:www-data /var/www/webdav
```

### 4. Create Credentials
Create a password file and add a user (replace `your_username` with your username):
```bash
sudo htpasswd -c /etc/apache2/webdav.password your_username
```
*(You will be prompted to enter and confirm a password).*

### 5. Configure Apache for WebDAV
Open the default configuration file:
```bash
sudo nano /etc/apache2/sites-available/000-default.conf
```
Add the following block inside the `<VirtualHost *:80>` directive:
```apache
Alias /webdav /var/www/webdav

<Directory /var/www/webdav>
    DAV On
    AuthType Basic
    AuthName "Raspberry Pi WebDAV"
    AuthUserFile /etc/apache2/webdav.password
    Require valid-user
</Directory>
```
Save the file (`Ctrl+O`, `Enter`, `Ctrl+X`) and restart Apache:
```bash
sudo systemctl restart apache2
```

---

## Connecting the `cloud_sync_lib` Client

Once your WebDAV server is running on your Raspberry Pi, retrieve the Pi's local IP address by running `hostname -I` on the Pi (e.g. `192.168.1.50`).

Update your `private_config.toml` on your client machine with the connection details:

### If using Docker (Method 1):
```toml
[webdav_credentials]
url = "http://<raspberry-pi-ip>:8080"
username = "your_username"
password = "your_password"
destination_folder = "MySyncFolder"
enabled = true
```

### If using Native Apache (Method 2):
```toml
[webdav_credentials]
url = "http://<raspberry-pi-ip>/webdav/"  # Trailing slash prevents redirect lookups
username = "your_username"
password = "your_password"
destination_folder = "MySyncFolder"
enabled = true
```

---

## Troubleshooting & Tips

### 1. Handling Port Conflicts
If you have other services running on your Raspberry Pi (such as Jenkins on port `8080` or Pi-hole on port `80`), you can change the WebDAV port.

* **Docker (Method 1)**: Change the port mapping parameter from `-p 8080:80` to a custom port (e.g., `-p 8085:80`).
* **Native Apache (Method 2)**:
  1. Edit the Apache ports configuration:
     ```bash
     sudo nano /etc/apache2/ports.conf
     ```
     Change `Listen 80` to `Listen 8085`.
  2. Edit the site VirtualHost configuration:
     ```bash
     sudo nano /etc/apache2/sites-available/000-default.conf
     ```
     Change `<VirtualHost *:80>` at the top of the file to `<VirtualHost *:8085>`.
  3. Restart Apache:
     ```bash
     sudo systemctl restart apache2
     ```
  4. Access WebDAV via `http://<raspberry-pi-ip>:8085/webdav/`.

### 2. Write Permission Errors
If the daemon fails to synchronize or create folders on the server, ensure the Apache user has full ownership of the storage root:
```bash
sudo chown -R www-data:www-data /var/www/webdav
sudo chmod -R 755 /var/www/webdav
```

