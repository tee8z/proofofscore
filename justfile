# Proof of Score - Development & Deployment Commands
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
    wasm-pack build crates/nostr_signer --target web --out-dir ../../ui/pkg/nostr_signer
    wasm-pack build crates/game_engine --target web --out-dir ../../ui/pkg/game_engine

# Build WASM modules in release mode
build-wasm-release:
    wasm-pack build crates/nostr_signer --target web --release --out-dir ../../ui/pkg/nostr_signer
    wasm-pack build crates/game_engine --target web --release --out-dir ../../ui/pkg/game_engine

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

# Run e2e tests (builds wasm + starts server with stub Lightning)
test-e2e: build-wasm
    npx playwright test

# Run e2e tests with browser UI
test-e2e-ui: build-wasm
    npx playwright test --ui

# Run e2e tests in headed mode (visible browser)
test-e2e-headed: build-wasm
    npx playwright test --headed

# Take mobile screenshots (Pixel 7 + iPhone 14 viewports)
test-mobile-screenshots: build-wasm
    npx playwright test --project=mobile-chrome --project=mobile-iphone --grep "Mobile screenshots"
    @echo "Screenshots saved to ./screenshots/"

# Install e2e test dependencies
setup-e2e:
    npm install
    npx playwright install chromium

# ============================================
# Run Commands
# ============================================

# Run the server (builds WASM modules first)
run *ARGS: build-wasm
    RUST_LOG=info cargo run --bin server -- {{ARGS}}

# Run with debug logging
run-debug *ARGS:
    RUST_LOG=debug cargo run --bin server -- {{ARGS}}

# Run with trace logging
run-trace *ARGS:
    RUST_LOG=trace cargo run --bin server -- {{ARGS}}

# Run with stub Lightning + cloudflare tunnel for phone testing
# Opens a public HTTPS URL you can hit from your phone
run-tunnel: build-wasm
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Starting server on port 8901 with stub Lightning..."
    cargo run --bin server -- --config config/test.toml &
    SERVER_PID=$!
    trap "kill $SERVER_PID 2>/dev/null" EXIT
    sleep 2
    echo ""
    echo "Starting cloudflare tunnel — scan the URL with your phone:"
    echo ""
    nix shell nixpkgs#cloudflared --command cloudflared tunnel --url http://127.0.0.1:8901

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
# Regtest (local Lightning testing)
# ============================================

# Start the regtest stack (bitcoind + 2 LND nodes + lnaddress server)
regtest-up:
    docker compose -f regtest/docker-compose.yml up -d
    @echo ""
    @echo "Waiting for health checks... (this takes ~30s)"
    @echo "Run 'just regtest-setup' once all services are healthy."

# Fund wallets, open channels, export creds, verify LNURL
regtest-setup:
    ./regtest/setup.sh

# Run the game server against regtest
regtest-run *ARGS: build-wasm
    RUST_LOG=info cargo run --bin server -- -c regtest/config.toml {{ARGS}}

# Run regtest with a short competition window (default 5 minutes)
regtest-run-quick mins="5": build-wasm
    #!/usr/bin/env bash
    set -euo pipefail
    SECS=$(({{mins}} * 60))
    NOW=$(date -u +%H:%M)
    echo "Competition: starts $NOW UTC, duration {{mins}}m (${SECS}s)"
    sed -e "s/^start_time = .*/start_time = \"$NOW\"/" \
        -e "s/^duration_secs = .*/duration_secs = $SECS/" \
        regtest/config.toml > /tmp/regtest-quick.toml
    RUST_LOG=info cargo run --bin server -- -c /tmp/regtest-quick.toml

# Stop and remove the regtest stack (preserves data volumes)
regtest-down:
    docker compose -f regtest/docker-compose.yml down

# Stop and remove everything including data volumes
regtest-clean:
    docker compose -f regtest/docker-compose.yml down -v
    rm -rf regtest/creds

# Show regtest service logs
regtest-logs *ARGS:
    docker compose -f regtest/docker-compose.yml logs {{ARGS}}

# Open a shell on lnd1 (server node)
regtest-lnd1 *ARGS:
    docker exec -it regtest-lnd1 lncli --network=regtest {{ARGS}}

# Open a shell on lnd2 (player node)
regtest-lnd2 *ARGS:
    docker exec -it regtest-lnd2 lncli --network=regtest {{ARGS}}

# Mine a block on regtest (useful for confirming payments)
regtest-mine blocks="1":
    docker exec regtest-bitcoind bitcoin-cli -regtest -rpcuser=bitcoin -rpcpassword=bitcoinpass generatetoaddress {{blocks}} $(docker exec regtest-bitcoind bitcoin-cli -regtest -rpcuser=bitcoin -rpcpassword=bitcoinpass getnewaddress miner)

# ============================================
# Cleanup
# ============================================

# Clean build artifacts
clean:
    cargo clean
