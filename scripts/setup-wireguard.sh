#!/usr/bin/env bash
# WireGuard key generation and client config setup
# Run this LOCALLY (not on the server)
set -euo pipefail

SERVER_IP="${1:?Usage: ./setup-wireguard.sh <server-ip>}"
WG_DIR="./wg-keys"

echo "=== WireGuard Setup ==="
echo ""

# Check for wg command
if ! command -v wg &>/dev/null; then
    echo "Error: 'wg' command not found."
    echo "Install WireGuard tools:"
    echo "  macOS:  brew install wireguard-tools"
    echo "  Linux:  sudo apt install wireguard-tools"
    echo "  Nix:    nix-shell -p wireguard-tools"
    exit 1
fi

mkdir -p "$WG_DIR"

# Generate server keys
echo "Generating server keys..."
wg genkey | tee "$WG_DIR/server-private.key" | wg pubkey > "$WG_DIR/server-public.key"
chmod 600 "$WG_DIR/server-private.key"

# Generate client keys (your phone/laptop)
echo "Generating client keys..."
wg genkey | tee "$WG_DIR/client-private.key" | wg pubkey > "$WG_DIR/client-public.key"
chmod 600 "$WG_DIR/client-private.key"

SERVER_PUBKEY=$(cat "$WG_DIR/server-public.key")
CLIENT_PUBKEY=$(cat "$WG_DIR/client-public.key")
CLIENT_PRIVKEY=$(cat "$WG_DIR/client-private.key")

# Create client config (for phone/laptop)
cat > "$WG_DIR/client.conf" << EOF
[Interface]
PrivateKey = ${CLIENT_PRIVKEY}
Address = 10.100.0.2/24
DNS = 1.1.1.1

[Peer]
PublicKey = ${SERVER_PUBKEY}
Endpoint = ${SERVER_IP}:51820
AllowedIPs = 10.100.0.1/32
PersistentKeepalive = 25
EOF

echo ""
echo "=== Keys Generated ==="
echo ""
echo "Files in $WG_DIR/:"
echo "  server-private.key  — upload to server"
echo "  server-public.key   — for reference"
echo "  client-private.key  — keep safe, DO NOT share"
echo "  client-public.key   — paste into NixOS config"
echo "  client.conf         — import into WireGuard app on phone"
echo ""
echo "=== Next Steps ==="
echo ""
echo "1. Upload server private key to the server:"
echo "   scp $WG_DIR/server-private.key root@${SERVER_IP}:/opt/proofofscore/secrets/wg-private-key"
echo "   ssh root@${SERVER_IP} 'chmod 600 /opt/proofofscore/secrets/wg-private-key'"
echo ""
echo "2. Update nix/configuration.nix — replace REPLACE_WITH_YOUR_CLIENT_PUBLIC_KEY with:"
echo "   ${CLIENT_PUBKEY}"
echo ""
echo "3. Apply the NixOS config:"
echo "   scp nix/configuration.nix root@${SERVER_IP}:/etc/nixos/configuration.nix"
echo "   ssh root@${SERVER_IP} 'nixos-rebuild switch'"
echo ""
echo "4. Import client.conf on your phone:"
echo "   - Open WireGuard app"
echo "   - Tap '+' → 'Create from file or archive'"
echo "   - Select $WG_DIR/client.conf"
echo "   OR scan this QR code:"
echo ""

# Generate QR code if qrencode is available
if command -v qrencode &>/dev/null; then
    qrencode -t ansiutf8 < "$WG_DIR/client.conf"
else
    echo "   (Install qrencode to display QR: brew install qrencode / nix-shell -p qrencode)"
fi

echo ""
echo "5. Connect VPN on phone, then access admin dashboard at:"
echo "   http://10.100.0.1:8900/admin"
echo ""
echo "=== IMPORTANT: Clean up ==="
echo "After setup, delete the local key files:"
echo "   rm -rf $WG_DIR"
echo "The server key is on the server, the client key is on your phone."
