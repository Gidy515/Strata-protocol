# XAU Carry Vault — Vault 1 POC

Vault 1 accepts a configured six-decimal SPL token and mints
six-decimal vXAU receipt tokens at a 1:1 ratio.

The POC demonstrates custody, receipt minting, account validation,
and atomic transaction rollback. It does not execute a gold carry strategy.

## Instructions

### initialize_vault_v1

Only the program's current upgrade authority may initialize a vault.

Creates:

- Vault state PDA: ["vault_v1", deposit_mint]
- Deposit custody PDA: ["token_vault", vault_state]
- vXAU mint PDA: ["vxau_mint", vault_state]

The state records the initializer, deposit mint, receipt mint,
cumulative deposits, and state PDA bump.

The configured deposit mint must have six decimals. The program
does not verify that it is an official USDC mint; the initializer
is responsible for choosing the correct mint.

### deposit_usdc_v1(amount: u64)

Requires a positive amount.

Atomically:

1. Transfers deposit tokens from the signer's ATA into vault custody.
2. Mints an equal raw amount of vXAU into the signer's receipt ATA.
3. Increases cumulative deposits using checked arithmetic.

Example: depositing 10,000,000 base units (10 USDC) mints
10,000,000 base units (10 vXAU).

The receipt ATA is created if needed. If any step fails, all
transaction account changes roll back, excluding transaction fees.

## Ownership and accounting

Vault state is owned by this program.

Custody accounts and receipt mints are owned by the SPL Token Program.
The vault state PDA controls custody and receipt mint authority.

total_deposited records cumulative successful deposits. It is not NAV,
current receipt supply, or a measure of strategy profit.

vXAU is a POC deposit receipt. Redemption is not implemented.

## Validation

From the Anchor workspace:

    anchor build
    cargo test -p xau-carry-vault --test test_initialize -- --nocapture

The 14 LiteSVM tests cover:

- Initialization state and authorities
- Unauthorized initialization
- Invalid deposit mint decimals
- Reinitialization rejection
- Successful deposits and receipt minting
- Receipt ATA reuse
- Zero amounts and insufficient funds
- Rollback after receipt mint failure
- Substituted custody accounts
- Unrelated receipt mints
- Another user's source account
- Cumulative accounting overflow
- Deposits from multiple users

Tests use mock SPL tokens and an injected program upgrade authority.

## Integration with Vault 2

Vault 2 must use the vXAU mint derived from the configured Vault 1
state and verify that mint in its account constraints.

vXAU uses six decimals. Depositors can supply their vXAU receipts
to Vault 2 once its deposit instruction is implemented.

## Future work

- Receipt redemption and withdrawals
- Tokenized-gold acquisition and perpetual hedging
- Funding collection and strategy risk controls
- NAV-based share pricing
- Fees and strategy execution

This POC has not been independently audited.
