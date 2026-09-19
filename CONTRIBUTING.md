# Contributing to Cloud Sync

Thank you for your interest in contributing to Cloud Sync! We welcome contributions from the community.

## Code of Conduct

Please be respectful, collaborative, and considerate in all project interactions.

## Getting Started

### Prerequisites
- [Rust 1.75.0+](https://rustup.rs/) (edition 2021)
- `cargo` and `rustfmt` / `clippy` components:
  ```bash
  rustup component add rustfmt clippy
  ```

### Building the Workspace
To build all crates and targets in the workspace:
```bash
cargo build --workspace --all-targets --all-features
```

### Running Tests
To run all unit and integration tests across the workspace:
```bash
cargo test --workspace --all-targets
```

### Checking Quality Before Committing
Run our check scripts or standard cargo checks:
```bash
# On Windows PowerShell:
.\scripts\check.ps1

# On Linux / macOS:
./scripts/check.sh
```

Or manually:
```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

## Pull Request Guidelines

1. Fork the repository and create your branch from `main`.
2. Ensure all changes include tests where applicable.
3. Verify that `cargo fmt`, `cargo clippy`, and `cargo test` pass with zero warnings.
4. Keep commit messages concise, clear, and conventional (e.g., `feat:`, `fix:`, `docs:`).
5. Open a pull request against the `main` branch.
