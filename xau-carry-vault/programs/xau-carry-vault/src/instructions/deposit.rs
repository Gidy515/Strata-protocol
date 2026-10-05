use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{
        self,
        Mint,
        MintTo,
        Token,
        TokenAccount,
        TransferChecked,
    },
};

use crate::{
    constants::*,
    error::VaultError,
    state::VaultV1State,
};

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
    pub vault_v1_state: Account<'info, VaultV1State>,

    #[account(
        constraint = usdc_mint.decimals == RECEIPT_DECIMALS
            @ VaultError::InvalidDecimals
    )]
    pub usdc_mint: Account<'info, Mint>,

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
    pub vault_v1_usdc_account: Account<'info, TokenAccount>,

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
    pub vxau_mint: Account<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = user
    )]
    pub user_usdc_ata: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = vxau_mint,
        associated_token::authority = user
    )]
    pub user_vxau_ata: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_deposit(
    ctx: Context<DepositUsdcV1>,
    amount: u64,
) -> Result<()> {
    require!(amount > 0, VaultError::ZeroAmount);

    let next_total = ctx
        .accounts
        .vault_v1_state
        .total_deposited
        .checked_add(amount)
        .ok_or(VaultError::Overflow)?;

    let transfer_accounts = TransferChecked {
        from: ctx.accounts.user_usdc_ata.to_account_info(),
        mint: ctx.accounts.usdc_mint.to_account_info(),
        to: ctx.accounts.vault_v1_usdc_account.to_account_info(),
        authority: ctx.accounts.user.to_account_info(),
    };

    token::transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            transfer_accounts,
        ),
        amount,
        RECEIPT_DECIMALS,
    )?;

    let usdc_mint_key = ctx.accounts.usdc_mint.key();
    let bump = [ctx.accounts.vault_v1_state.bump];

    let vault_seeds: &[&[u8]] = &[
        VAULT_SEED,
        usdc_mint_key.as_ref(),
        &bump,
    ];

    let signer_seeds: &[&[&[u8]]] = &[vault_seeds];

    let mint_accounts = MintTo {
        mint: ctx.accounts.vxau_mint.to_account_info(),
        to: ctx.accounts.user_vxau_ata.to_account_info(),
        authority: ctx.accounts.vault_v1_state.to_account_info(),
    };

    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            mint_accounts,
            signer_seeds,
        ),
        amount,
    )?;

    ctx.accounts.vault_v1_state.total_deposited = next_total;

    Ok(())
}