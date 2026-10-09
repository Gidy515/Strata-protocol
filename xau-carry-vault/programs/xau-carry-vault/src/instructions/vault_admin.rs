use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, TransferChecked},
};

use crate::{constants::*, error::VaultError, state::VaultV1State};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct VaultControlsV1 {
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
    pub strategy_paused: bool,
    pub max_order_collateral: u64,
    pub max_order_size: u128,
}

#[derive(Accounts)]
pub struct ManageVaultV1<'info> {
    pub admin: Signer<'info>,

    #[account(
        mut,
        seeds = [VAULT_SEED, vault_v1_state.usdc_mint.as_ref()],
        bump = vault_v1_state.bump,
        has_one = admin @ VaultError::UnauthorizedStrategyAdmin
    )]
    pub vault_v1_state: Account<'info, VaultV1State>,
}

pub fn handle_set_controls(ctx: Context<ManageVaultV1>, controls: VaultControlsV1) -> Result<()> {
    require!(
        controls.max_order_collateral > 0 && controls.max_order_size > 0,
        VaultError::InvalidOrderLimits
    );

    let state = &mut ctx.accounts.vault_v1_state;
    state.deposits_paused = controls.deposits_paused;
    state.withdrawals_paused = controls.withdrawals_paused;
    state.strategy_paused = controls.strategy_paused;
    state.max_order_collateral = controls.max_order_collateral;
    state.max_order_size = controls.max_order_size;

    // This instruction never changes execution_enabled.
    Ok(())
}

pub fn handle_propose_admin(ctx: Context<ManageVaultV1>, new_admin: Pubkey) -> Result<()> {
    require!(
        new_admin != Pubkey::default() && new_admin != ctx.accounts.admin.key(),
        VaultError::InvalidProposedAdmin
    );

    ctx.accounts.vault_v1_state.pending_admin = new_admin;
    Ok(())
}

#[derive(Accounts)]
pub struct AcceptVaultAdminV1<'info> {
    pub new_admin: Signer<'info>,

    #[account(
        mut,
        seeds = [VAULT_SEED, vault_v1_state.usdc_mint.as_ref()],
        bump = vault_v1_state.bump,
        constraint = vault_v1_state.pending_admin == new_admin.key()
            @ VaultError::NotPendingAdmin
    )]
    pub vault_v1_state: Account<'info, VaultV1State>,
}

pub fn handle_accept_admin(ctx: Context<AcceptVaultAdminV1>) -> Result<()> {
    ctx.accounts.vault_v1_state.admin = ctx.accounts.new_admin.key();
    ctx.accounts.vault_v1_state.pending_admin = Pubkey::default();
    Ok(())
}

#[derive(Accounts)]
pub struct RecoverUnownedCustodyV1<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [VAULT_SEED, usdc_mint.key().as_ref()],
        bump = vault_v1_state.bump,
        has_one = admin @ VaultError::UnauthorizedStrategyAdmin,
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
        seeds = [VXAU_MINT_SEED, vault_v1_state.key().as_ref()],
        bump,
        mint::authority = vault_v1_state,
        mint::decimals = RECEIPT_DECIMALS
    )]
    pub vxau_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [TOKEN_VAULT_SEED, vault_v1_state.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = vault_v1_state
    )]
    pub vault_v1_usdc_account: Box<Account<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = admin
    )]
    pub admin_usdc_ata: Box<Account<'info, TokenAccount>>,

    /// CHECK: Canonical strategy PDA; handler requires it to be absent.
    #[account(
        seeds = [STRATEGY_SEED, vault_v1_state.key().as_ref()],
        bump
    )]
    pub strategy_account: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_recover_unowned(ctx: Context<RecoverUnownedCustodyV1>) -> Result<()> {
    super::read_idle_nav::require_idle_strategy(&ctx.accounts.strategy_account.to_account_info())?;

    require!(
        ctx.accounts.vxau_mint.supply == 0,
        VaultError::OutstandingShares
    );

    let amount = ctx.accounts.vault_v1_usdc_account.amount;
    require!(amount > 0, VaultError::ZeroAmount);

    let mint_key = ctx.accounts.usdc_mint.key();
    let bump = [ctx.accounts.vault_v1_state.bump];
    let seeds: &[&[u8]] = &[VAULT_SEED, mint_key.as_ref(), &bump];

    token::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.vault_v1_usdc_account.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.admin_usdc_ata.to_account_info(),
                authority: ctx.accounts.vault_v1_state.to_account_info(),
            },
            &[seeds],
        ),
        amount,
        RECEIPT_DECIMALS,
    )?;

    Ok(())
}
