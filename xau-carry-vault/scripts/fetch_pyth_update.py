import base64
import json
import os
import time
import urllib.parse
from pathlib import Path

from verify_pyth_accounts import HERMES, request_json

FEEDS = {
    "paxg": "273717b49430906f4b0c230e99aa1007f83758e3199edbc887c0d06c3e332494",
    "usdc": "eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a",
}

MAX_AGE_SECONDS = 30
MAX_CONFIDENCE_BPS = 100

# The posting script must fetch again immediately before building
# transactions. This saved update is for inspection, not later reuse.
OUTPUT = Path("tests/fixtures/pyth/latest-update.json")


def main():
    api_key = os.environ.get("PYTH_API_KEY", "").strip()

    if not api_key:
        raise RuntimeError("Set PYTH_API_KEY first.")

    # Remove an earlier payload so a failed fetch cannot leave it
    # looking like the latest successful update.
    OUTPUT.unlink(missing_ok=True)

    query = "&".join(
    f"ids[]=0x{feed_id}"
    for feed_id in FEEDS.values()
    )
    query += "&encoding=base64&parsed=true"

    result = request_json(
        f"{HERMES}/v2/updates/price/latest?{query}",
        headers={"Authorization": f"Bearer {api_key}"},
    )

    parsed = result.get("parsed")

    if not isinstance(parsed, list):
        raise RuntimeError("Hermes did not return parsed prices.")

    by_id = {}

    for observation in parsed:
        feed_id = observation["id"].removeprefix("0x").lower()

        if feed_id in by_id:
            raise RuntimeError("Hermes returned a duplicate feed.")

        by_id[feed_id] = observation

    if set(by_id) != set(FEEDS.values()):
        raise RuntimeError("Hermes returned unexpected or missing feeds.")

    now = int(time.time())
    report = {}
    failures = []

    for label, feed_id in FEEDS.items():
        observation = by_id[feed_id]
        price_data = observation["price"]

        price = int(price_data["price"])
        confidence = int(price_data["conf"])
        exponent = int(price_data["expo"])
        published_at = int(price_data["publish_time"])
        age = now - published_at

        problems = []

        if not 0 < price <= (2**63 - 1):
            problems.append("invalid price")

        if not 0 <= confidence <= (2**64 - 1):
            problems.append("invalid confidence")

        if not -24 <= exponent <= 12:
            problems.append("unsupported exponent")

        if published_at < 0 or age < 0:
            problems.append("future or invalid timestamp")
        elif age > MAX_AGE_SECONDS:
            problems.append("stale update")

        if price > 0 and (
            confidence * 10_000 > price * MAX_CONFIDENCE_BPS
        ):
            problems.append("confidence exceeds limit")

        report[label] = {
            "feed_id": feed_id,
            "price": price,
            "confidence": confidence,
            "exponent": exponent,
            "published_at": published_at,
            "age_seconds": age,
            "status": "rejected" if problems else "passed",
            "problems": problems,
        }

        if problems:
            failures.append(label)

    print(json.dumps(report, indent=2))

    if failures:
        raise RuntimeError(
            "Not saving a posting payload: rejected "
            + ", ".join(failures)
            + "."
        )

    binary = result.get("binary", {})

    if binary.get("encoding") != "base64":
        raise RuntimeError("Unexpected binary encoding.")

    updates = binary.get("data")

    if not isinstance(updates, list) or not updates:
        raise RuntimeError("Hermes returned no binary updates.")

    for update in updates:
        if not isinstance(update, str):
            raise RuntimeError("Malformed binary update.")

        if not base64.b64decode(update, validate=True):
            raise RuntimeError("Empty binary update.")

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(
        json.dumps(
            {
                "fetched_at": now,
                "feed_ids": FEEDS,
                "binary": binary,
                "parsed": parsed,
            },
            indent=2,
        )
        + "\n"
    )

    print(
        "Both feeds passed freshness and confidence checks. "
        "Saved inspection payload."
    )
    print("No transaction was submitted.")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, ValueError, KeyError, TypeError) as error:
        raise SystemExit(f"Fetch stopped: {error}")