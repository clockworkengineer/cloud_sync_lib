//! Standalone diagnostic utility to verify MEGA client connection status.

#[macro_use]
#[path = "common.rs"]
pub mod common;

define_verifier_binary!(
    "MEGA",
    "mega",
    mega_credentials,
    MegaProvider,
    "mega_test_connection_tmp.txt"
);
