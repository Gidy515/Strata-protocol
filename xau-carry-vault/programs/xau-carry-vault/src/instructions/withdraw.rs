use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Burn, Mint, Token, TokenAccount, TransferChecked},
};

use crate::{constants::*, error::VaultError, state::VaultV1State};

#[derive(Accounts)]
pub struct WithdrawUsdcV1<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        seeds = [
            VAULT_SEED,
            usdc_mint.key().as_ref()
        ],
        bump = vault_v1_state.bump,
        has_one = usdc_mint,
        has_one = vxau_mint
    )]
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,

    #[account(
        constraint = usdc_mint.decimals == RECEIPT_DECIMALS
            @ VaultError::InvalidDecimals
    )]
    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [
            TOKEN_VAULT_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        token::mint = usdc_mint,
        token::authority = vault_v1_state
    )]
    pub vault_v1_usdc_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [
            VXAU_MINT_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        mint::decimals = RECEIPT_DECIMALS,
        mint::authority = vault_v1_state
    )]
    pub vxau_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = vxau_mint,
        associated_token::authority = user
    )]
    pub user_vxau_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = usdc_mint,
        associated_token::authority = user
    )]
    pub user_usdc_ata: Box<Account<'info, TokenAccount>>,

    /// CHECK: Canonical strategy PDA is constrained.
    /// The handler requires strategy configuration to be absent.
    #[account(
        seeds = [
            STRATEGY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump
    )]
    pub strategy_account: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_withdraw(
    ctx: Context<WithdrawUsdcV1>,
    amount: u64,
    min_assets_out: u64,
) -> Result<()> {
    require!(
        !ctx.accounts.vault_v1_state.withdrawals_paused,
        VaultError::WithdrawalsPaused
    );
    // amount is the number of vXAU base units to burn.
    require!(amount > 0, VaultError::ZeroAmount);
    require!(min_assets_out > 0, VaultError::InvalidMinimumOutput);

    crate::instructions::read_idle_nav::require_idle_strategy(
        &ctx.accounts.strategy_account.to_account_info(),
    )?;

    require!(
        ctx.accounts.user_vxau_ata.amount >= amount,
        VaultError::InsufficientReceipts
    );

    let assets_out = crate::share_math::assets_for_redemption(
        amount,
        ctx.accounts.vxau_mint.supply,
        ctx.accounts.vault_v1_usdc_account.amount,
    )?;

    // Never burn shares for an output rounded down to zero.
    require!(assets_out > 0, VaultError::RedemptionTooSmall);

    require!(
        assets_out >= min_assets_out,
        VaultError::MinimumOutputNotMet
    );

    require!(
        ctx.accounts.vault_v1_usdc_account.amount >= assets_out,
        VaultError::InsufficientLiquidity
    );

    token::burn(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            Burn {
                mint: ctx.accounts.vxau_mint.to_account_info(),
                from: ctx.accounts.user_vxau_ata.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        amount,
    )?;

    let usdc_mint_key = ctx.accounts.usdc_mint.key();
    let bump = [ctx.accounts.vault_v1_state.bump];

    let vault_seeds: &[&[u8]] = &[VAULT_SEED, usdc_mint_key.as_ref(), &bump];

    token::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.vault_v1_usdc_account.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.user_usdc_ata.to_account_info(),
                authority: ctx.accounts.vault_v1_state.to_account_info(),
            },
            &[vault_seeds],
        ),
        assets_out,
        RECEIPT_DECIMALS,
    )?;

    // Historical cumulative deposits remain unchanged.
    Ok(())
}
