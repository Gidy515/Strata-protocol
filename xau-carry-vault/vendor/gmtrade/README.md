# Pinned GMTrade valuation model

Source: https://github.com/gmsol-labs/gmx-solana at revision `8af1e70c0c0b6ff9d6b41ba4997910588fbcec28` (workspace version 0.11.0). MIT license is retained in LICENSE. This is a narrow local model dependency, not the GMTrade deployment source.

Local changes:

- Model clocks receive the transaction timestamp explicitly; host wall-clock `time` is not enabled.
- BorrowingFeeMarketMut projection advances borrowing factors to that timestamp.
- The broad generated SDK decoder is replaced by the exact zero-copy type closure needed for Market, Position, Order and VirtualInventory from the pinned IDL. Decoding/authentication is performed on the heap by Vault 1. This removes an unused generated multi-kilobyte-stack decoder.
- Public-key types use Anchor 1.2.0, matching Vault 1. Captured account sizes/metadata are checked in tests.
- Position virtual inventory can be attached after its address/owner/layout are authenticated.
- `ruint` is pinned to 1.15.0 for platform-tools v1.52's Rust 1.89.
- One return-type lifetime is explicit to avoid a compiler warning.

Protocol arithmetic is preserved. No claim is made that matching an account layout alone proves the deployed program's complete business semantics. Vault tests exercise captured GMTrade preparation, creation and closure CPIs; a completed execution outcome is injected in the reconciliation test. Live oracle-driven execution still needs end-to-end verification.
