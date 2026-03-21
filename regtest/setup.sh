#!/usr/bin/env bash
# Regtest setup: fund wallets, open channels, verify LNURL.
#
# Run after `docker compose up -d` and all health checks pass:
#   just regtest-setup
set -euo pipefail

BITCOIN_CLI="docker exec regtest-bitcoind bitcoin-cli -regtest -rpcuser=bitcoin -rpcpassword=bitcoinpass"
LND1="docker exec regtest-lnd1 lncli --network=regtest"
LND2="docker exec regtest-lnd2 lncli --network=regtest"

echo "=== Regtest Setup ==="
echo ""

# ── Wait for services ─────────────────────────────────────────────────────
echo "Waiting for services..."
for container in regtest-bitcoind regtest-lnd1 regtest-lnd2; do
    echo -n "  $container: "
    for i in $(seq 1 60); do
        if docker inspect --format='{{.State.Health.Status}}' "$container" 2>/dev/null | grep -q healthy; then
            echo "healthy"
            break
        fi
        if [ "$i" -eq 60 ]; then
            echo "TIMEOUT"
            echo "Run: docker compose -f regtest/docker-compose.yml logs $container"
            exit 1
        fi
        sleep 2
    done
done

# Wait for lnaddress server
echo -n "  regtest-lnaddress: "
for i in $(seq 1 30); do
    if curl -s http://localhost:9090/health 2>/dev/null | grep -q ok; then
        echo "healthy"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "TIMEOUT (non-critical, LNURL tests will be skipped)"
    fi
    sleep 2
done
echo ""

# ── Create miner wallet if needed ─────────────────────────────────────────
echo "=== Step 1: Bitcoin wallet ==="
WALLETS=$($BITCOIN_CLI listwallets 2>/dev/null || echo "[]")
if ! echo "$WALLETS" | grep -q "miner"; then
    echo "  Creating miner wallet..."
    $BITCOIN_CLI createwallet "miner" >/dev/null 2>&1 || true
    $BITCOIN_CLI loadwallet "miner" >/dev/null 2>&1 || true
fi

# Mine initial blocks (need 101 for coinbase maturity)
BLOCKS=$($BITCOIN_CLI getblockcount)
if [ "$BLOCKS" -lt 101 ]; then
    echo "  Mining initial blocks (current: $BLOCKS)..."
    MINER_ADDR=$($BITCOIN_CLI getnewaddress "miner")
    $BITCOIN_CLI generatetoaddress 101 "$MINER_ADDR" >/dev/null
    echo "  Mined to block $($BITCOIN_CLI getblockcount)"
else
    echo "  Already at block $BLOCKS"
fi
echo ""

# ── Fund LND nodes ───────────────────────────────────────────────────────
echo "=== Step 2: Fund LND nodes ==="
MINER_ADDR=$($BITCOIN_CLI getnewaddress "miner")

for node_name in lnd1 lnd2; do
    if [ "$node_name" = "lnd1" ]; then LND_CMD="$LND1"; else LND_CMD="$LND2"; fi

    BALANCE=$($LND_CMD walletbalance 2>/dev/null | grep -o '"confirmed_balance":[[:space:]]*"[^"]*"' | grep -o '[0-9]*' || echo "0")
    echo "  $node_name balance: $BALANCE sats"

    if [ "$BALANCE" -lt 1000000 ]; then
        ADDR=$($LND_CMD newaddress p2wkh 2>/dev/null | grep -o '"address":[[:space:]]*"[^"]*"' | grep -o '"[^"]*"$' | tr -d '"')
        echo "  Funding $node_name → $ADDR"
        $BITCOIN_CLI sendtoaddress "$ADDR" 1.0 >/dev/null
    fi
done

# Mine to confirm funding
echo "  Mining 6 blocks to confirm..."
$BITCOIN_CLI generatetoaddress 6 "$MINER_ADDR" >/dev/null
sleep 3

# Print updated balances
for node_name in lnd1 lnd2; do
    if [ "$node_name" = "lnd1" ]; then LND_CMD="$LND1"; else LND_CMD="$LND2"; fi
    BALANCE=$($LND_CMD walletbalance 2>/dev/null | grep -o '"confirmed_balance":[[:space:]]*"[^"]*"' | grep -o '[0-9]*' || echo "0")
    echo "  $node_name: $BALANCE sats"
done
echo ""

# ── Connect peers and open channels ───────────────────────────────────────
echo "=== Step 3: Open channels (both directions) ==="
LND1_PUBKEY=$($LND1 getinfo 2>/dev/null | grep -o '"identity_pubkey":[[:space:]]*"[^"]*"' | grep -o '"[^"]*"$' | tr -d '"')
LND2_PUBKEY=$($LND2 getinfo 2>/dev/null | grep -o '"identity_pubkey":[[:space:]]*"[^"]*"' | grep -o '"[^"]*"$' | tr -d '"')
echo "  lnd1 pubkey: $LND1_PUBKEY"
echo "  lnd2 pubkey: $LND2_PUBKEY"

