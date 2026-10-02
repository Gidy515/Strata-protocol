# Strata Protocol

Strata is a Solana dual-vault protocol combining USDC deposit receipts with a shared RWA basket and permissionless basket rebalancing.

This repository is the shared workspace for Team 13: Ankit Tiwari, Viraz, and Gideon. The project is in development. The POC does not yet execute a gold carry strategy or provide withdrawals.

## Architecture

**Vault 1 — XAU Carry Vault:** users deposit USDC and receive vXAU receipts at a 1:1 ratio during the POC. The planned strategy layer will pair a long tokenized-gold position with a short XAU perpetual position. Its target return is net carry after costs; no return is guaranteed.

**Vault 2 — Basket Weighted Vault:** users deposit the required proportions of vXAU, PAXG, USDY, and SPYx into a shared basket and receive BASKET shares, referred to as STRAT-INDEX in the architecture document. All shares of a basket instance represent the same portfolio.

Permissionless rebalancers supply an underweight component and receive an overweight component through an oracle-priced atomic swap. The program must enforce pricing, weight, and execution limits on-chain.

See the [architecture and requirements](docs/architecture-and-requirements.pdf) for the team’s design baseline. See also the [architecture diagram](docs/architecture-diagram.png). Implementation decisions and clarifications should be recorded in `docs/`.

## POC scope

| Requirement | Instruction | Purpose |
| --- | --- | --- |
| REQ01 | `deposit_usdc_v1(amount)` | Transfer USDC into Vault 1 and mint vXAU receipts. |
| REQ02 | `deposit_to_basket_v2(amounts)` | Transfer the four configured basket components and mint proportional BASKET shares. |
| REQ03 | `rebalance_basket(token_in, token_out, amount_in, min_out)` | Exchange underweight and overweight components subject to oracle and portfolio checks. |

Admin-signed `initialize_vault_v1` and `initialize_vault_v2` are setup prerequisites. Each selected use case maps to one Anchor instruction handler; its token transfers and minting occur atomically through CPIs.

Vault state accounts belong to the relevant Strata program. Token accounts and mints belong to the SPL Token or Token-2022 program, with Strata PDAs as their authorities where appropriate.

## Repository structure

Both programs share one Anchor workspace:

```text
Anchor.toml
Cargo.toml
programs/
  xau-carry-vault/          Vault 1 program
  basket-weighted-vault/   Vault 2 program including rebalancing
tests/                    Integration tests
app/                      Frontend as development progresses
docs/
  architecture-and-requirements.pdf
  architecture-diagram.png
  README.md
```

The workspace was created with `anchor init xau-carry-vault`, then `anchor new basket-weighted-vault` from its root. The local workspace folder may remain `xau-carry-vault` while the GitHub repository is named `strata-protocol`.

## Team ownership

- Gideon: Vault 1 initialization, USDC deposits, and vXAU receipt issuance.
- Viraz: Vault 2 initialization, basket deposits, and permissionless rebalancing.
- Ankit: Rebalancing 
- Shared review: the vXAU mint, decimals, token program, POC valuation, and cross-program integration tests.

## Local development

Run commands from the directory containing `Anchor.toml`:

```bash
anchor build
```

Use the test command configured for the workspace once its fixtures and dependencies are in place. Agree on and record Anchor, Solana, Rust, and client package versions before introducing dependencies.

## Implementation decisions to finalize

- Initial basket recipe and first-deposit share issuance.
- Integer math, decimal handling, rounding, and excess-component deposit handling.
- POC vXAU valuation for rebalancing and the later transition to NAV-based share accounting.
- Supported mint addresses, token programs, extensions, and verified oracle feeds.
- Discount basis-point conversion, trade bounds, oracle confidence and freshness checks, and market-session policy.
- Consistent Vault 2 account naming: the component account holding vXAU should be named accordingly, rather than as a USDC account.

## Roadmap

- Gold acquisition and XAU perpetual execution.
- Funding settlement, hedge management, and position-health checks.
- Verified NAV accounting and dynamic vXAU issuance.
- Withdrawals and emergency pause/unwind paths.
- Secondary BASKET/USDC AMM liquidity.

## Development workflow

1. Create a feature branch from `main`.
2. Keep each PR focused on one requirement or implementation decision.
3. Include the related REQ ID, behavior changes, and validation results in the PR.
4. Request review from another teammate before merging.

Record agreed toolchain versions and test commands here as the workspace is developed. Never commit wallet secrets or private keys.

