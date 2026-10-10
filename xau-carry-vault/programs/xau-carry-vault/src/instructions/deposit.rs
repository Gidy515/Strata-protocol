use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, MintTo, Token, TokenAccount, TransferChecked},
};

use crate::{constants::*, error::VaultError, state::VaultV1State};

#[derive(Accounts)]
pub struct DepositUsdcV1<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
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
        associated_token::mint = usdc_mint,
        associated_token::authority = user
    )]
    pub user_usdc_ata: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = vxau_mint,
        associated_token::authority = user
    )]
    pub user_vxau_ata: Box<Account<'info, TokenAccount>>,

    /// CHECK: Canonical strategy PDA is constrained.
    /// Configured strategies require the authenticated NAV remaining accounts.
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

pub fn handle_deposit(ctx: Context<DepositUsdcV1>, amount: u64, min_shares_out: u64) -> Result<()> {
    require!(
        !ctx.accounts.vault_v1_state.deposits_paused,
        VaultError::DepositsPaused
    );
    require!(amount > 0, VaultError::ZeroAmount);
    require!(min_shares_out > 0, VaultError::InvalidMinimumOutput);

    let nav = crate::strategy_nav::quote(
        ctx.accounts.vault_v1_state.key(),
        &ctx.accounts.vault_v1_state,
        &ctx.accounts.strategy_account.to_account_info(),
        ctx.accounts.vault_v1_usdc_account.amount,
        ctx.remaining_accounts,
    )?;

    let next_total = ctx
        .accounts
        .vault_v1_state
        .total_deposited
        .checked_add(amount)
        .ok_or(VaultError::Overflow)?;

    // Read NAV and outstanding supply BEFORE the deposit.
    let shares_out = if *ctx.accounts.strategy_account.owner == anchor_lang::system_program::ID {
        crate::share_math::shares_for_deposit(
            amount,
            ctx.accounts.vxau_mint.supply,
            nav.net_assets,
        )?
    } else {
        crate::strategy_accounting::deposit_quote(
            amount,
            ctx.accounts.vxau_mint.supply,
            nav,
            min_shares_out,
        )
        .map_err(crate::strategy_nav::accounting_error)?
    };

    require!(
        shares_out >= min_shares_out,
        VaultError::MinimumOutputNotMet
    );

    token::transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.user_usdc_ata.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.vault_v1_usdc_account.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        amount,
        RECEIPT_DECIMALS,
    )?;

    let usdc_mint_key = ctx.accounts.usdc_mint.key();
    let bump = [ctx.accounts.vault_v1_state.bump];

    let vault_seeds: &[&[u8]] = &[VAULT_SEED, usdc_mint_key.as_ref(), &bump];

    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            MintTo {
                mint: ctx.accounts.vxau_mint.to_account_info(),
                to: ctx.accounts.user_vxau_ata.to_account_info(),
                authority: ctx.accounts.vault_v1_state.to_account_info(),
            },
            &[vault_seeds],
        ),
        shares_out,
    )?;

    ctx.accounts.vault_v1_state.total_deposited = next_total;

    Ok(())
}
