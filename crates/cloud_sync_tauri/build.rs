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

    if target.contains("android") || target.contains("ios") {
        tauri_build::build();
        return;
    }

    // 2. Determine profile (debug vs release)
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_dir = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("failed to resolve workspace directory");

    // 3. Locate target binary name
    let mut bin_name = "cloud_sync_daemon".to_string();
    if target.contains("windows") {
        bin_name.push_str(".exe");
    }

    let sidecar_target_dir = workspace_dir.join("target").join("sidecar");

    // Only build cloud_sync_daemon sidecar if explicitly requested via BUILD_SIDECAR=1
    // to avoid recursive Cargo invocations during workspace check/test.
    if env::var("BUILD_SIDECAR").map(|v| v == "1").unwrap_or(false) {
        let mut cargo_cmd = Command::new("cargo");
        cargo_cmd.args([
            "build",
            "--bin",
            "cloud_sync_daemon",
            "--all-features",
            "--target-dir",
            sidecar_target_dir.to_str().unwrap(),
        ]);
        if profile == "release" {
            cargo_cmd.arg("--release");
        }
        cargo_cmd.env_remove("CARGO_ENCODED_RUSTFLAGS");
        cargo_cmd.env_remove("RUSTFLAGS");
        cargo_cmd.current_dir(workspace_dir);

        let status = cargo_cmd
            .status()
            .expect("failed to run cargo build for cloud_sync_daemon");
        if !status.success() {
            panic!("Failed to build cloud_sync_daemon");
        }
    }

    // 4. Find candidate pre-built binary
    let candidate_paths = [
        sidecar_target_dir.join(&profile).join(&bin_name),
        sidecar_target_dir
            .join(&target)
            .join(&profile)
            .join(&bin_name),
        workspace_dir.join("target").join(&profile).join(&bin_name),
        workspace_dir
            .join("target")
            .join(&target)
            .join(&profile)
            .join(&bin_name),
        workspace_dir.join("target").join("release").join(&bin_name),
        workspace_dir.join("target").join("debug").join(&bin_name),
    ];

    let found_bin = candidate_paths.into_iter().find(|p| p.is_file());

    // 5. Ensure binaries folder exists
    let dest_dir = manifest_dir.join("binaries");
    fs::create_dir_all(&dest_dir).unwrap();

    // 6. Copy binary with target triple or ensure placeholder exists
    let dest_bin_name = format!(
        "cloud_sync_daemon-{}{}",
        target,
        if target.contains("windows") {
            ".exe"
        } else {
            ""
        }
    );
    let dest_bin_path = dest_dir.join(dest_bin_name);

    if let Some(src_bin_path) = found_bin {
        let _ = fs::copy(&src_bin_path, &dest_bin_path);
    } else if !dest_bin_path.exists() {
        // Create placeholder file so Tauri bundling / validation doesn't fail on missing resource
        let _ = fs::write(&dest_bin_path, b"");
    }

    // 6.5 Generate required icons from app_icon.png if present
    let icons_dir = manifest_dir.join("icons");
    let app_icon_path = icons_dir.join("app_icon.png");
    let icon_ico_path = icons_dir.join("icon.ico");
    let icon_128_path = icons_dir.join("128x128.png");

    if app_icon_path.exists() {
        if let Ok(file_bytes) = std::fs::read(&app_icon_path) {
            if let Ok(img) = image::load_from_memory(&file_bytes) {
                let resized_128 = img.resize_exact(128, 128, image::imageops::FilterType::Lanczos3);
                let rgba_128 = resized_128.to_rgba8();
                let _ = rgba_128.save(&icon_128_path);

                // Save icon.ico (containing standard sizes)
                let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
                for size in [16, 32, 48, 64, 128, 256] {
                    let resized =
                        img.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
                    let rgba = resized.to_rgba8();
                    if let Ok(entry) = ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(
                        size,
                        size,
                        rgba.into_raw(),
                    )) {
                        icon_dir.add_entry(entry);
                    }
                }
                if let Ok(file) = std::fs::File::create(&icon_ico_path) {
                    let _ = icon_dir.write(file);
                }
            }
        }
    }

    // 7. Prevent tauri-build 2.6.3 copy_binaries bug on modern Cargo layouts.
    // In Cargo's new layout (`target/<profile>/build/<pkg>/<hash>/out`), tauri-build's
    // 3-parent calculation resolves target_dir to `target/<profile>/build`.
    // It then attempts to copy the sidecar to `target/<profile>/build/cloud_sync_daemon`,
    // which collides with Cargo's build directory and panics with `IsADirectory` on Linux.
    // Suppressing externalBin during build script execution avoids this faulty copy.
    if env::var("TAURI_CONFIG").is_err() {
        env::set_var("TAURI_CONFIG", r#"{"bundle":{"externalBin":[]}}"#);
    }

    // 8. Invoke tauri_build
    tauri_build::build();
}
