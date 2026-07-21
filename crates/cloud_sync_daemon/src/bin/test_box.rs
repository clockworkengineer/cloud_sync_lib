//! Standalone diagnostic utility to verify Box client connection status.

#[macro_use]
#[path = "common.rs"]
pub mod common;

define_verifier_binary!(
    "Box",
    "box",
    box_credentials,
    BoxProvider,
    "box_test_connection_tmp.txt"
);
