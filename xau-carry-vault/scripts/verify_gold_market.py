import base64
import hashlib
import json
import os
import urllib.request

RPC_URL = os.environ.get(
    "STRATA_RPC_URL",
    "https://api.mainnet-beta.solana.com",
)

GMTRADE_PROGRAM = "Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo"
GOLD_MARKET = "59uFARJWg7B8wcEuXzvkafiT4DuKemdNCN5bshDbwun9"
USDC_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

BASE58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def rpc(method, params):
    payload = json.dumps({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    }).encode()

    request = urllib.request.Request(
        RPC_URL,
        data=payload,
        headers={"Content-Type": "application/json"},
    )

    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.load(response)

    if "error" in result:
        raise RuntimeError(result["error"])

    return result["result"]


def base58_encode(raw):
    number = int.from_bytes(raw, "big")
    encoded = ""

    while number:
        number, remainder = divmod(number, 58)
        encoded = BASE58[remainder] + encoded

    leading_zeros = len(raw) - len(raw.lstrip(b"\x00"))
    return "1" * leading_zeros + encoded


def fetch_account(address):
    result = rpc("getAccountInfo", [
        address,
        {"encoding": "base64", "commitment": "confirmed"},
    ])

    account = result["value"]
    if account is None:
        raise RuntimeError(f"Account does not exist: {address}")

    return result["context"]["slot"], account


def main():
    slot, market = fetch_account(GOLD_MARKET)

    if market["owner"] != GMTRADE_PROGRAM:
        raise RuntimeError("Unexpected market account owner")

    data = base64.b64decode(market["data"][0], validate=True)
    discriminator = hashlib.sha256(b"account:Market").digest()[:8]

    if len(data) < 248 or data[:8] != discriminator:
        raise RuntimeError("Unexpected market account layout")

    version = data[8]
    if version != 0:
        raise RuntimeError(
            f"Unsupported market version {version}; review layout first"
        )

    # Current published Market layout:
    # discriminator: 0..8
    # header and name: 8..88
    # MarketMeta: four consecutive public keys, 88..216
    # store public key: 216..248
    name = data[24:88].split(b"\x00", 1)[0].decode("utf-8")
    market_token = base58_encode(data[88:120])
    index_token = base58_encode(data[120:152])
    long_token = base58_encode(data[152:184])
    short_token = base58_encode(data[184:216])
    store_address = base58_encode(data[216:248])

    print(json.dumps({
        "market": GOLD_MARKET,
        "observed_slot": slot,
        "version": version,
        "name": name,
        "raw_flags": data[10],
        "market_token_mint": market_token,
        "index_token_identifier": index_token,
        "long_collateral_mint": long_token,
        "short_collateral_mint": short_token,
        "store": store_address,
        "both_collateral_mints_are_usdc": (
            long_token == USDC_MINT and short_token == USDC_MINT
        ),
    }, indent=2))

    if long_token != USDC_MINT or short_token != USDC_MINT:
        raise RuntimeError("Market collateral differs from expected USDC")

    store_slot, store = fetch_account(store_address)
    if store["owner"] != GMTRADE_PROGRAM:
        raise RuntimeError("Unexpected store account owner")

    store_data = base64.b64decode(store["data"][0], validate=True)
    store_discriminator = hashlib.sha256(b"account:Store").digest()[:8]

    if store_data[:8] != store_discriminator:
        raise RuntimeError("Unexpected store account discriminator")

    print(f"\nStore owner and discriminator verified at slot {store_slot}.")
    print(
        "Collateral verification passed. Trading status, oracle validation, "
        "available liquidity, fees, and PDA execution remain unverified."
    )


if __name__ == "__main__":
    main()