//! Standalone diagnostic utility to verify S3 client connection status.

#[macro_use]
#[path = "common.rs"]
pub mod common;

define_verifier_binary!(
    "S3",
    "s3",
    s3_credentials,
    S3Provider,
    "s3_test_connection_tmp.txt"
);