# Connect peers
$LND2 connect "${LND1_PUBKEY}@lnd1:9735" 2>/dev/null || true
sleep 1

# Channel from lnd2→lnd1 (player can pay game invoices)
CHANNELS=$($LND2 listchannels 2>/dev/null || echo "")
if echo "$CHANNELS" | grep -q "$LND1_PUBKEY"; then
    echo "  lnd2→lnd1 channel already exists"
else
    echo "  Opening lnd2→lnd1 channel (500k sats — player pays in)..."
    $LND2 openchannel --node_key="$LND1_PUBKEY" --local_amt=500000 2>/dev/null || true
    sleep 1
fi

# Mine to confirm first channel before opening second
$BITCOIN_CLI generatetoaddress 3 "$MINER_ADDR" >/dev/null
sleep 3

# Channel from lnd1→lnd2 (server can pay out prizes)
CHANNELS=$($LND1 listchannels 2>/dev/null || echo "")
if echo "$CHANNELS" | grep -q "local_balance" && echo "$CHANNELS" | grep -v '"local_balance":  "0"' | grep -q "local_balance"; then
    echo "  lnd1→lnd2 channel already exists with outbound"
else
    echo "  Opening lnd1→lnd2 channel (500k sats — server pays out)..."
    $LND1 openchannel --node_key="$LND2_PUBKEY" --local_amt=500000 2>/dev/null || true
    sleep 1
fi

# Mine to confirm all channels
echo "  Mining 6 blocks..."
$BITCOIN_CLI generatetoaddress 6 "$MINER_ADDR" >/dev/null
sleep 5

# Verify channels
echo "  Channels:"
LND1_ACTIVE=$($LND1 listchannels 2>/dev/null | grep -c '"active": true' || echo "0")
LND2_ACTIVE=$($LND2 listchannels 2>/dev/null | grep -c '"active": true' || echo "0")
echo "    lnd1: $LND1_ACTIVE active channels"
echo "    lnd2: $LND2_ACTIVE active channels"
echo ""

# ── Copy credentials for the game server ──────────────────────────────────
echo "=== Step 4: Export LND1 credentials ==="
CREDS_DIR="regtest/creds"
mkdir -p "$CREDS_DIR"

docker cp regtest-lnd1:/root/.lnd/data/chain/bitcoin/regtest/admin.macaroon "$CREDS_DIR/admin.macaroon"
docker cp regtest-lnd1:/root/.lnd/tls.cert "$CREDS_DIR/tls.cert"
echo "  Saved to $CREDS_DIR/admin.macaroon and $CREDS_DIR/tls.cert"
echo ""

# ── Test LNURL ────────────────────────────────────────────────────────────
echo "=== Step 5: Test LNURL (lightning address) ==="
LNURL_RESP=$(curl -s http://localhost:9090/.well-known/lnurlp/player1 2>/dev/null || echo "")
if echo "$LNURL_RESP" | grep -q "payRequest"; then
    echo "  Lightning address player1@localhost:9090 works!"

    # Test getting an invoice
    CALLBACK=$(echo "$LNURL_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin)['callback'])" 2>/dev/null || echo "")
    if [ -n "$CALLBACK" ]; then
        INV_RESP=$(curl -s "${CALLBACK}?amount=100000" 2>/dev/null || echo "")
        if echo "$INV_RESP" | grep -q '"pr"'; then
            echo "  Invoice generation works!"
            INVOICE=$(echo "$INV_RESP" | python3 -c "import sys,json; print(json.load(sys.stdin)['pr'])" 2>/dev/null || echo "")
            echo "  Invoice: ${INVOICE:0:40}..."
        else
            echo "  Invoice generation failed: $INV_RESP"
        fi
    fi
else
    echo "  LNURL not responding (lnaddress service may still be starting)"
fi
echo ""

# ── Summary ───────────────────────────────────────────────────────────────
echo "=== Setup Complete ==="
echo ""
echo "Services:"
echo "  bitcoind RPC:    http://bitcoin:bitcoinpass@localhost:18443"
echo "  lnd1 REST:       https://localhost:8081 (server/house node)"
echo "  lnd2 REST:       https://localhost:8082 (player node)"
echo "  lnaddress:       http://localhost:9090"
echo ""
echo "Lightning addresses (all route to lnd2):"
echo "  player1@localhost:9090"
echo "  anyname@localhost:9090"
echo ""
echo "Game server config (regtest/config.toml):"
echo "  lnd_base_url = \"https://localhost:8081\""
echo "  lnd_macaroon_path = \"regtest/creds/admin.macaroon\""
echo "  lnd_tls_cert_path = \"regtest/creds/tls.cert\""
echo ""
echo "Run the game server:"
echo "  just run -c regtest/config.toml"
