import base64
import hashlib
import json
import os
import urllib.request

RPC_URL = os.environ.get(
    "STRATA_RPC_URL",
    "https://api.mainnet-beta.solana.com",
)

GMTRADE_PROGRAM = (
    "Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo"
)

PAXG_MINT = (
    "5GgRAEmv8ZxF2PR5hY72Qs5x1bnQ6UK2RbTPoqJ3wSwW"
)

TOKEN_2022_PROGRAM = (
    "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
)


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
        raise RuntimeError(
            json.dumps(result["error"], indent=2)
        )

    return result["result"]


def verify_paxg():
    result = rpc("getAccountInfo", [
        PAXG_MINT,
        {
            "encoding": "jsonParsed",
            "commitment": "confirmed",
        },
    ])

    account = result["value"]

    if account is None:
        raise RuntimeError("PAXG mint account was not found")

    if account["owner"] != TOKEN_2022_PROGRAM:
        raise RuntimeError("Unexpected PAXG token program")

    parsed = account["data"]["parsed"]

    if parsed["type"] != "mint":
        raise RuntimeError("PAXG address is not a mint")

    info = parsed["info"]

    if info["decimals"] != 6:
        raise RuntimeError("Unexpected PAXG decimals")

    print("PAXG mint:", PAXG_MINT)
    print("Observed slot:", result["context"]["slot"])
    print("Decimals:", info["decimals"])
    print("Freeze authority:", info.get("freezeAuthority"))
    print("Extensions:")
    print(json.dumps(info.get("extensions", []), indent=2))


def discover_gold_markets():
    program = rpc("getAccountInfo", [
        GMTRADE_PROGRAM,
        {
            "encoding": "base64",
            "commitment": "confirmed",
        },
    ])["value"]

    if program is None or not program["executable"]:
        raise RuntimeError("GMTrade program is not executable")

    discriminator = hashlib.sha256(
        b"account:Market"
    ).digest()[:8]

    result = rpc("getProgramAccounts", [
        GMTRADE_PROGRAM,
        {
            "encoding": "base64",
            "commitment": "confirmed",
            "withContext": True,
            "filters": [{
                "memcmp": {
                    "offset": 0,
                    "bytes": base64.b64encode(
                        discriminator
                    ).decode(),
                    "encoding": "base64",
                },
            }],
            "dataSlice": {
                "offset": 0,
                "length": 88,
            },
        },
    ])

    print("\nGMTrade program:", GMTRADE_PROGRAM)
    print("Observed slot:", result["context"]["slot"])

    matches = []

    for item in result["value"]:
        account = item["account"]

        if account["owner"] != GMTRADE_PROGRAM:
            raise RuntimeError("Unexpected market account owner")

        data = base64.b64decode(account["data"][0])

        if len(data) != 88 or data[:8] != discriminator:
            raise RuntimeError("Unexpected market header")

        # Discovery-only decoder for the published Market layout:
        # 8-byte discriminator + 16-byte header + 64-byte name.
        name = data[24:88].split(b"\0", 1)[0].decode("utf-8")

        if any(
            word in name.upper()
            for word in ("XAU", "GOLD", "PAXG")
        ):
            matches.append({
                "address": item["pubkey"],
                "name": name,
                "version": data[8],
            })

    print("Gold market candidates:")
    print(json.dumps(matches, indent=2))

    if not matches:
        raise RuntimeError(
            "No gold market names found with this layout. "
            "Check deployment and account layout before proceeding."
        )

    print(
        "\nDiscovery complete. Market status, collateral, "
        "oracles, liquidity, and PDA execution remain to be verified."
    )


if __name__ == "__main__":
    verify_paxg()
    discover_gold_markets()