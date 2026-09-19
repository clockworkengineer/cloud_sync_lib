# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-09-19

### Added
- Standardized Cargo workspace dependency and metadata inheritance across all 8 crates (`version.workspace = true`, etc.).
- Complete CI/CD GitHub Actions workflows for continuous integration, multi-platform matrix testing, and automated release builds.
- Local developer check scripts (`scripts/check.ps1` and `scripts/check.sh`).
- GitHub issue templates for bug reports, feature requests, and pull request template.
- `.editorconfig` and `.gitattributes` for cross-platform repository consistency.
- Standard documentation: `CONTRIBUTING.md`, `SECURITY.md`, and `CHANGELOG.md`.
- `config.toml.example` configuration template.

### Fixed
- Fixed binary verifiers macro collision (`pub mod common`) in `crates/cloud_sync_daemon/src/bin/common.rs`.
- Fixed field borrowing issue on `config.credentials` in verifier binary generation macro.
- Resolved Tauri build script workspace path traversal and added `--all-features` flag to daemon sidecar packaging.
- Fixed `float_literal_f32_fallback` warnings in `cloud_sync_egui`.
- Cleaned and eliminated all Clippy warnings under strict `-D warnings`.
- Corrected MSRV compatibility on `Option::map_or` across daemon services.
- Removed hardcoded developer absolute paths across all documentation and READMEs.
- Sanitized credentials in `config.toml` and updated `.gitignore` for local credential protection.

### Changed
- Standardized crate versions to `0.2.0`.
- Tauri application bundle target broadened to `"all"`.

## [0.1.1] - Prior Release

- Initial multi-crate workspace structure supporting cloud storage synchronization.
