use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub destination_folder: Option<String>,
    pub enabled: Option<bool>,
}

pub mod google_drive;
pub mod dropbox;
pub mod onedrive;

pub use google_drive::GoogleDriveProvider;
pub use dropbox::DropboxProvider;
pub use onedrive::OneDriveProvider;
