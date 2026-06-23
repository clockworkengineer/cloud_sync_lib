# Recommended Cloud Storage Backends

This document details recommendations and technical findings for expanding `cloud_sync_lib` to support additional web-based disk storage systems.

---

## 1. Amazon S3 & S3-Compatible Storage
Object storage is the industry standard for cloud backups and file archiving. Implementing the S3 API provides support for a massive ecosystem of providers.

### Supported Providers
* **Amazon S3**: The primary target.
* **MinIO**: High-performance self-hosted object storage.
* **Cloudflare R2**: Extremely popular for zero-egress fees.
* **Backblaze B2 & Wasabi**: Popular low-cost alternatives.

### Technical Approach
* **Authentication**: AWS Signature Version 4 (HMAC-SHA256 signing of requests).
* **Integration**: In Rust, you can use the official `aws-sdk-s3` crate or the lightweight `s3` crate.
* **Operations**:
  * Upload: `PutObject`
  * Download: `GetObject`
  * Delete: `DeleteObject`
  * List: `ListObjectsV2`

---

## 2. WebDAV (Nextcloud, ownCloud, Box, NAS)
WebDAV (Web Distributed Authoring and Versioning) is an extension of HTTP that allows clients to perform remote Web content authoring operations.

### Supported Providers
* **Nextcloud / ownCloud**: Leading self-hosted personal clouds.
* **Box**: Has a WebDAV endpoint for legacy clients.
* **Synology / QNAP NAS**: Most home/office NAS drives run WebDAV servers.

### Technical Approach
* **Authentication**: Typically HTTP Basic Auth or Digest Auth.
* **Protocol**: Uses custom HTTP verbs sending and receiving XML payloads:
  * Upload: `PUT`
  * Download: `GET`
  * Delete: `DELETE`
  * List: `PROPFIND` (with Depth header)
  * Create Directory: `MKCOL`

---

## 3. Box (Enterprise Collaboration)
Box is widely used in corporate environments due to its focus on security, granular access controls, and compliance.

### Technical Approach
* **Authentication**: Standard OAuth 2.0 (similar to Google Drive and OneDrive).
* **Protocol**: REST API returning JSON responses:
  * Upload: `POST https://upload.box.com/api/2.0/files/content`
  * Download: `GET https://api.box.com/2.0/files/{file_id}/content`
  * Delete: `DELETE https://api.box.com/2.0/files/{file_id}`
  * List: `GET https://api.box.com/2.0/folders/{folder_id}/items`

---

## 4. MEGA (mega.nz)
MEGA is highly popular among consumers for privacy and its generous free storage tier.

### Technical Approach
* **Authentication / Protocol**: Custom JSON-RPC API.
* **Encryption**: Files must be encrypted client-side using AES before upload, and decrypted upon download. Keys are managed locally. 
* **Complexity**: High, but offers the strongest privacy features.
