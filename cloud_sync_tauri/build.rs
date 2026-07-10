use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Rerun build.rs if daemon or lib source changes
    println!("cargo:rerun-if-changed=../cloud_sync_daemon/src");
    println!("cargo:rerun-if-changed=../cloud_sync_lib/src");

    // 1. Determine target triple
    let target = env::var("TARGET").unwrap_or_else(|_| "x86_64-pc-windows-msvc".to_string());
    
    // 2. Determine profile (debug vs release)
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    
    // 3. Build cloud_sync_daemon using a separate target directory to avoid deadlocks
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_dir = manifest_dir.parent().unwrap();
    let sidecar_target_dir = workspace_dir.join("target").join("sidecar");

    let mut cargo_cmd = Command::new("cargo");
    cargo_cmd.args(&[
        "build",
        "--bin",
        "cloud_sync_daemon",
        "--target-dir",
        sidecar_target_dir.to_str().unwrap(),
    ]);
    if profile == "release" {
        cargo_cmd.arg("--release");
    }
    
    // Clear cargo flags that might cause problems or deadlocks in nested invocations
    cargo_cmd.env_remove("CARGO_ENCODED_RUSTFLAGS");
    cargo_cmd.env_remove("RUSTFLAGS");
    
    cargo_cmd.current_dir(workspace_dir);
    
    let status = cargo_cmd.status().expect("failed to run cargo build for cloud_sync_daemon");
    if !status.success() {
        panic!("Failed to build cloud_sync_daemon");
    }
    
    // 4. Locate target binary
    let mut bin_name = "cloud_sync_daemon".to_string();
    if target.contains("windows") {
        bin_name.push_str(".exe");
    }
    
    let src_bin_path = sidecar_target_dir
        .join(&profile)
        .join(&bin_name);
        
    // 5. Create binaries folder if it doesn't exist
    let dest_dir = manifest_dir.join("binaries");
    fs::create_dir_all(&dest_dir).unwrap();
    
    // 6. Copy and rename the binary with the target triple
    let dest_bin_name = format!(
        "cloud_sync_daemon-{}{}",
        target,
        if target.contains("windows") { ".exe" } else { "" }
    );
    let dest_bin_path = dest_dir.join(dest_bin_name);
    
    fs::copy(&src_bin_path, &dest_bin_path)
        .expect("failed to copy cloud_sync_daemon sidecar binary");

    // 6.5 Generate required icons from app_icon.png if present
    let icons_dir = manifest_dir.join("icons");
    let app_icon_path = icons_dir.join("app_icon.png");
    let icon_ico_path = icons_dir.join("icon.ico");
    let icon_128_path = icons_dir.join("128x128.png");

    if app_icon_path.exists() {
        let file_bytes = std::fs::read(&app_icon_path).expect("failed to read app_icon.png file");
        let img = image::load_from_memory(&file_bytes).expect("failed to decode app_icon.png image data");
        
        // Save 128x128.png

        let resized_128 = img.resize_exact(128, 128, image::imageops::FilterType::Lanczos3);
        resized_128.save(&icon_128_path).expect("failed to save 128x128.png");

        // Save icon.ico (containing standard sizes)
        let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
        for size in &[16, 32, 48, 64, 128, 256] {
            let resized = img.resize_exact(*size, *size, image::imageops::FilterType::Lanczos3);
            let rgba = resized.to_rgba8();
            let ico_image = ico::IconImage::from_rgba_data(*size, *size, rgba.into_raw());
            let entry = ico::IconDirEntry::encode(&ico_image).expect("failed to encode icon image");
            icon_dir.add_entry(entry);
        }
        let file = std::fs::File::create(&icon_ico_path).expect("failed to create icon.ico");
        icon_dir.write(file).expect("failed to write icon.ico");

    }
        
    // 7. Invoke tauri_build

    tauri_build::build();
}


