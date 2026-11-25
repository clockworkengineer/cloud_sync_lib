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

    #[arg(short = 'C', long, default_value = "photos_catalog.json")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_subcommands() {
        // Sync
        let cli = Cli::try_parse_from([
            "cloud_sync_photos",
            "sync",
            "--source",
            "/photos",
            "--destination",
            "remote_photos",
        ])
        .unwrap();
        match cli.command {
            Commands::Sync(args) => {
                assert_eq!(args.source.as_deref(), Some("/photos"));
                assert_eq!(args.destination.as_deref(), Some("remote_photos"));
            }
            _ => panic!("Expected Sync command"),
        }

        // Timeline
        let cli = Cli::try_parse_from(["cloud_sync_photos", "timeline"]).unwrap();
        assert!(matches!(cli.command, Commands::Timeline));

        // List
        let cli = Cli::try_parse_from(["cloud_sync_photos", "list", "-l", "25", "-f"]).unwrap();
        match cli.command {
            Commands::List(args) => {
                assert_eq!(args.limit, 25);
                assert!(args.favorites_only);
            }
            _ => panic!("Expected List command"),
        }

        // Search
        let cli = Cli::try_parse_from([
            "cloud_sync_photos",
            "search",
            "-q",
            "sunset",
            "-c",
            "Pixel",
            "--start-date",
            "2024-01-01",
        ])
        .unwrap();
        match cli.command {
            Commands::Search(args) => {
                assert_eq!(args.query.as_deref(), Some("sunset"));
                assert_eq!(args.camera.as_deref(), Some("Pixel"));
                assert_eq!(args.start_date.as_deref(), Some("2024-01-01"));
            }
            _ => panic!("Expected Search command"),
        }

        // Album Create & Add
        let cli = Cli::try_parse_from([
            "cloud_sync_photos",
            "album",
            "create",
            "Vacation",
            "-d",
            "Summer trip",
        ])
        .unwrap();
        match cli.command {
            Commands::Album(args) => match args.action {
                AlbumAction::Create { name, description } => {
                    assert_eq!(name, "Vacation");
                    assert_eq!(description.as_deref(), Some("Summer trip"));
                }
                _ => panic!("Expected Create action"),
            },
            _ => panic!("Expected Album command"),
        }

        let cli = Cli::try_parse_from([
            "cloud_sync_photos",
            "album",
            "add",
            "album_123",
            "item_1",
            "item_2",
        ])
        .unwrap();
        match cli.command {
            Commands::Album(args) => match args.action {
                AlbumAction::Add { album_id, item_ids } => {
                    assert_eq!(album_id, "album_123");
                    assert_eq!(item_ids, vec!["item_1", "item_2"]);
                }
                _ => panic!("Expected Add action"),
            },
            _ => panic!("Expected Album command"),
        }

        // Favorite, Stats, PullCatalog, PushCatalog, Download
        assert!(matches!(
            Cli::try_parse_from(["cloud_sync_photos", "favorite", "id123"]).unwrap().command,
            Commands::Favorite { item_id } if item_id == "id123"
        ));
        assert!(matches!(
            Cli::try_parse_from(["cloud_sync_photos", "stats"])
                .unwrap()
                .command,
            Commands::Stats
        ));
        assert!(matches!(
            Cli::try_parse_from(["cloud_sync_photos", "pull-catalog"])
                .unwrap()
                .command,
            Commands::PullCatalog
        ));
        assert!(matches!(
            Cli::try_parse_from(["cloud_sync_photos", "push-catalog"])
                .unwrap()
                .command,
            Commands::PushCatalog
        ));
        assert!(matches!(
            Cli::try_parse_from(["cloud_sync_photos", "download", "item456", "out.jpg"]).unwrap().command,
            Commands::Download { item_id, output } if item_id == "item456" && output == "out.jpg"
        ));
    }
}
