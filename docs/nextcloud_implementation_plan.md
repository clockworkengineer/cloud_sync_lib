# Nextcloud Support Implementation Plan

This plan outlines the steps required to implement native **Nextcloud** integration within the Cloud Sync Workspace.

---

## 1. Technical Design

Nextcloud exposes two primary APIs that we can utilize:
1. **WebDAV API** (for core storage operations): Files can be synced to `/remote.php/dav/files/{username}/{path}` using standard WebDAV HTTP methods (PROPFIND, PUT, GET, DELETE, MKCOL).
2. **OCS Share API** (for advanced Nextcloud features): Allows creating and managing share links, checking notifications, and viewing metadata via `/ocs/v2.php/apps/files_sharing/api/v1/shares`.

### Recommended Credentials Struct
Define Nextcloud configurations in `cloud_sync_lib/src/providers/mod.rs`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextcloudCredentials {
    pub url: String,               // Nextcloud Server URL (e.g. https://nextcloud.example.com)
    pub username: String,          // Nextcloud Username
    pub app_password: String,      // Nextcloud App Password (recommended over raw password)
    pub destination_folder: Option<String>,
    pub enabled: Option<bool>,
    pub sync: Option<bool>,
}
```

---

## 2. Step-by-Step Implementation Steps

### Phase A: `cloud_sync_lib`

1. **Define Structs**: Add `NextcloudCredentials` to `src/providers/mod.rs`.
2. **Create Nextcloud Module**: Create `src/providers/nextcloud.rs`.
3. **Implement WebDAV Operations**: Implement `StorageBackend` for `NextcloudProvider`.
   * **Base URL**: Resolve path to `url + "/remote.php/dav/files/" + username + "/" + destination_folder + "/" + remote_path`.
   * Implement HTTP calls (`reqwest`) mapping to GET, PUT, DELETE, and PROPFIND methods.
4. **Register Provider**: Expose `NextcloudProvider` and `NextcloudCredentials` in `lib.rs` and write mock integration tests.

### Phase B: `cloud_sync_daemon`

1. **Update Config Structs**: Update `AppConfig` in `config.rs` to include `nextcloud_root` and `nextcloud_credentials`.
2. **Add Startup Instantiation**: Update `main.rs` to check `is_nextcloud_enabled` and spawn a new `NextcloudProvider` target inside `backends`.
3. **Update Reload command**: Add equivalent reload mapping in `control.rs`.
4. **Update config files**:
   * Add `nextcloud_root = "./cloud_simulation/nextcloud"` to config.
   * Add `[nextcloud_credentials]` section templates.

### Phase C: `cloud_sync_ui`

1. **Update Dashboard List**: Add `"Nextcloud"` to the `allProviders` array in `index.html` to render status checks.

---

## 3. Verification Plan

- **Local Mocking**: Run a local Nextcloud container using Docker for manual testing:
  ```bash
  docker run -d -p 8080:80 --name local-nextcloud nextcloud
  ```
- **Automated Tests**: Add unit tests using simulated folders and wiremock to verify Nextcloud HTTP interactions.
