# XAU Carry Vault — Vault 1

XAU Carry Vault is a USDC-denominated strategy vault that combines tokenized gold exposure with a short XAU perpetual hedge.

Users deposit USDC and receive **vXAU**, a six-decimal share token representing proportional ownership of the vault’s net assets. The strategy buys tokenized gold through Jupiter and manages its perpetual hedge through GMTrade.

The vault accounts for gold holdings, perpetual position equity, funding, borrowing costs, and execution fees. Deposits and redemptions use net asset value (NAV), allowing strategy gains and losses to flow into the value of each vXAU share.

## Strategy lifecycle

1. Users deposit USDC and receive vXAU shares.
2. The strategy administrator buys PAXG through Jupiter.
3. The administrator submits a short XAU perpetual order through GMTrade.
4. After execution, reconciliation authenticates the resulting order and position against a persisted pre-order baseline.
5. Full NAV determines deposit and redemption pricing.
6. To release liquidity, the administrator reduces or closes the hedge and sells gold.
7. Returned USDC is swept into vault custody and becomes available for withdrawals.

The strategy targets net carry from holding gold alongside an offsetting perpetual position. Returns depend on funding, borrowing costs, trading fees, execution prices, and hedge performance; they are not guaranteed.

## Shares and accounting

USDC and vXAU use six decimals.

For an empty vault, shares bootstrap at a 1:1 ratio. Once shares are outstanding, deposits and redemptions use proportional NAV pricing:

```text
shares minted = floor(deposit amount × outstanding shares / NAV)

assets redeemed = floor(shares redeemed × NAV / outstanding shares)
```

For example, if the vault has 120 USDC of NAV and 100 vXAU outstanding:

- Depositing 12 USDC mints 10 vXAU.
- Redeeming 10 vXAU returns 12 USDC, subject to available withdrawal liquidity.

Minimum-output parameters protect users against receiving fewer shares or assets than expected.

`total_deposited` records cumulative successful deposits. It does not represent current NAV, available cash, outstanding share supply, or strategy profit.

## Net asset value

Full NAV combines:

- USDC held in vault custody.
- USDC held by the strategy authority.
- Eligible collateral held in pending-order escrow.
- Realizable tokenized-gold value.
- Signed perpetual position equity, including applicable funding, borrowing costs, and projected closing costs.

Accounting distinguishes asset transfers from profit, avoids double-counting collateral, and applies conservative rounding and price-confidence bounds.

NAV and withdrawal liquidity are separate. Gold and perpetual equity contribute to share value, but immediate withdrawals require sufficient USDC in vault custody.

## Instructions

### Initialization and administration

| Instruction | Purpose |
| --- | --- |
| `initialize_vault_v1` | Initializes vault state, deposit custody, and the vXAU mint. Restricted to the program’s current upgrade authority. |
| `configure_strategy_v1` | Configures the strategy’s GMTrade accounts. |
| `set_vault_controls_v1` | Updates pause controls and order limits. |
| `set_strategy_execution_v1` | Enables or disables new strategy deployment. |
| `propose_vault_admin_v1` | Proposes a new vault administrator. |
| `accept_vault_admin_v1` | Completes administrator transfer through acceptance by the proposed administrator. |
| `recover_unowned_custody_v1` | Recovers eligible unowned bootstrap custody without taking assets backing outstanding receipts. |

Initialization creates these PDAs:

```text
Vault state:     ["vault_v1", deposit_mint]
Deposit custody: ["token_vault", vault_state]
vXAU mint:       ["vxau_mint", vault_state]
```

The deposit mint must have six decimals. Initialization does not establish that it is an official USDC mint; deployment configuration must select the intended mint. Strategy execution additionally validates its supported mint identities.

### Deposits and withdrawals

| Instruction | Purpose |
| --- | --- |
| `deposit_usdc_v1(amount, min_shares_out)` | Transfers deposit tokens into custody and mints NAV-priced vXAU shares. |
| `withdraw_usdc_v1(amount, min_assets_out)` | Burns the specified number of vXAU shares and returns NAV-priced deposit tokens. |
| `request_withdrawal_v1(nonce, shares, min_assets_out)` | Escrows shares for deferred redemption. |
| `settle_withdrawal_v1` | Permissionlessly settles an eligible request using current NAV and available custody liquidity. |
| `cancel_withdrawal_v1` | Returns escrowed shares to the request owner. |

Queued shares remain outstanding until settlement burns them. Requests are priced at settlement rather than locking an asset value when submitted.

The final redemption requires strategy assets and pending orders to be fully unwound.

