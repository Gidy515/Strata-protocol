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

#[derive(Accounts)]
pub struct ConfigureStrategyV1<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [
            VAULT_SEED,
            vault_v1_state.usdc_mint.as_ref()
        ],
        bump = vault_v1_state.bump,
        has_one = admin @ VaultError::UnauthorizedStrategyAdmin,
        constraint = vault_v1_state.usdc_mint.to_string()
            == MAINNET_USDC_MINT
            @ VaultError::InvalidStrategyDepositMint
    )]
    pub vault_v1_state: Account<'info, VaultV1State>,

    #[account(
        constraint = gold_mint.key().to_string()
            == PAXG_MINT_ADDRESS
            @ VaultError::InvalidGoldMint,
        constraint = gold_mint.decimals == GOLD_DECIMALS
            @ VaultError::InvalidGoldDecimals,
        mint::token_program = gold_token_program
    )]
    pub gold_mint: InterfaceAccount<'info, Mint>,

    /// CHECK: The exact program address and executable flag are checked.
    #[account(
        constraint = perpetual_program.key().to_string()
            == GMTRADE_PROGRAM_ADDRESS
            @ VaultError::InvalidPerpetualProgram,
        executable
    )]
    pub perpetual_program: UncheckedAccount<'info>,

    /// CHECK: Address and owner are checked here; discriminator in handler.
    #[account(
        constraint = perpetual_store.key().to_string()
            == GMTRADE_STORE_ADDRESS
            @ VaultError::InvalidPerpetualStore,
        owner = perpetual_program.key()
    )]
    pub perpetual_store: UncheckedAccount<'info>,

    /// CHECK: Address and owner are checked here; market fields in handler.
    #[account(
        constraint = perpetual_market.key().to_string()
            == GMTRADE_XAU_MARKET_ADDRESS
            @ VaultError::InvalidPerpetualMarket,
        owner = perpetual_program.key()
    )]
    pub perpetual_market: UncheckedAccount<'info>,

    #[account(
        init,
        payer = admin,
        space = 8 + StrategyV1State::INIT_SPACE,
        seeds = [
            STRATEGY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump
    )]
    pub strategy_v1_state: Account<'info, StrategyV1State>,

    #[account(
        init,
        payer = admin,
        seeds = [
            GOLD_VAULT_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        token::mint = gold_mint,
        token::authority = vault_v1_state,
        token::token_program = gold_token_program
    )]
    pub vault_v1_gold_account: InterfaceAccount<'info, TokenAccount>,

    pub gold_token_program: Program<'info, Token2022>,
    pub system_program: Program<'info, System>,
}

fn read_pubkey(data: &[u8], offset: usize) -> Result<Pubkey> {
    let end = offset
        .checked_add(32)
        .ok_or(VaultError::InvalidExternalAccountData)?;

    let bytes: [u8; 32] = data
        .get(offset..end)
        .ok_or(VaultError::InvalidExternalAccountData)?
        .try_into()
        .map_err(|_| error!(VaultError::InvalidExternalAccountData))?;

    Ok(Pubkey::new_from_array(bytes))
}

fn validate_external_accounts(ctx: &Context<ConfigureStrategyV1>) -> Result<()> {
    {
        let data = ctx.accounts.perpetual_store.try_borrow_data()?;

        require!(
            data.get(..8) == Some(GMTRADE_STORE_DISCRIMINATOR.as_slice()),
            VaultError::InvalidExternalAccountData
        );
    }

    let data = ctx.accounts.perpetual_market.try_borrow_data()?;

    // Published version-zero Market header and metadata layout.
    // This validates the fields used by configuration, not the full account.
    require!(data.len() >= 248, VaultError::InvalidExternalAccountData);

    require!(
        data.get(..8) == Some(GMTRADE_MARKET_DISCRIMINATOR.as_slice()),
        VaultError::InvalidExternalAccountData
    );

    require!(data[8] == 0, VaultError::InvalidExternalAccountData);

    let flags = data[10];
    let enabled = flags & (1 << 0) != 0;
    let closed = flags & (1 << 5) != 0;

    require!(enabled && !closed, VaultError::PerpetualMarketUnavailable);

    let index_identifier = read_pubkey(&data, 120)?;
    let long_collateral = read_pubkey(&data, 152)?;
    let short_collateral = read_pubkey(&data, 184)?;
    let store = read_pubkey(&data, 216)?;

    require!(
        index_identifier.to_string() == GMTRADE_XAU_INDEX_ADDRESS,
        VaultError::InvalidPerpetualIndex
    );

    require_keys_eq!(
        store,
        ctx.accounts.perpetual_store.key(),
        VaultError::InvalidPerpetualStore
    );

    require_keys_eq!(
        long_collateral,
        ctx.accounts.vault_v1_state.usdc_mint,
        VaultError::InvalidPerpetualCollateral
    );

    require_keys_eq!(
        short_collateral,
        ctx.accounts.vault_v1_state.usdc_mint,
        VaultError::InvalidPerpetualCollateral
    );

    Ok(())
}

pub fn handle_configure_strategy(ctx: Context<ConfigureStrategyV1>) -> Result<()> {
    validate_external_accounts(&ctx)?;

    ctx.accounts.strategy_v1_state.set_inner(StrategyV1State {
        vault: ctx.accounts.vault_v1_state.key(),
        gold_mint: ctx.accounts.gold_mint.key(),
        gold_custody: ctx.accounts.vault_v1_gold_account.key(),
        gold_token_program: ctx.accounts.gold_token_program.key(),
        perpetual_program: ctx.accounts.perpetual_program.key(),
        perpetual_store: ctx.accounts.perpetual_store.key(),
        perpetual_market: ctx.accounts.perpetual_market.key(),
        configured_at_slot: Clock::get()?.slot,
        execution_enabled: false,
        bump: ctx.bumps.strategy_v1_state,
    });

    Ok(())
}

// Configuration remains admin-only and starts with execution disabled.
// Funded vaults can configure now: deposit/redemption/settlement all authenticate
// full strategy NAV instead of treating configured strategies as idle custody.
