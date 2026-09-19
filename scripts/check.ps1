# Local quality check script for Windows PowerShell
$ErrorActionPreference = "Stop"

Write-Host "===> 1. Checking Code Formatting (cargo fmt)..." -ForegroundColor Cyan
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) {
    Write-Error "Code formatting check failed. Run 'cargo fmt --all' to fix."
}

Write-Host "`n===> 2. Checking Workspace Compilation (all targets & features)..." -ForegroundColor Cyan
cargo check --workspace --all-targets --all-features
if ($LASTEXITCODE -ne 0) {
    Write-Error "Workspace check failed."
}

Write-Host "`n===> 3. Running Clippy Lints (with -D warnings)..." -ForegroundColor Cyan
cargo clippy --workspace --all-targets --all-features -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Error "Clippy check failed."
}

Write-Host "`n===> 4. Running Workspace Tests..." -ForegroundColor Cyan
cargo test --workspace --all-targets
if ($LASTEXITCODE -ne 0) {
    Write-Error "Tests failed."
}

Write-Host "`nAll checks passed successfully! Ready for release." -ForegroundColor Green
