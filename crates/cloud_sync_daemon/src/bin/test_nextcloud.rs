//! Standalone diagnostic utility to verify Nextcloud client connection status.

#[macro_use]
#[path = "common.rs"]
pub mod common;

define_verifier_binary!(
    "Nextcloud",
    "nextcloud",
    nextcloud_credentials,
    NextcloudProvider,
    "nextcloud_test_connection_tmp.txt"
);