### Gold trading and custody

| Instruction | Purpose |
| --- | --- |
| `buy_gold_v1` | Exchanges vault USDC for PAXG through a validated Jupiter shared-accounts route. |
| `sell_gold_v1` | Exchanges PAXG back into USDC through a validated route. |
| `sweep_strategy_usdc_v1` | Permissionlessly returns strategy USDC to canonical vault custody. |

Gold trades validate custody accounts, mint identities, route accounts, exact input amounts, and minimum net output. Supported Token-2022 transfer fees are measured through actual received balances.

Gold purchases require strategy deployment to be enabled and unpaused. Gold sales remain available as a recovery operation when deployment is disabled or paused.

### Perpetual hedging and reconciliation

| Instruction | Purpose |
| --- | --- |
| `prepare_perpetual_user_v1` | Initializes or reuses the strategy’s GMTrade user account. |
| `prepare_short_position_v1` | Initializes or reuses the canonical short position. |
| `create_short_order_v1` | Persists a pre-order baseline and submits a short-increase order. |
| `reconcile_short_order_v1` | Authenticates completed execution and clears local pending-order records. |
| `cancel_short_order_v1` | Cancels or cleans up an eligible increase order and recovers collateral. |
| `create_short_decrease_v1` | Submits a partial reduction or full unwind with a persisted baseline and minimum output. |
| `reconcile_short_decrease_v1` | Authenticates completed reduction, recovers proceeds, and sweeps returned USDC into custody. |
| `cancel_short_decrease_v1` | Cancels or cleans up an eligible decrease order. |
| `accept_full_close_decrease_v1` | Allows the administrator to explicitly accept a protocol-forced full close, subject to reconciliation checks. |

Reconciliation checks authenticated GMTrade accounts, order identity, nonce, execution metadata, and position changes. A missing or cancelled order does not prove successful execution.

### Valuation

| Instruction | Purpose |
| --- | --- |
| `read_idle_nav_v1` | Reports custody-based NAV for an unconfigured strategy. |
| `read_gold_component_v1` | Reports authenticated tokenized-gold valuation. |
| `read_full_nav_v1` | Reports strategy-wide NAV, outstanding shares, and available custody liquidity. |

## Security controls

- PDA, owner, mint, authority, and external-account identity checks.
- Authenticated price updates with freshness and confidence limits.
- Checked arithmetic and conservative valuation rounding.
- User-defined minimum share and redemption outputs.
- Administrator-controlled deployment and order limits.
- Persisted execution baselines and replay-resistant reconciliation.
- Custody-liquidity checks before redemption.
- Recovery operations for disabling deployment, reducing exposure, and returning strategy cash.
- Atomic rollback if a transfer, CPI, validation, or accounting check fails.

Transaction fees remain payable when a transaction fails.

## Validation

From the Anchor workspace:

```bash
cd ~/Downloads/strata-protocol/xau-carry-vault

"$HOME/solana-v3.1.8/solana-release/bin/cargo-build-sbf" \
  --manifest-path programs/xau-carry-vault/Cargo.toml \
  --tools-version v1.52

RUSTUP_TOOLCHAIN=1.95.0 CARGO_BUILD_JOBS=1 \
  cargo test -p xau-carry-vault

RUSTUP_TOOLCHAIN=1.95.0 CARGO_BUILD_JOBS=1 \
  cargo test -p xau-carry-vault --test test_gmtrade_prepare -- --ignored

RUSTUP_TOOLCHAIN=1.95.0 CARGO_BUILD_JOBS=1 \
  cargo check -p xau-carry-vault --features idl-build
```

The verified suite contains **119 passing tests**:

- 54 unit tests.
- 64 LiteSVM integration tests.
- One explicitly enabled GMTrade snapshot test.

Coverage includes initialization, deposits, redemptions, withdrawal queues, NAV pricing, account authentication, position valuation, execution reconciliation, gold trading, transfer fees, recovery, and transaction rollback.

Tests use controlled price updates, a test-only swap router, captured GMTrade fixtures, and controlled execution-state transitions. The router fixture is exclusively for testing.

## Integration with Vault 2

Vault 2 must derive the vXAU mint from the configured Vault 1 state and enforce that mint identity in its account constraints.

vXAU represents proportional ownership of Vault 1’s net assets. It is not a fixed-value USDC receipt or a promise of one unit of physical gold.

Vault 2 can accept vXAU as a basket component while Vault 1 manages its underlying strategy and redemption lifecycle.

## Audit status

This implementation has not been independently audited.