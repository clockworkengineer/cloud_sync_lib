use chrono::{NaiveDate, TimeZone, Utc};
use clap::Parser;
use cloud_sync_photos::cli::{AlbumAction, Cli, Commands};
use cloud_sync_photos::config::{instantiate_backend, PhotosConfig};
use cloud_sync_photos::engine::PhotoLibrary;
use cloud_sync_photos::models::SearchFilter;
use std::path::{Path, PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let args = Cli::parse();

    let config_path = PathBuf::from(&args.config);
    let config = if config_path.exists() {
        PhotosConfig::load_from_file(&config_path).unwrap_or_else(|_| PhotosConfig::default())
    } else {
        let def = PhotosConfig::default();
        let _ = def.save_to_file(&config_path);
        def
    };

    let catalog_path = PathBuf::from(&args.catalog);

    let backend = instantiate_backend(&config.destination_provider, &config.provider_settings)
        .await
        .map_err(|e| {
            format!(
                "Failed to initialize cloud backend {}: {}",
                config.destination_provider, e
            )
        })?;

    let library = PhotoLibrary::new(
        backend,
        &config.destination_folder,
        catalog_path,
        config.strategy,
    )
    .await
    .map_err(|e| format!("Failed to create photo library: {}", e))?;

    match args.command {
        Commands::Sync(sync_args) => {
            let source_dir = sync_args.source.as_deref().unwrap_or(&config.source_folder);
            println!(
                "📸 Syncing photos from {} to {} ({})",
                source_dir, config.destination_provider, config.destination_folder
            );
            let res = library.sync_folder(Path::new(source_dir)).await?;
            println!("\n✅ Sync Complete:");
            println!("   Scanned files: {}", res.files_scanned);
            println!("   Uploaded new:  {}", res.files_imported);
            println!("   Skipped dupes: {}", res.files_skipped_duplicates);
            println!(
                "   Data uploaded: {:.2} MB",
                res.bytes_uploaded as f64 / (1024.0 * 1024.0)
            );
            if !res.errors.is_empty() {
                println!("   Errors encountered: {}", res.errors.len());
                for err in res.errors.iter().take(5) {
                    println!("     - {}", err);
                }
            }
        }
        Commands::Timeline => {
            let timeline = library.get_timeline().await;
            println!("📅 Google Photos Timeline ({} periods)", timeline.len());
            for group in timeline {
                println!(
                    "\n━━━ {} ({} items) ━━━",
                    group.display_title,
                    group.items.len()
                );
                for item in group.items.iter().take(10) {
                    let fav = if item.is_favorite { " ⭐" } else { "" };
                    let cam = item
                        .exif
                        .model
                        .as_deref()
                        .map(|m| format!(" [{}]", m))
                        .unwrap_or_default();
                    println!(
                        "  • {} ({} KB) - {}{}{}",
                        item.file_name,
                        item.size_bytes / 1024,
                        item.date_taken.format("%Y-%m-%d %H:%M"),
                        cam,
                        fav
                    );
                }
                if group.items.len() > 10 {
                    println!("    ... and {} more", group.items.len() - 10);
                }
            }
        }
        Commands::List(list_args) => {
            let cat = library.catalog();
            let cat_lock = cat.read().await;
            let mut items: Vec<_> = cat_lock.items.values().collect();
            items.sort_by(|a, b| b.date_taken.cmp(&a.date_taken));

            println!("📋 Media Items (Showing up to {}):", list_args.limit);
            for item in items
                .iter()
                .filter(|i| !list_args.favorites_only || i.is_favorite)
                .take(list_args.limit)
            {
                let fav = if item.is_favorite { " ⭐" } else { "" };
                println!(
                    "  [{}] {} | {:.2} MB | {}{}",
                    item.id,
                    item.file_name,
                    item.size_bytes as f64 / (1024.0 * 1024.0),
                    item.date_taken.format("%Y-%m-%d %H:%M"),
                    fav
                );
            }
        }
        Commands::Search(search_args) => {
            let mut filter = SearchFilter {
                query: search_args.query,
                camera_model: search_args.camera,
                favorite_only: search_args.favorites_only,
                ..Default::default()
            };
            if let Some(ref s) = search_args.start_date {
                if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                    filter.start_date =
                        Some(Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap()));
                }
            }
            if let Some(ref e) = search_args.end_date {
                if let Ok(d) = NaiveDate::parse_from_str(e, "%Y-%m-%d") {
                    filter.end_date =
                        Some(Utc.from_utc_datetime(&d.and_hms_opt(23, 59, 59).unwrap()));
                }
            }

            let results = library.search(&filter).await;
            println!("🔍 Search Results ({} matches):", results.len());
            for item in results.iter().take(20) {
                let fav = if item.is_favorite { " ⭐" } else { "" };
                println!(
                    "  • {} | {} KB | {}{}",
                    item.file_name,
                    item.size_bytes / 1024,
                    item.date_taken.format("%Y-%m-%d %H:%M"),
                    fav
                );
            }
        }
        Commands::Album(album_args) => match album_args.action {
            AlbumAction::List => {
                let cat = library.catalog();
                let cat_lock = cat.read().await;
                println!("📁 Virtual Albums ({} albums):", cat_lock.albums.len());
                for album in cat_lock.albums.values() {
                    println!(
                        "  • {} (ID: {}) - {} photos",
                        album.name,
                        album.id,
                        album.item_ids.len()
                    );
                    if let Some(ref desc) = album.description {
                        println!("    {}", desc);
                    }
                }
            }
            AlbumAction::Create { name, description } => {
                let id = library.create_album(name.clone(), description).await;
                println!("✅ Created album '{}' (ID: {})", name, id);
            }
            AlbumAction::Add { album_id, item_ids } => {
                let ok = library.add_to_album(&album_id, &item_ids).await;
                if ok {
                    println!("✅ Added {} items to album {}", item_ids.len(), album_id);
                } else {
                    println!("❌ Album not found: {}", album_id);
                }
            }
        },
        Commands::Favorite { item_id } => {
            let status = library.toggle_favorite(&item_id).await;
            match status {
                Some(fav) => println!(
                    "⭐ Favorite status for {}: {}",
                    item_id,
                    if fav {
                        "Marked Favorite"
                    } else {
                        "Unfavorited"
                    }
                ),
                None => println!("❌ Item not found: {}", item_id),
            }
        }
        Commands::Stats => {
            let stats = library.get_stats().await;
            println!("📊 Photo Library Stats:");
            println!("   Total items:    {}", stats.total_items);
            println!("   Photos:         {}", stats.photo_count);
            println!("   Videos:         {}", stats.video_count);
            println!("   Favorites:      {}", stats.favorites_count);
            println!("   Albums:         {}", stats.albums_count);
            println!(
                "   Total Storage:  {:.2} MB",
                stats.total_bytes as f64 / (1024.0 * 1024.0)
            );
            if let Some(old) = stats.oldest_date {
                println!("   Oldest photo:   {}", old.format("%Y-%m-%d %H:%M"));
            }
            if let Some(new) = stats.newest_date {
                println!("   Newest photo:   {}", new.format("%Y-%m-%d %H:%M"));
            }
        }
        Commands::PullCatalog => {
            library.pull_remote_catalog().await?;
            println!("✅ Successfully pulled and merged remote catalog.");
        }
        Commands::PushCatalog => {
            library.push_remote_catalog().await?;
            println!("✅ Successfully pushed catalog to cloud storage.");
        }
        Commands::Download { item_id, output } => {
            library.download_item(&item_id, Path::new(&output)).await?;
            println!("✅ Downloaded photo {} to {}", item_id, output);
        }
    }

    Ok(())
}
