use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::{
    constants::*,
    error::VaultError,
    state::VaultV1State,
};

#[derive(Accounts)]
pub struct InitializeVaultV1<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        constraint = program.programdata_address()?
            == Some(program_data.key())
            @ VaultError::Unauthorized
    )]
    pub program: Program<'info, crate::program::XauCarryVault>,

    #[account(
        constraint = program_data.upgrade_authority_address
            == Some(admin.key())
            @ VaultError::Unauthorized
    )]
    pub program_data: Account<'info, ProgramData>,

    #[account(
        constraint = usdc_mint.decimals == RECEIPT_DECIMALS
            @ VaultError::InvalidDecimals
    )]
    pub usdc_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = admin,
        space = 8 + VaultV1State::INIT_SPACE,
        seeds = [
            VAULT_SEED,
            usdc_mint.key().as_ref()
        ],
        bump
    )]
    pub vault_v1_state: Account<'info, VaultV1State>,

    #[account(
        init,
        payer = admin,
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
        init,
        payer = admin,
        seeds = [
            VXAU_MINT_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        mint::decimals = RECEIPT_DECIMALS,
        mint::authority = vault_v1_state
    )]
    pub vxau_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handle_initialize(
    ctx: Context<InitializeVaultV1>,
) -> Result<()> {
    ctx.accounts.vault_v1_state.set_inner(VaultV1State {
        admin: ctx.accounts.admin.key(),
        usdc_mint: ctx.accounts.usdc_mint.key(),
        vxau_mint: ctx.accounts.vxau_mint.key(),
        total_deposited: 0,
        bump: ctx.bumps.vault_v1_state,
    });

    Ok(())
}