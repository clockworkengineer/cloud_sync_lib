@cd /d "%~dp0.."
@if exist "target\release\cloud_sync_tauri.exe" (
    @start "" "target\release\cloud_sync_tauri.exe"
) else if exist "target\debug\cloud_sync_tauri.exe" (
    @start "" "target\debug\cloud_sync_tauri.exe"
) else (
    @cargo run --bin cloud_sync_tauri
)
