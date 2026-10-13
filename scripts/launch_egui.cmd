@cd /d "%~dp0.."
@if exist "target\release\cloud_sync_egui.exe" (
    @start "" "target\release\cloud_sync_egui.exe"
) else if exist "target\debug\cloud_sync_egui.exe" (
    @start "" "target\debug\cloud_sync_egui.exe"
) else (
    @cargo run --bin cloud_sync_egui
)
