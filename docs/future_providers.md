# Future Providers Roadmap

Recommendations for the next storage backends to support in the Cloud Sync Workspace.

---

## 1. Azure Blob Storage (Enterprise Focus)

- **Description**: Microsoft's object storage service, equivalent to Amazon S3.
- **Why Support It**: Completes support for the standard enterprise cloud storage tier (alongside AWS S3).
- **Rust Library**: `azure_storage_blobs` crate.

---

## 2. Nextcloud (Self-Hosted/Privacy Focus)

- **Description**: The industry-standard open-source private cloud productivity suite.
- **Why Support It**: While Nextcloud works over WebDAV (which is already supported), a native Nextcloud provider allows utilizing Nextcloud APIs to generate public share links, manage file versions, and tag files directly from the UI.
- **Rust Library**: Custom HTTP wrapper targeting the Nextcloud OCS API.

---

## 3. Google Cloud Storage - GCS (Developer Focus)

- **Description**: Google Cloud's developer-focused object storage.
- **Why Support It**: Popular for hosting backups and developer assets on GCP.
- **Rust Library**: `google-cloud-storage` crate, or configuration via the S3 Compatibility API wrapper.

---

## 4. Mega.nz (Consumer Privacy Focus)

- **Description**: Consumer cloud storage focused on end-to-end zero-knowledge encryption.
- **Why Support It**: Highly popular among personal users due to its generous free tier (20 GB) and security model.
- **Rust Library**: `mega` crate.
