# Refactor Plan: Compliance with Core Software Library Attributes

This document outlines the master refactor plan for `cloud_sync_lib` to guarantee complete alignment with the 10 software library attributes defined in [attributes.md](file:///home/robt/projects/cloud_sync_lib/notes/attributes.md).

---

## 1. Comprehensive Attribute Analysis

### Attribute 1: Intuitive API Design
- **Current State:** The Primary integration boundary has been refactored by moving sync policy details out of the `StorageBackend` trait into `SyncPolicy`. Builder patterns are implemented for all providers to configuration settings.
- **Future Roadmap:** Introduce a unified factory registry (`BackendRegistry`) so clients can dynamically look up and instantiate a provider using its feature name and credentials configuration without manually matching features.

### Attribute 2: Comprehensive Documentation
- **Current State:** Implemented three runnable example programs in `/examples` demonstrating basic sync, rate-limiting, and encryption.
- **Future Roadmap:** Expand public inline documentation with doc-tests (`cargo test --doc`) to verify that examples inside inline comments compile.

### Attribute 3: High Reliability
- **Current State:** Revamped `StorageError` with structured status codes, mapping 401/403 to `AuthenticationExpired` and 409 to `Conflict`. Added connection drop transient retry handling and Retry-After Timing tests.
- **Future Roadmap:** Implement robust connection recovery logic that detects raw socket errors and maps them to a specialized transient retry category.

### Attribute 4: Performance and Efficiency
- **Current State:** Pagination loops implemented for Google Drive, OneDrive, and Dropbox directory listings.
- **Future Roadmap:** Standardize pagination loops across all other paginated backends (GCS, B2, S3, Azure Blob) so directories containing large amounts of files are always fetched in full.

### Attribute 5: Maintainability
- **Current State:** Builder pattern structs and cleaner error parser boundaries.
- **Future Roadmap:** Extract common request-building, OAuth token-refreshing, and error-handling boilerplate into a shared `HttpProviderClient` helper trait to eliminate duplicated code.

### Attribute 6: Flexibility and Customization
- **Current State:** Connection timeouts and custom headers can be tuned via builders.
- **Future Roadmap:** Allow injecting a custom `reqwest::Client` or a middleware/interceptor layer into the backend builders for logging and network middleware hooks.

### Attribute 7: Strong Security
- **Current State:** Credentials, common configurations, cryptographic keys, and intermediate cryptobuffers are safely zeroed out using `zeroize` when they are dropped.
- **Future Roadmap:** Wrap OAuth transient access token fields in secure containers to prevent them from remaining in heap memory.

### Attribute 8: High Testability
- **Current State:** S3, WebDAV, Google Drive, OneDrive, and Dropbox have mock HTTP flow tests. Added timing test for `Retry-After` header.
- **Future Roadmap:** Implement mock HTTP tests for remaining cloud-based backends (e.g. GCS, B2) to verify response parsing.

### Attribute 9: Compatibility and Portability
- **Current State:** Path conversions automatically map Windows backslashes to standard Unix forward slashes.
- **Future Roadmap:** Standardize date-time parsing for file modification times to consistently return RFC-3339 timestamps across all backends.

### Attribute 10: Low Dependency Footprint
- **Current State:** Features are disabled by default.
- **Future Roadmap:** Migrate heavy provider SDKs (e.g., MEGA) to raw HTTP REST client calls where feasible to keep the dependency graph as small as possible.
