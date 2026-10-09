# Score Integrity

## Deterministic Replay Verification

Every score is verified server-side before being accepted:

```
1. Server generates a random seed → sends to client with engine config
2. Client creates WASM GameEngine(seed, config) + InputRecorder
3. Client plays — all physics run in the deterministic WASM engine
4. Each frame: recorder captures {thrust, rotate_left, rotate_right, shoot}
5. Game over: client submits score + bitpacked input log + SHA-256 hash
6. Server decodes input log, replays GameState::new(seed, config) → tick(inputs[0..N])
7. Server checks: replayed score == claimed score
8. Match → accept. Mismatch → reject.
```

The engine uses fixed-point arithmetic (`Fixed` type, no floats) and a deterministic xorshift64 RNG. Same seed + config + inputs = same game on any platform.

Input logs are compact: 4 bools per frame = 4 bits, 2 frames per byte. A 5-minute game at 60fps ≈ 9KB.

### Verification resource limits

The declared frame count must match the encoded byte length, rounded up for an odd final frame.
The server accepts at most 3,000,000 frames, or approximately thirteen hours at 60 frames per second.
This bounds the encoded log to 1.5 MB before base64 encoding; the existing HTTP body limit also applies.
Invalid lengths and counts receive HTTP 400 before replay starts.

The server decodes frames incrementally instead of allocating a vector from the declared count.
Two submissions can hold verification permits at once, with no waiting queue.
Additional submissions receive HTTP 503 with `Retry-After: 1`.
Verification runs on blocking workers, and a disconnected caller retains its permit until its worker finishes.
The browser retries explicit busy responses up to eleven times, keeping the original session and input log.
If the service remains busy, the game-over dialog offers a Save Score Again button.
Other errors and ambiguous network failures are not automatically resubmitted.

A replay checks its ten-second work budget every 1,024 frames.
Exceeding that budget returns HTTP 422, even if the partial score matches.
This is a cooperative limit, so an individual engine tick can extend the deadline.
Normal replay physics, score verification, and stored input-log formats are unchanged.

## What This Eliminates

| Attack | Why It's Caught |
|--------|-----------------|
| **Fabricated scores** | Server replays inputs and derives the real score independently |
| **Modified client (invincibility)** | Server replay hits the collision → different score → mismatch → rejected |
| **Modified client (auto-aim)** | Any client modification that changes game state diverges from the server's replay |
| **Favorable seed selection** | Seed is server-generated per session |
| **Session replay abuse** | One input log per session, ownership verified |

## Bots

A bot can produce a **perfectly valid input log** — the inputs are legitimate, just computed by software. The server replay verifies correctly because the gameplay was real.

**Why per-account rate limiting doesn't work:** Nostr accounts are free (just generate a keypair).

## Bot Detection (implemented)

### 1. Server-side timing verification (unforgeable)
Compares frame count against wall-clock time between session creation and score submission. At 60fps, 3600 frames should take ~60 seconds. Submitting 3600 frames after only 20 seconds is physically impossible → **rejected**.

Uses server timestamps — the client can't fake it.

### 2. Client timing cross-reference (unforgeable)
The client reports timing data, and the server independently measures elapsed time. If the client claims "60 seconds of play" but the server only saw 30 seconds pass, the client is lying about their timing data → **rejected**.

### 3. IP-based analysis
Every session stores the client's IP. On score submission:
- More than 5 distinct accounts from the same IP in a rolling hour → **flagged**
- More than 20 sessions from the same IP in a rolling hour → **flagged**

Note: these are soft signals, not hard rejections — VPNs and shared networks produce legitimate multi-account IPs.

### 4. Frame timing analysis (client-reported)
Client samples `performance.now()` every 60 frames. Server analyzes variance (human: 5-50ms jitter, bot: <1ms) and mean offset (detects slow-motion play).

Client-reported data can be faked by sophisticated bots, but catches lazy automation.

### Dashboard
All bot detection signals are stored in `score_metadata` for monitoring via the admin dashboard (`/admin`, WireGuard-only).

## Other Mitigations

- **Economic deterrent**: Every game costs real sats (configurable, default 1000 sats for 5 plays with a 60-minute expiry window). A bot needs to consistently win the daily competition to be profitable — a single human player winning breaks the bot's ROI.
- **Session ownership**: Verified against `user_id`
- **Nostr authentication**: Every API call requires a signed NIP-98 event
- **Nostr audit ledger**: Every verified score is published with seed, frames, and input hash — anyone can independently replay

## Nostr Audit Ledger

Cryptographically signed record of all game activity (Kind 10100 events):

| Event | What It Proves |
|-------|----------------|
| `game_entry` | Player paid the entry fee |
| `score_verified` | Score was replay-verified. Seed + input hash enable independent replay. |
| `competition_result` | Daily winner, pool size, prize amount |
| `prize_payout` | Prize was paid. Payment ID for cross-referencing. |

**For players**: Verify prize pool math, that the highest score won, and that payouts were made.

**For disputes**: Gaps in the event chain (missing `prize_payout` after `competition_result`) are visible.

**For verification**: Anyone with the game engine can replay a `score_verified` event independently.

## Future Mitigations (not implemented)

- **Reaction time heuristics**: Consistently dodging asteroids within 1-2 frames of threat → beyond human reaction time (~200ms = ~12 frames)
- **Input entropy scoring**: Bots have unnaturally smooth rotation patterns. Human input is noisier.
