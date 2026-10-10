import base64
import hashlib
import json
import os
import struct
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

# HERMES = "https://pyth.dourolabs.app/hermes"
HERMES = "https://hermes.pyth.network"
RECEIVER = "rec2HHDDnjLfj4kE7VyEtFA1HPGQLK33259532cRyHp"
RPC = os.environ.get(
    "PYTH_RPC_URL",
    "https://api.mainnet-beta.solana.com",
)

DISCRIMINATOR = hashlib.sha256(
    b"account:PriceUpdateV2"
).digest()[:8]

SYMBOLS = {
    "paxg": "Crypto.PAXG/USD",
    "usdc": "Crypto.USDC/USD",
}

OUTPUT = Path("tests/fixtures/pyth")
ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def base58(data):
    number = int.from_bytes(data, "big")
    encoded = ""

    while number:
        number, remainder = divmod(number, 58)
        encoded = ALPHABET[remainder] + encoded

    leading = len(data) - len(data.lstrip(b"\0"))
    return "1" * leading + encoded


def request_json(url, payload=None, headers=None):
    request_headers = dict(headers or {})
    request_headers["User-Agent"] = "Strata-Pyth-Verifier/1.0"
    body = None

    service = "Pyth Hermes" if url.startswith(HERMES) else "Solana RPC"

    if payload is not None:
        body = json.dumps(payload).encode()
        request_headers["Content-Type"] = "application/json"

    request = urllib.request.Request(
        url,
        data=body,
        headers=request_headers,
    )

    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return json.load(response)

    except urllib.error.HTTPError as error:
        raw = error.read(4096).decode("utf-8", errors="replace")
        detail = raw.strip() or "Empty response body."

        try:
            parsed = json.loads(raw)

            if isinstance(parsed, dict):
                message = parsed.get("message") or parsed.get("error")

                if isinstance(message, dict):
                    message = message.get("message")

                if isinstance(message, str):
                    detail = message
        except json.JSONDecodeError:
            pass

        # Remove known credentials before displaying any message.
        for secret in (
            os.environ.get("PYTH_API_KEY"),
            os.environ.get("PYTH_RPC_URL"),
        ):
            if secret:
                detail = detail.replace(secret, "[REDACTED]")

        raise RuntimeError(
            f"{service}: HTTP {error.code}. {detail[:500]}"
        ) from None

    except urllib.error.URLError:
        raise RuntimeError(
            f"{service}: network request failed."
        ) from None


def discover_feed(symbol, api_key):
    query = urllib.parse.urlencode({
        "query": symbol.split("/")[0].split(".")[-1],
        "asset_type": "crypto",
    })

    feeds = request_json(
        f"{HERMES}/v2/price_feeds?{query}",
        headers={"Authorization": f"Bearer {api_key}"},
    )

    matches = [
        feed for feed in feeds
        if feed.get("attributes", {}).get("symbol") == symbol
    ]

    if len(matches) != 1:
        raise RuntimeError(
            f"Expected exactly one feed for {symbol}; found {len(matches)}."
        )

    feed_id = matches[0]["id"].removeprefix("0x")
    decoded = bytes.fromhex(feed_id)

    if len(decoded) != 32:
        raise RuntimeError(f"Invalid feed ID length for {symbol}.")

    return feed_id, decoded


def find_accounts(feed_bytes):
    response = request_json(RPC, {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getProgramAccounts",
        "params": [
            RECEIVER,
            {
                "encoding": "base64",
                "commitment": "confirmed",
                "withContext": True,
                "filters": [
                    {"dataSize": 134},
                    {
                        "memcmp": {
                            "offset": 0,
                            "bytes": base58(DISCRIMINATOR),
                        },
                    },
                    {
                        "memcmp": {
                            "offset": 40,
                            "bytes": base58(bytes([1])),
                        },
                    },
                    {
                        "memcmp": {
                            "offset": 41,
                            "bytes": base58(feed_bytes),
                        },
                    },
                ],
            },
        ],
    })

    if "error" in response:
        code = response["error"].get("code")
        raise RuntimeError(
            f"RPC getProgramAccounts failed, code {code}. "
            "Your provider may restrict program scans; "
            "set PYTH_RPC_URL to a provider that supports them."
        )

    return response["result"]


def inspect(entry, expected_feed, observed_slot):
    account = entry["account"]
    data = base64.b64decode(account["data"][0], validate=True)

    if account["owner"] != RECEIVER or account["executable"]:
        raise RuntimeError("Unexpected account owner or executable flag.")

    if len(data) != 134 or data[:8] != DISCRIMINATOR:
        raise RuntimeError("Unexpected PriceUpdateV2 layout.")

    if data[40] != 1 or data[41:73] != expected_feed:
        raise RuntimeError("Verification level or feed mismatch.")

    price = struct.unpack_from("<q", data, 73)[0]
    confidence = struct.unpack_from("<Q", data, 81)[0]
    exponent = struct.unpack_from("<i", data, 89)[0]
    published_at = struct.unpack_from("<q", data, 93)[0]
    previous_time = struct.unpack_from("<q", data, 101)[0]
    posted_slot = struct.unpack_from("<Q", data, 125)[0]

    if previous_time > published_at or posted_slot > observed_slot:
        raise RuntimeError("Inconsistent update timestamp or slot.")

    if price <= 0:
        raise RuntimeError("Nonpositive oracle price.")

    return {
        "address": entry["pubkey"],
        "price": price,
        "confidence": confidence,
        "exponent": exponent,
        "published_at": published_at,
        "posted_slot": posted_slot,
        "age_seconds_at_capture": int(time.time()) - published_at,
    }


def main():
    api_key = os.environ.get("PYTH_API_KEY", "").strip()

    if not api_key:
        raise RuntimeError(
            "Set PYTH_API_KEY in your terminal environment first."
        )

    OUTPUT.mkdir(parents=True, exist_ok=True)
    report = {
        "receiver": RECEIVER,
        "feeds": {},
    }

    for label, symbol in SYMBOLS.items():
        feed_id, feed_bytes = discover_feed(symbol, api_key)
        result = find_accounts(feed_bytes)
        slot = result["context"]["slot"]

        candidates = [
            (inspect(entry, feed_bytes, slot), entry)
            for entry in result["value"]
        ]

        feed_report = {
            "symbol": symbol,
            "feed_id": feed_id,
            "observed_slot": slot,
            "matching_account_count": len(candidates),
        }

        if candidates:
            observation, entry = max(
                candidates,
                key=lambda candidate: (
                    candidate[0]["published_at"],
                    candidate[0]["posted_slot"],
                ),
            )

            feed_report["observation"] = observation

            snapshot = {
                "context": result["context"],
                "pubkey": entry["pubkey"],
                "account": entry["account"],
                "feed_id": feed_id,
            }

            (OUTPUT / f"{label}.json").write_text(
                json.dumps(snapshot, indent=2) + "\n"
            )
        else:
            feed_report["status"] = (
                "No matching account found. A pull update may need "
                "to be posted, or this receiver deployment may not "
                "have an account for the feed."
            )

        report["feeds"][label] = feed_report

    (OUTPUT / "manifest.json").write_text(
        json.dumps(report, indent=2) + "\n"
    )

    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, ValueError, KeyError) as error:
        raise SystemExit(f"Verification stopped: {error}")