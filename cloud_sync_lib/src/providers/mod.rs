//! Cloud storage provider implementations.
//!
//! This module houses individual provider clients (Google Drive, Dropbox, OneDrive)
//! and definitions for sharing OAuth client credentials.

use serde::{Deserialize, Serialize};

/// Credentials configuration for OAuth2 authorization flows.
///
/// Contains client secrets and long-lived refresh tokens used to retrieve
/// short-lived access tokens dynamically during API execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    /// OAuth2 Client ID.
    pub client_id: String,
    /// OAuth2 Client Secret.
    pub client_secret: String,
    /// Long-lived Refresh Token.
    pub refresh_token: String,
    /// Optional prefix folder in the remote storage where files will be synced.
    pub destination_folder: Option<String>,
    /// Optional toggle to enable/disable the provider backend.
    pub enabled: Option<bool>,
}

pub mod google_drive;
pub mod dropbox;
pub mod onedrive;
pub mod local_sim;
pub mod utils;
pub mod fallback;

pub use google_drive::GoogleDriveProvider;
pub use dropbox::DropboxProvider;
pub use onedrive::OneDriveProvider;
pub use fallback::SimulatedFallback;
