//! Standardized specification compliance test harness for `StorageBackend` implementations.
//!
//! Enables provider authors to verify contract conformance in a single test call.

#[cfg(feature = "std")]
use crate::traits::{StorageBackend, StorageCapabilities, StorageError};
#[cfg(feature = "std")]
use std::path::Path;

/// Verifies that a `StorageBackend` satisfies the core specification contract:
/// 1. Non-empty provider name.
/// 2. Upload of file metadata and content.
/// 3. Listing presence and correct size.
/// 4. Download and byte-for-byte verification.
/// 5. NotFound error contract when downloading a non-existent file.
/// 6. Deletion and subsequent absence from listing.
/// 7. Capability accessor consistency.
#[cfg(feature = "std")]
pub async fn verify_backend_compliance<B: StorageBackend + ?Sized>(
    backend: &B,
    temp_dir: &Path,
) -> Result<(), StorageError> {
    assert!(!backend.name().trim().is_empty(), "Provider name must not be empty");

    let test_filename = "test_contract_probe.txt";
    let remote_path = "contract_suite/probe.txt";
    let local_file = temp_dir.join(test_filename);
    let payload = b"CloudSync Core Specification Compliance Test Payload - 2026";

    tokio::fs::write(&local_file, payload)
        .await
        .map_err(StorageError::Io)?;

    // 1. Upload
    backend.upload(&local_file, remote_path).await?;

    // 2. List & Verify Size
    let items = backend.list("contract_suite").await?;
    let found = items.iter().find(|i| {
        let p_str = i.path.to_string_lossy();
        p_str.contains("probe.txt")
    });
    assert!(found.is_some(), "Uploaded file must be returned in list()");
    let item = found.unwrap();
    assert_eq!(item.size, payload.len() as u64, "Listed size must match uploaded byte length");

    // 3. Download & Verify Content
    let downloaded_file = temp_dir.join("downloaded_probe.txt");
    backend.download(remote_path, &downloaded_file).await?;
    let read_back = tokio::fs::read(&downloaded_file)
        .await
        .map_err(StorageError::Io)?;
    assert_eq!(read_back, payload, "Downloaded payload must match uploaded content exactly");

    // 4. Verify NotFound Contract
    let non_existent = "contract_suite/non_existent_file_xyz_123.bin";
    let bogus_dest = temp_dir.join("bogus.bin");
    let missing_err = backend.download(non_existent, &bogus_dest).await;
    assert!(missing_err.is_err(), "Downloading non-existent path must return an error");
    let err = missing_err.unwrap_err();
    assert!(err.is_not_found(), "Error on non-existent file must satisfy err.is_not_found()");

    // 5. Delete & Verify Removal
    backend.delete(remote_path).await?;
    let items_after = backend.list("contract_suite").await?;
    let still_found = items_after.iter().any(|i| {
        let p_str = i.path.to_string_lossy();
        p_str.contains("probe.txt")
    });
    assert!(!still_found, "Deleted item must no longer appear in list()");

    // 6. Capability Invariants Check
    let caps = backend.capabilities();
    if caps.contains(StorageCapabilities::SERVER_SIDE_MOVE)
        || caps.contains(StorageCapabilities::SERVER_SIDE_COPY)
    {
        assert!(
            backend.as_server_side_ops().is_some(),
            "Backend declaring SERVER_SIDE_MOVE/COPY must return Some from as_server_side_ops()"
        );
    }
    if caps.contains(StorageCapabilities::CHANGE_CURSOR) {
        assert!(
            backend.as_change_feed().is_some(),
            "Backend declaring CHANGE_CURSOR must return Some from as_change_feed()"
        );
    }
    if caps.contains(StorageCapabilities::RESUMABLE_UPLOAD) {
        assert!(
            backend.as_resumable_uploader().is_some(),
            "Backend declaring RESUMABLE_UPLOAD must return Some from as_resumable_uploader()"
        );
    }

    Ok(())
}
