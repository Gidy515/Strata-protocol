use anchor_lang::prelude::*;
use anchor_spl::{
    token_2022::Token2022,
    token_interface::{Mint, TokenAccount},
};

use crate::{
    constants::*,
    error::VaultError,
    state::{StrategyV1State, VaultV1State},
};

// Trusted feed identities; never supplied by the caller.
const PAXG_USD_FEED_HEX: &str =
    "273717b49430906f4b0c230e99aa1007f83758e3199edbc887c0d06c3e332494";

const USDC_USD_FEED_HEX: &str =
    "eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a";

const MAX_PRICE_AGE_SECONDS: u64 = 30;
const MAX_CONFIDENCE_BPS: u16 = 100;

#[derive(Accounts)]
pub struct ReadGoldComponentV1<'info> {
    #[account(
        seeds = [
            VAULT_SEED,
            vault_v1_state.usdc_mint.as_ref()
        ],
        bump = vault_v1_state.bump
    )]
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,

    #[account(
        seeds = [
            STRATEGY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump = strategy_v1_state.bump,
        constraint = strategy_v1_state.vault
            == vault_v1_state.key()
            @ VaultError::InvalidStrategyVault,
        has_one = gold_mint @ VaultError::InvalidGoldMint,
        has_one = gold_custody @ VaultError::InvalidGoldMint,
        has_one = gold_token_program @ VaultError::InvalidGoldMint
    )]
    pub strategy_v1_state: Box<Account<'info, StrategyV1State>>,

    #[account(
        constraint = gold_mint.key().to_string()
            == PAXG_MINT_ADDRESS
            @ VaultError::InvalidGoldMint,
        constraint = gold_mint.decimals == GOLD_DECIMALS
            @ VaultError::InvalidGoldDecimals,
        mint::token_program = gold_token_program
    )]
    pub gold_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        seeds = [
            GOLD_VAULT_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        token::mint = gold_mint,
        token::authority = vault_v1_state,
        token::token_program = gold_token_program
    )]
    pub gold_custody: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: pyth_reader validates receiver ownership,
    /// layout, full verification, feed identity and freshness.
    pub paxg_price_update: UncheckedAccount<'info>,

    /// CHECK: Validated independently for the USDC/USD feed.
    pub usdc_price_update: UncheckedAccount<'info>,

    pub gold_token_program: Program<'info, Token2022>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct GoldComponentQuoteV1 {
    pub vault: Pubkey,
    pub gold_amount: u64,
    pub gold_value_usdc: u64,
    pub paxg_published_at: i64,
    pub usdc_published_at: i64,
    pub observed_slot: u64,
}

fn trusted_feed(hex: &str) -> Result<[u8; 32]> {
    require!(
        hex.len() == 64 && hex.is_ascii(),
        VaultError::InvalidOracleFeed
    );

    let mut feed = [0_u8; 32];

    for (index, byte) in feed.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&hex[start..start + 2], 16)
            .map_err(|_| error!(VaultError::InvalidOracleFeed))?;
    }

    Ok(feed)
}

pub fn handle_read_gold_component(
    ctx: Context<ReadGoldComponentV1>,
) -> Result<GoldComponentQuoteV1> {
    // Configuration already restricts the strategy deposit mint
    // to mainnet USDC. Recheck it at the valuation boundary.
    require!(
        ctx.accounts.vault_v1_state.usdc_mint.to_string()
            == MAINNET_USDC_MINT,
        VaultError::InvalidStrategyDepositMint
    );

    let paxg_feed = trusted_feed(PAXG_USD_FEED_HEX)?;
    let usdc_feed = trusted_feed(USDC_USD_FEED_HEX)?;

    let paxg = crate::pyth_reader::read_price(
        &ctx.accounts.paxg_price_update.to_account_info(),
        &paxg_feed,
        MAX_PRICE_AGE_SECONDS,
        MAX_CONFIDENCE_BPS,
    )?;

    let usdc = crate::pyth_reader::read_price(
        &ctx.accounts.usdc_price_update.to_account_info(),
        &usdc_feed,
        MAX_PRICE_AGE_SECONDS,
        MAX_CONFIDENCE_BPS,
    )?;

    let clock = Clock::get()?;

    // Validate both prices even when the gold balance is zero.
    // Lower PAXG bound / upper USDC bound, rounded down.
    let value = crate::oracle_math::gold_value_in_usdc(
        ctx.accounts.gold_custody.amount,
        &paxg,
        &usdc,
        clock.unix_timestamp,
        MAX_PRICE_AGE_SECONDS,
        MAX_CONFIDENCE_BPS,
        MAX_CONFIDENCE_BPS,
    )?;

    Ok(GoldComponentQuoteV1 {
        vault: ctx.accounts.vault_v1_state.key(),
        gold_amount: ctx.accounts.gold_custody.amount,
        gold_value_usdc: value,
        paxg_published_at: paxg.published_at,
        usdc_published_at: usdc.published_at,
        observed_slot: clock.slot,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_feeds_are_distinct_and_nonzero() {
        let gold = trusted_feed(PAXG_USD_FEED_HEX).unwrap();
        let usdc = trusted_feed(USDC_USD_FEED_HEX).unwrap();

        assert_ne!(gold, [0; 32]);
        assert_ne!(usdc, [0; 32]);
        assert_ne!(gold, usdc);
        assert_eq!(&gold[..4], &[0x27, 0x37, 0x17, 0xb4]);
        assert_eq!(&usdc[..4], &[0xea, 0xa0, 0x20, 0xc6]);
    }

    #[test]
    fn malformed_feed_constants_return_errors() {
        assert!(trusted_feed("").is_err());
        assert!(trusted_feed(&"z".repeat(64)).is_err());
        assert!(trusted_feed(&"é".repeat(32)).is_err());
    }
}