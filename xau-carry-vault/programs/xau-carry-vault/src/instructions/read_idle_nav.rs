use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, TokenAccount};

use crate::{constants::*, error::VaultError, state::VaultV1State};

#[derive(Accounts)]
pub struct ReadIdleNavV1<'info> {
    #[account(
        seeds = [
            VAULT_SEED,
            usdc_mint.key().as_ref()
        ],
        bump = vault_v1_state.bump,
        has_one = usdc_mint,
        has_one = vxau_mint
    )]
    pub vault_v1_state: Account<'info, VaultV1State>,

    #[account(
        constraint = usdc_mint.decimals == RECEIPT_DECIMALS
            @ VaultError::InvalidDecimals
    )]
    pub usdc_mint: Account<'info, Mint>,

    #[account(
        seeds = [
            TOKEN_VAULT_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        token::mint = usdc_mint,
        token::authority = vault_v1_state
    )]
    pub vault_v1_usdc_account: Account<'info, TokenAccount>,

    #[account(
        seeds = [
            VXAU_MINT_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        mint::decimals = RECEIPT_DECIMALS,
        mint::authority = vault_v1_state
    )]
    pub vxau_mint: Account<'info, Mint>,

    /// CHECK: The canonical strategy PDA is constrained here.
    /// The handler requires an empty, system-owned account,
    /// proving that strategy configuration has not been created.
    #[account(
        seeds = [
            STRATEGY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump
    )]
    pub strategy_account: UncheckedAccount<'info>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct IdleNavQuoteV1 {
    /// USDC base units currently held in canonical custody.
    pub net_assets: u64,

    /// Actual outstanding vXAU base units.
    pub share_supply: u64,

    /// Slot at which these accounts were read.
    pub observed_slot: u64,
}

pub fn handle_read_idle_nav(ctx: Context<ReadIdleNavV1>) -> Result<IdleNavQuoteV1> {
    require_idle_strategy(&ctx.accounts.strategy_account.to_account_info())?;

    Ok(IdleNavQuoteV1 {
        net_assets: ctx.accounts.vault_v1_usdc_account.amount,
        share_supply: ctx.accounts.vxau_mint.supply,
        observed_slot: Clock::get()?.slot,
    })
}

pub fn require_idle_strategy(strategy_info: &AccountInfo<'_>) -> Result<()> {
    require!(
        *strategy_info.owner == anchor_lang::system_program::ID
            && strategy_info.data_is_empty()
            && !strategy_info.executable,
        VaultError::FullStrategyNavRequired
    );

    Ok(())
}
