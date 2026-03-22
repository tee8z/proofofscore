# Deployment Guide

## Infrastructure

- **Server**: Hetzner CAX11 (ARM/Ampere), Helsinki Finland
- **OS**: NixOS 24.11 (converted from Ubuntu via nixos-infect)
- **Reverse proxy**: Caddy (auto TLS via Let's Encrypt)
- **Backups**: Backblaze B2 (daily, 7-day local retention)

## Initial Server Setup

### 1. Convert to NixOS

SSH into the fresh Ubuntu server and run nixos-infect:

```bash
ssh root@YOUR_SERVER_IP
curl https://raw.githubusercontent.com/elitak/nixos-infect/master/nixos-infect | NIX_CHANNEL=nixos-24.11 bash -x
```

The server will reboot. Wait a minute, then SSH back in — you're now on NixOS.

### 2. Apply NixOS Configuration

Copy `nix/configuration.nix` to the server:

```bash
scp nix/configuration.nix root@YOUR_SERVER_IP:/etc/nixos/configuration.nix
```

Edit it on the server to set:
- Your domain name (replace `YOUR_DOMAIN` in the Caddy config)
- Your SSH public key at `/opt/proofofscore/secrets/authorized_keys`

Then apply:

```bash
ssh root@YOUR_SERVER_IP "nixos-rebuild switch"
```

### 3. Create Directory Structure

```bash
ssh root@YOUR_SERVER_IP "mkdir -p /opt/proofofscore/{bin,config,data,secrets,backups,creds,ui/pkg/{nostr_signer,game_engine},static,migrations}"
ssh root@YOUR_SERVER_IP "chown -R proofofscore:proofofscore /opt/proofofscore"
```

### 4. Configure Secrets

On the server, create:

**Production config** (`/opt/proofofscore/config/production.toml`):
```toml
[db_settings]
data_folder = "/opt/proofofscore/data"
migrations_folder = "/opt/proofofscore/migrations"

[api_settings]
domain = "127.0.0.1"
port = "8900"
private_key_file = "/opt/proofofscore/creds/private.pem"
# Voltage fields required by config but unused with LND provider
voltage_api_key = ""
voltage_api_url = ""
voltage_org_id = ""
voltage_env_id = ""
voltage_wallet_id = ""

[ui_settings]
remote_url = "https://proofofscore.win"
ui_dir = "/opt/proofofscore/ui"

[ln_settings]
provider = "lnd"
lnd_base_url = "https://YOUR_LND_NODE"
lnd_macaroon_path = "/opt/proofofscore/secrets/admin.macaroon"

[competition_settings]
start_time = "00:00"
duration_secs = 86400       # 24 hours
entry_fee_sats = 1000
plays_per_payment = 5
plays_ttl_minutes = 60
prize_pool_pct = 80

[bot_detection]
enabled = true
max_accounts_per_ip_per_hour = 5
max_sessions_per_ip_per_hour = 20
min_timing_variance_us2 = 1000
max_mean_offset_us = 50000

[admin]
allowed_subnets = ["10.100.0.0/24", "127.0.0.1/32", "::1/128"]
```

**LND macaroon**:
```bash
scp /path/to/admin.macaroon root@YOUR_SERVER_IP:/opt/proofofscore/secrets/admin.macaroon
chown proofofscore:proofofscore /opt/proofofscore/secrets/admin.macaroon
chmod 600 /opt/proofofscore/secrets/admin.macaroon
```

**B2 backup credentials** (`/opt/proofofscore/secrets/b2_credentials`):
```bash
B2_KEY_ID=your_key_id
B2_APP_KEY=your_app_key
```

### 5. Set Up GitHub Actions Secrets

In your GitHub repo settings → Secrets → Actions, add:

- `DEPLOY_HOST`: Server IP address
- `DEPLOY_USER`: SSH user (typically `root` for NixOS)
- `DEPLOY_SSH_KEY`: A private SSH key that can access the server. Generate one:
  ```bash
  ssh-keygen -t ed25519 -f deploy_key -N ""
  # Add deploy_key.pub to the server's authorized_keys
  # Paste the contents of deploy_key into the GitHub secret
  ```

### 6. WireGuard (admin dashboard access)

```bash
chmod +x scripts/setup-wireguard.sh
./scripts/setup-wireguard.sh YOUR_SERVER_IP
# Follow the printed instructions to upload keys and scan QR on your phone
```

Admin dashboard is at `http://10.100.0.1:8900/admin` via VPN.

### 7. DNS

Point your domain's A record to the server IP. Caddy handles TLS automatically.

## Deploying

### Automated (recommended)

Tag a release and push:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The GitHub Actions workflow will:
1. Build the aarch64 release binary
2. Build WASM modules (game_engine + nostr_signer)
3. rsync everything to the server
4. Restart the service
5. Create a GitHub Release

### Manual

```bash
# Build locally (requires aarch64 target or cross-compilation)
cargo build --release --target aarch64-unknown-linux-gnu --bin server
just build-wasm-release

# Deploy
rsync -avz target/aarch64-unknown-linux-gnu/release/server root@SERVER:/opt/proofofscore/bin/
rsync -avz --delete ui/pkg/ root@SERVER:/opt/proofofscore/ui/pkg/
rsync -avz --delete crates/server/static/ root@SERVER:/opt/proofofscore/static/
rsync -avz --delete crates/server/migrations/ root@SERVER:/opt/proofofscore/migrations/
ssh root@SERVER "chown -R proofofscore:proofofscore /opt/proofofscore && systemctl restart proofofscore"
```

## Operations

### Check service status
```bash
ssh root@SERVER "systemctl status proofofscore"
ssh root@SERVER "journalctl -u proofofscore -f"
```

### Manual backup
```bash
ssh root@SERVER "systemctl start proofofscore-backup"
```

### Restore from backup
```bash
# List B2 backups
b2 ls proofofscore-backups-prod backups/

# Download a backup
b2 download-file-by-name proofofscore-backups-prod backups/game-20260320-030000.db ./restore.db

# Stop service, replace DB, start service
ssh root@SERVER "systemctl stop proofofscore"
scp restore.db root@SERVER:/opt/proofofscore/data/game.db
ssh root@SERVER "chown proofofscore:proofofscore /opt/proofofscore/data/game.db && systemctl start proofofscore"
```

### Update NixOS configuration
```bash
scp nix/configuration.nix root@SERVER:/etc/nixos/configuration.nix
ssh root@SERVER "nixos-rebuild switch"
```
