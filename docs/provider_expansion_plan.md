# Provider Expansion Plan: Adding More Cloud Storage Backends

This document outlines a standardized, step-by-step technical plan for adding new cloud storage backends to this workspace. It uses **SFTP (SSH File Transfer Protocol)** as a concrete reference implementation example.

---

## 1. Standardized Integration Workflow

To add any new storage provider, follow these sequential steps across the library and daemon components:

### Phase A: Core Library Integration (`cloud_sync_lib`)

1. **Define Credentials Configuration**:
   Add a new credential configuration struct in `cloud_sync_lib/src/providers/mod.rs`.
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize)]
   pub struct SFTPCredentials {
       pub host: String,
       pub port: Option<u16>,
       pub username: String,
       pub password: Option<String>,
       pub private_key_path: Option<String>,
       pub destination_folder: Option<String>,
       pub enabled: Option<bool>,
       pub sync: Option<bool>,
   }
   ```
2. **Create the Provider Module**:
   Create `cloud_sync_lib/src/providers/sftp.rs` and implement the client/API interface.
3. **Implement the `StorageBackend` Trait**:
   Implement the `StorageBackend` trait on the provider:
   ```rust
   #[async_trait::async_trait]
   impl StorageBackend for SFTPProvider {
       fn name(&self) -> &str { "SFTP" }
       async fn upload(&self, local_path: &Path, remote_path: &str) -> Result<(), StorageError>;
       async fn download(&self, remote_path: &str, local_path: &Path) -> Result<(), StorageError>;
       async fn delete(&self, remote_path: &str) -> Result<(), StorageError>;
       async fn list(&self, remote_path: &str) -> Result<Vec<StorageItem>, StorageError>;
   }
   ```
4. **Re-export and Test Integration**:
   - Expose the credentials and provider in `cloud_sync_lib/src/lib.rs`.
   - Write mock tests in `lib.rs` using simulated fallbacks or wiremock equivalent.

---

### Phase B: Daemon Integration (`cloud_sync_daemon`)

1. **Update App Configuration**:
   Update `AppConfig` in `cloud_sync_daemon/src/config.rs` to parse the new credentials section:
   ```rust
   pub struct AppConfig {
       // ...
       pub sftp_credentials: Option<SFTPCredentials>,
   }
   ```
2. **Update Daemon Startup (`main.rs`)**:
   Instantiate the backend and push it into the list of active sync targets:
   ```rust
   if is_sftp_enabled(&config.sftp_credentials) {
       let sync = config.sftp_credentials.as_ref().and_then(|c| c.sync).unwrap_or(true);
       let inner = config.sftp_credentials.clone().map(SFTPProvider::new);
       let local_sim = LocalSimulation::new(config.sftp_root.clone(), "SFTP".to_string());
       backends.push(Arc::new(SimulatedFallback::new(inner, local_sim, "SFTP", sync)));
   }
   ```
3. **Update Reload Configuration (`control.rs`)**:
   Add identical instantiation logic to the TCP reloading handler.
4. **Update Default Configuration templates**:
   Add a commented template section to `config.toml` (and `private_config.toml`):
   ```toml
   [sftp_credentials]
   host = "127.0.0.1"
   port = 22
   username = "username"
   password = "password"
   destination_folder = ""
   enabled = false
   sync = true
   ```

---

## 2. Concrete Reference Implementation: SFTP Backend

### Recommended Dependencies
* **`openssh-sftp-client`** or **`ssh2`** (a binding to `libssh2`) for handling SSH channel operations asynchronously.

### Expected Trait Method Mapping

- **`upload`**: Open an SFTP write channel, strip prefixes, and write local file chunks over the network.
- **`download`**: Open an SFTP read channel to retrieve file contents and write them locally.
- **`delete`**: Send an SFTP unlink/remove file command to the host.
- **`list`**: Query directory paths and map remote attributes (size, modified time) to `StorageItem` struct instances.
