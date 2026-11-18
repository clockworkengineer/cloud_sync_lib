//! CLI options and subcommands for cloud_sync_photos binary.

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "cloud_sync_photos",
    about = "Google Photos equivalent sync and organization engine for any cloud backend",
    version
)]
pub struct Cli {
    #[arg(short, long, default_value = "photos_config.toml")]
    pub config: String,

    #[arg(short, long, default_value = "photos_catalog.json")]
    pub catalog: String,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Synchronize local photos directory with the cloud backend (deduplicate and upload).
    Sync(SyncArgs),

    /// Display the chronological timeline (grouped by month/year).
    Timeline,

    /// List all indexed media items.
    List(ListArgs),

    /// Search photos by query, camera model, date range, or favorites.
    Search(SearchArgs),

    /// Manage virtual albums (create, list, add items).
    Album(AlbumArgs),

    /// Toggle favorite status of a photo by ID.
    Favorite { item_id: String },

    /// Display overall photo library statistics.
    Stats,

    /// Pull latest catalog from cloud backend.
    PullCatalog,

    /// Push current local catalog up to the cloud backend.
    PushCatalog,

    /// Download a media item from the cloud to local storage.
    Download { item_id: String, output: String },
}

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Override source directory to scan (defaults to configured source_folder).
    #[arg(short, long)]
    pub source: Option<String>,

    /// Override cloud destination folder.
    #[arg(short, long)]
    pub destination: Option<String>,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Maximum number of items to display.
    #[arg(short, long, default_value = "50")]
    pub limit: usize,

    /// Show only favorites.
    #[arg(short, long)]
    pub favorites_only: bool,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Keyword search in filename, camera make/model, and tags.
    #[arg(short, long)]
    pub query: Option<String>,

    /// Filter by camera model (e.g. "Pixel 8", "iPhone").
    #[arg(short, long)]
    pub camera: Option<String>,

    /// Filter for favorites only.
    #[arg(short, long)]
    pub favorites_only: bool,

    /// Filter start date (YYYY-MM-DD).
    #[arg(long)]
    pub start_date: Option<String>,

    /// Filter end date (YYYY-MM-DD).
    #[arg(long)]
    pub end_date: Option<String>,
}

#[derive(Args, Debug)]
pub struct AlbumArgs {
    #[command(subcommand)]
    pub action: AlbumAction,
}

#[derive(Subcommand, Debug)]
pub enum AlbumAction {
    /// List all albums.
    List,

    /// Create a new album.
    Create {
        name: String,
        #[arg(short, long)]
        description: Option<String>,
    },

    /// Add photos to an album.
    Add {
        album_id: String,
        #[arg(required = true)]
        item_ids: Vec<String>,
    },
}
