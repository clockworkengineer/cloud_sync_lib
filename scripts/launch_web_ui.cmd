@cd /d "%~dp0.."
@if exist "target\release\cloud_sync_ui.exe" (
    @start "" "target\release\cloud_sync_ui.exe"
) else if exist "target\debug\cloud_sync_ui.exe" (
    @start "" "target\debug\cloud_sync_ui.exe"
) else (
    @start "" cargo run --bin cloud_sync_ui
)
@start http://127.0.0.1:8082
