# Next-Stage Refactor Plan: Deepening Core Software Library Attributes

This document outlines the next phase of refactoring for `cloud_sync_lib` after the successful completion of Phases 1 through 4. It systematically maps the 10 software library attributes defined in [attributes.md](file:///home/robt/projects/cloud_sync_lib/notes/attributes.md) to identify remaining gaps and propose actionable improvements.

---

## 1. Attribute Analysis & Proposed Enhancements

### Attribute 1: Intuitive API Design
* **Current State:** API simplified by moving sync policy details into `SyncPolicy` and using builder patterns for providers.
* **Remaining Gaps:** Backend instantiation still requires manually matching features and calling concrete constructors or closures inside the daemon.
* **Refactor Plan:** Implement a unified backend factory pattern or provider registry (e.g. `BackendRegistry`) so clients can dynamically look up and instantiate a provider using its feature name and credentials configuration.

### Attribute 2: Comprehensive Documentation
* **Current State:** Added three runnable example programs under `/examples`.
* **Remaining Gaps:** Missing a high-level architectural overview, setup tutorial, and complete Rustdoc generation configurations.
* **Refactor Plan:** Implement documentation tests inside the public library API and create a user manual (`docs/architecture.md`) detailing thread safety, trait boundary designs, and provider simulation strategies.

### Attribute 3: High Reliability
* **Current State:** Unified error parser and `Retry-After` sleep window logic implemented.
* **Remaining Gaps:** Network failures (e.g., connection drops, SSL handshakes) are generic reqwest errors; HTTP status codes like `401 Unauthorized` or `409 Conflict` are not explicitly categorized.
* **Refactor Plan:** Add explicit `StorageError` variants for `AuthenticationExpired`, `Conflict`, and `ConnectionFailed` to allow fine-grained recovery and error handling policies.

### Attribute 4: Performance and Efficiency
* **Current State:** Pagination loops implemented for Google Drive and OneDrive listing.
* **Remaining Gaps:** Other paginated backends (like Dropbox, GCS, B2, S3, Azure Blob) still retrieve files in a single page limit. Checksum generation uses static sizes.
* **Refactor Plan:** 
  - Standardize pagination loops across all paginated backends (Dropbox, GCS, B2, S3, Azure Blob).
  - Dynamically scale stream buffer sizes depending on network latency or upload configuration settings.

### Attribute 5: Maintainability
* **Current State:** Providers contain builder structs and cleaner error boundaries.
* **Remaining Gaps:** There is significant copy-pasted boilerplate across HTTP providers for building requests, executing retries, and handling OAuth token refreshing.
* **Refactor Plan:** Create an `HttpClientExt` helper trait or a generic `HttpProviderClient` wrapper that encapsulates OAuth token management and standard REST methods (GET/PUT/DELETE) to eliminate duplicated logic.

### Attribute 6: Flexibility and Customization
* **Current State:** Fine-tuning parameters (timeouts, custom headers) are supported via builders.
* **Remaining Gaps:** Internal HTTP client (`reqwest::Client`) is hardcoded; users cannot inject custom clients or middlewares (e.g., logging, custom DNS resolution).
* **Refactor Plan:** Allow injecting a custom `reqwest::Client` or a middleware/interceptor layer into the backend builders.

### Attribute 7: Strong Security
* **Current State:** Zeroization implemented for credentials and common settings.
* **Remaining Gaps:** Cryptographic keys in `EncryptedBackend` and raw buffer values are not securely cleared after processing.
* **Refactor Plan:** Wrap the raw AES keys and encryption buffers inside `zeroize` containers to ensure cryptosecrets are securely wiped from RAM immediately after encryption or decryption.

### Attribute 8: High Testability
* **Current State:** S3, WebDAV, Google Drive, OneDrive, and Dropbox have mock HTTP flow tests.
* **Remaining Gaps:** The rate-limiting retry mechanism (using `Retry-After` headers) is not covered by mock server tests.
* **Refactor Plan:** Add integration tests utilizing `wiremock` to simulate `TOO_MANY_REQUESTS` responses with custom `Retry-After` headers, verifying that the retry backoff duration exactly respects the server header.

### Attribute 9: Compatibility and Portability
* **Current State:** Windows backslash `\` path separators are normalized to Unix forward slashes `/`.
* **Remaining Gaps:** File modification timestamps are handled differently across providers (some return system time, some return custom strings).
* **Refactor Plan:** Standardize modification metadata parsing to ensure RFC-3339 timestamps are uniformly returned across all 14 backends.

### Attribute 10: Low Dependency Footprint
* **Current State:** Features are disabled by default.
* **Remaining Gaps:** Several providers pull in heavy dependencies (e.g., custom SDKs) that could be simplified with lightweight REST implementations.
* **Refactor Plan:** Audit dependencies and migrate heavy provider crates (e.g., `mega`) to raw HTTP client calls where feasible to keep the dependency graph minimal.
