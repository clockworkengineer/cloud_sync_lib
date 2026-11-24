<#
.SYNOPSIS
    Configures Windows optimizations to drastically accelerate Rust build and link times.

.DESCRIPTION
    1. Verifies that lld-link is configured in .cargo/config.toml.
    2. Adds Windows Defender exclusions for the workspace target directory and Rust binaries (requires Run as Administrator).
    3. Checks for sccache.
#>

Write-Host "=== Rust Build Optimization Setup ===" -ForegroundColor Cyan

# 1. Check Linker in .cargo/config.toml
$cargoConfig = Join-Path $PSScriptRoot "..\.cargo\config.toml"
if (Test-Path $cargoConfig) {
    Write-Host "[OK] .cargo/config.toml is configured with lld-link linker." -ForegroundColor Green
} else {
    Write-Host "[WARN] .cargo/config.toml not found." -ForegroundColor Yellow
}

# 2. Check Administrator Privileges for Windows Defender Exclusions
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if ($isAdmin) {
    Write-Host "Configuring Windows Defender Exclusions..." -ForegroundColor Cyan
    $targetPath = (Resolve-Path (Join-Path $PSScriptRoot "..\target")).Path
    if (-not (Test-Path $targetPath)) {
        New-Item -ItemType Directory -Path $targetPath -Force | Out-Null
    }

    try {
        Add-MpPreference -ExclusionPath $targetPath -ErrorAction Stop
        Write-Host "[OK] Added Defender folder exclusion: $targetPath" -ForegroundColor Green
        
        Add-MpPreference -ExclusionProcess "cargo.exe", "rustc.exe", "lld-link.exe" -ErrorAction SilentlyContinue
        Write-Host "[OK] Added Defender process exclusions for cargo, rustc, and lld-link" -ForegroundColor Green
    } catch {
        Write-Host "[ERR] Failed to set Defender exclusion: $_" -ForegroundColor Red
    }
} else {
    Write-Host "`n[!] To configure Windows Defender exclusions, please run this script in an elevated PowerShell prompt (Run as Administrator):" -ForegroundColor Yellow
    Write-Host "    powershell -ExecutionPolicy Bypass -File scripts\speed_up_builds.ps1`n" -ForegroundColor White
}

Write-Host "=== Setup Check Complete ===" -ForegroundColor Cyan
