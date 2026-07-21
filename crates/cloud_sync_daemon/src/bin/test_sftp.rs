//! Standalone diagnostic utility to verify SFTP client connection status.

#[macro_use]
#[path = "common.rs"]
pub mod common;

define_verifier_binary!(
    "SFTP",
    "sftp",
    sftp_credentials,
    SFTPProvider,
    "sftp_test_connection_tmp.txt"
);
