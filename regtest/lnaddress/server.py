"""
Minimal LNURL-pay / Lightning Address server for regtest testing.

Serves:
  GET /.well-known/lnurlp/<username>  → LNURL-pay params
  GET /lnurlp/callback?amount=<msats> → bolt11 invoice from LND

Any username is accepted — they all resolve to the same LND node (lnd2).
"""

import json
import os
import ssl
import urllib3

import requests
from flask import Flask, jsonify, request

# Suppress TLS warnings for self-signed LND certs
urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)

app = Flask(__name__)

LND_REST_URL = os.environ.get("LND_REST_URL", "https://lnd2:8080")
LND_MACAROON_PATH = os.environ.get(
    "LND_MACAROON_PATH", "/lnd/data/chain/bitcoin/regtest/admin.macaroon"
)
LND_TLS_CERT_PATH = os.environ.get("LND_TLS_CERT_PATH", "/lnd/tls.cert")
DOMAIN = os.environ.get("DOMAIN", "localhost:9090")
PORT = int(os.environ.get("PORT", "9090"))

# Min/max sendable in millisats
MIN_SENDABLE = 1_000       # 1 sat
MAX_SENDABLE = 100_000_000_000  # 100k sats


def get_macaroon_hex():
    with open(LND_MACAROON_PATH, "rb") as f:
        return f.read().hex()


def lnd_headers():
    return {"Grpc-Metadata-macaroon": get_macaroon_hex()}


@app.route("/.well-known/lnurlp/<username>")
def lnurlp(username):
    """LNURL-pay first request (LUD-16 lightning address resolution)."""
    callback = f"http://{DOMAIN}/lnurlp/callback"
    return jsonify(
        {
            "tag": "payRequest",
            "callback": callback,
            "minSendable": MIN_SENDABLE,
            "maxSendable": MAX_SENDABLE,
            "metadata": json.dumps(
                [["text/plain", f"Payment to {username}@{DOMAIN}"]]
            ),
            "commentAllowed": 0,
        }
    )


@app.route("/lnurlp/callback")
def lnurlp_callback():
    """LNURL-pay callback — creates a bolt11 invoice on LND and returns it."""
    amount_msats = request.args.get("amount")
    if not amount_msats:
        return jsonify({"status": "ERROR", "reason": "Missing amount parameter"}), 400

    try:
        amount_msats = int(amount_msats)
    except ValueError:
        return jsonify({"status": "ERROR", "reason": "Invalid amount"}), 400

    if amount_msats < MIN_SENDABLE or amount_msats > MAX_SENDABLE:
        return (
            jsonify(
                {
                    "status": "ERROR",
                    "reason": f"Amount out of range ({MIN_SENDABLE}-{MAX_SENDABLE})",
                }
            ),
            400,
        )

    # Convert to sats for LND (LND v1 API uses "value" in sats)
    amount_sats = amount_msats // 1000

    # Create invoice on LND
    try:
        resp = requests.post(
            f"{LND_REST_URL}/v1/invoices",
            headers=lnd_headers(),
            json={"value": str(amount_sats), "memo": f"LNURL payment ({amount_sats} sats)"},
            verify=False,
            timeout=10,
        )
        resp.raise_for_status()
        data = resp.json()
        bolt11 = data.get("payment_request", "")

        if not bolt11:
            return (
                jsonify({"status": "ERROR", "reason": "LND returned empty invoice"}),
                500,
            )

        return jsonify({"pr": bolt11, "routes": []})

    except Exception as e:
        return jsonify({"status": "ERROR", "reason": str(e)}), 500


@app.route("/health")
def health():
    return "ok"


if __name__ == "__main__":
    print(f"Lightning Address server starting on :{PORT}")
    print(f"  LND: {LND_REST_URL}")
    print(f"  Domain: {DOMAIN}")
    print(f"  Any username@{DOMAIN} will work")
    app.run(host="0.0.0.0", port=PORT)
