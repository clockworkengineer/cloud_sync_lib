# Code Hardening Plan: Security, Reliability, & Panic Safety

This document outlines a concrete plan to harden `cloud_sync_lib` against security vulnerabilities, network anomalies, and malformed API responses.

---

## 1. Security Hardening

### 1.1 Key Derivation Function (KDF)
- **Vulnerability:** The client-side encryption wrapper (`EncryptedBackend`) derives its 256-bit AES key using plain SHA-256 over the user password. This is highly vulnerable to dictionary and brute-force attacks if ciphertext files are compromised.
- **Hardening Task:** Replace the simple SHA-256 hasher with a proper key derivation function (KDF) like **PBKDF2-HMAC-SHA256** (or Argon2id) with a salt derived from the backend name or a static system salt.

### 1.2 Insecure Temporary File Creation
- **Vulnerability:** During encryption/decryption, payload files are written to standard temporary directories using random names. However, standard `/tmp` directories on Unix systems are shared among all local users. If standard file creation permissions are used, other local users might read/write the transient files.
- **Hardening Task:** Restrict file creation permissions on temporary sync files. Ensure that temporary file handles are created with `0600` permissions (read/write only by owner) by using crate utilities like `tempfile` or platform-specific Unix options.

### 1.3 Path Traversal Sanitization
- **Vulnerability:** Remote paths are passed directly to provider clients. A malicious or compromised server/file metadata could return filenames containing path traversal sequences (e.g. `../`, `..\\`) that map outside the synchronization root directory when downloading files.
- **Hardening Task:** Sanitize paths returned from list APIs. Ensure that any item name containing `..` or leading slashes is rejected or filtered out to prevent directory traversal attacks during synchronization.

---

## 2. Reliability & Panic Safety

### 2.1 Panic-Free Parser Implementations
- **Vulnerability:** Raw XML parsing (for S3, WebDAV, Nextcloud responses) and JSON parsing (OneDrive, Google Drive, Dropbox) use several `.unwrap()` and `.unwrap_or()` calls on nested options/results. Malformed API responses could cause thread panics in the daemon.
- **Hardening Task:** Audit and refactor all response parsing. Convert all unsafe `.unwrap()` calls to proper pattern matching, returning `StorageError::Provider` if the structure is unexpected.

### 2.2 Input & URL Sanitization
- **Vulnerability:** URLs for S3 bucket endpoints, WebDAV servers, or Nextcloud endpoints are parsed directly without validation. Malformed hostnames could trigger uncaught exceptions in client constructors.
- **Hardening Task:** Validate all input endpoints at build/registry instantiation time. Ensure that URLs are well-formed scheme/host structures and block localhost/internal address mapping if SSRF protection is required.
