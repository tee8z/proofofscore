# Proof of Play

Daily competitive arcade game with Bitcoin Lightning payments, server-verified scores, and a Nostr audit ledger.

Players pay a small entry fee in sats, compete on daily leaderboards, and the top scorer wins the prize pool.

## How It Works

1. **Pay** — Lightning invoice for entry (configurable, default 500 sats)
2. **Play** — Asteroids-style game runs in the browser via deterministic WASM engine
3. **Prove** — Every score is replay-verified server-side before being accepted
4. **Win** — Daily top scorer claims 90% of the prize pool

## Architecture

```
Browser (WASM)                    Server (Axum)
┌──────────────────┐             ┌─────────────────────────┐
│  game_engine     │             │  replay verification    │
│  nostr_signer    │◄──REST────►│  bot detection           │
│  input recorder  │             │  Lightning (LND)         │
└──────────────────┘             │  Nostr audit ledger      │
                                 │  SQLite                  │
                                 └─────────────────────────┘
```

| Crate | Target | Description |
|-------|--------|-------------|
| `game_engine` | WASM + native | Deterministic engine: fixed-point math, seeded RNG, replay verification |
| `nostr_signer` | WASM | Nostr key management, NIP-98 HTTP auth |
| `server` | native | Axum: sessions, payments, score verification, prizes, admin dashboard |

## Gameplay

- **Lives**: 3 starting, max 5. Earn extra lives from boss kills.
- **Asteroids**: Split when shot (large → medium → small). Points scale by size.
- **Enemies**: Drones (level 3+), Fighters (level 5+, homing), Bombers (level 7+, tanky)
- **Bosses**: Every 5 levels. High HP, scaling difficulty each cycle.
- **Power-ups**: Rapid Fire, Shield, Spread Shot, Speed Boost (drop from enemies)
- **Time bonus**: Clear waves fast for extra points
- **Level phases**: Accumulation → The Halving → Bull Market → Bear Market (repeating)

## Score Integrity

Every score is **replay-verified**: the server replays your recorded inputs through the same deterministic engine and independently derives the score. Fabricated scores, modified clients, and speedhacks are all caught.

Bot detection layers: server-side timing verification, IP analysis, frame timing cross-referencing. All signals stored in `score_metadata` for dashboard monitoring.

See [docs/score-integrity.md](docs/score-integrity.md) for full details.

## Setup

```bash
# Build
just build-all           # cargo + WASM

# Configure
cp config/local.example.toml config/local.toml
# Edit with your LND node details

# Run
just run
```

The server auto-creates the SQLite database and runs migrations on startup.

## Deployment

Production deployment uses NixOS on Hetzner with Caddy (auto TLS), WireGuard (admin access), and Backblaze B2 (backups).

See [docs/deployment.md](docs/deployment.md) for full setup guide.

## Docs

- [docs/deployment.md](docs/deployment.md) — Server setup, secrets, CI/CD
- [docs/score-integrity.md](docs/score-integrity.md) — Replay verification, bot detection, attack vectors


## API

```
GET  /api/v1/health_check             Health check

POST /api/v1/users/login              Nostr NIP-98 login
POST /api/v1/users/register           Register with Nostr key
POST /api/v1/users/username/register  Register with username + password
POST /api/v1/users/username/login     Login with username + password

POST /api/v1/game/session             Create game session (402 if unpaid)
GET  /api/v1/game/config              Game config for session
POST /api/v1/game/score               Submit verified score
GET  /api/v1/game/scores/top          Top 10 scores
GET  /api/v1/game/scores/user         User's best scores
GET  /api/v1/game/competition         Competition window + entry fee
GET  /api/v1/game/replays/top         Top replay data

GET  /api/v1/payments/status/{id}     Payment status

GET  /api/v1/prizes/check             Prize eligibility
POST /api/v1/prizes/claim             Claim prize with Lightning invoice

GET  /api/v1/ledger/events            Audit events
GET  /api/v1/ledger/pubkey            Server Nostr pubkey
GET  /api/v1/ledger/summary           Ledger stats
```
