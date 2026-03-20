# Astroid Comp - Development & Deployment Commands
# Run 'just' or 'just --list' to see available commands

# Default recipe - show help
default:
    @just --list

# ============================================
# Development
# ============================================

# Enter development shell with all dependencies
dev:
    nix develop

# Build all crates
build:
    cargo build --workspace

# Build in release mode
build-release:
    cargo build --workspace --release

# Build all WASM modules (nostr_signer + game_engine)
build-wasm:
    wasm-pack build crates/nostr_signer --target web --out-dir ../public_ui/pkg/nostr_signer
    wasm-pack build crates/game_engine --target web --out-dir ../public_ui/pkg/game_engine

# Build WASM modules in release mode
build-wasm-release:
    wasm-pack build crates/nostr_signer --target web --release --out-dir ../public_ui/pkg/nostr_signer
    wasm-pack build crates/game_engine --target web --release --out-dir ../public_ui/pkg/game_engine

# Build everything (cargo + wasm)
build-all: build build-wasm

# ============================================
# Database
# ============================================

# Run database migrations
migrate:
    sqlx migrate run --source crates/server/migrations

# Create a new migration
migrate-add name:
    sqlx migrate add -r {{name}} --source crates/server/migrations

# Prepare SQLx offline data
sqlx-prepare:
    cargo sqlx prepare --workspace

# ============================================
# Testing & Code Quality
# ============================================

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Run a specific test
test-one name:
    cargo test --workspace {{name}} -- --nocapture

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Run clippy linter
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Run all checks (format, clippy, test)
check: fmt-check clippy test

# ============================================
# Run Commands
# ============================================

# Run the server
run *ARGS:
    RUST_LOG=info cargo run --bin server -- {{ARGS}}

# Run with debug logging
run-debug *ARGS:
    RUST_LOG=debug cargo run --bin server -- {{ARGS}}

# Run with trace logging
run-trace *ARGS:
    RUST_LOG=trace cargo run --bin server -- {{ARGS}}

# ============================================
# Setup
# ============================================

# Setup local development environment
setup:
    mkdir -p data
    cp -n config/local.example.toml config/local.toml || true

# ============================================
# Nix Builds
# ============================================

# Build server using Nix
nix-build:
    nix build .#server

# Run server from Nix build
nix-run *ARGS:
    nix run .#server -- {{ARGS}}

# Run Nix flake check
nix-check:
    nix flake check

# ============================================
# Cleanup
# ============================================

# Clean build artifacts
clean:
    cargo clean
