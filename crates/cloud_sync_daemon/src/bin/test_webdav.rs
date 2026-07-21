//! Standalone diagnostic utility to verify WebDAV client connection status.

#[macro_use]
#[path = "common.rs"]
pub mod common;

define_verifier_binary!(
    "WebDAV",
    "webdav",
    webdav_credentials,
    WebDAVProvider,
    "webdav_test_connection_tmp.txt"
);
