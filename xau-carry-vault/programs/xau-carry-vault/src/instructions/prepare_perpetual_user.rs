use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke_signed,
    },
    system_program::{self, Transfer},
};

use crate::{
    constants::*,
    error::VaultError,
    state::{StrategyV1State, VaultV1State},
};

#[derive(Accounts)]
pub struct PreparePerpetualUserV1<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [
            VAULT_SEED,
            vault_v1_state.usdc_mint.as_ref()
        ],
        bump = vault_v1_state.bump,
        has_one = admin @ VaultError::UnauthorizedStrategyAdmin
    )]
    pub vault_v1_state: Account<'info, VaultV1State>,

    #[account(
        seeds = [
            STRATEGY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump = strategy_v1_state.bump,
        constraint = strategy_v1_state.vault
            == vault_v1_state.key()
            @ VaultError::InvalidStrategyVault,
        has_one = perpetual_program
            @ VaultError::InvalidPerpetualProgram,
        has_one = perpetual_store
            @ VaultError::InvalidPerpetualStore
    )]
    pub strategy_v1_state: Account<'info, StrategyV1State>,

    // This PDA stays system-owned so it can pay rent during GMTrade CPIs.
    // It requires no program-owned state account or private key.
    #[account(
        mut,
        seeds = [
            STRATEGY_AUTHORITY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        constraint = strategy_authority.to_account_info().data_is_empty()
            @ VaultError::InvalidStrategyAuthority
    )]
    pub strategy_authority: SystemAccount<'info>,

    /// CHECK: Pinned executable program, also linked through configuration.
    #[account(
        constraint = perpetual_program.key().to_string()
            == GMTRADE_PROGRAM_ADDRESS
            @ VaultError::InvalidPerpetualProgram,
        executable
    )]
    pub perpetual_program: UncheckedAccount<'info>,

    /// CHECK: Pinned address and owner; discriminator checked in handler.
    #[account(
        constraint = perpetual_store.key().to_string()
            == GMTRADE_STORE_ADDRESS
            @ VaultError::InvalidPerpetualStore,
        owner = perpetual_program.key()
    )]
    pub perpetual_store: UncheckedAccount<'info>,

    /// CHECK: GMTrade PDA seeds are checked here.
    /// GMTrade initializes or validates the account through CPI.
    /// Its owner and identity fields are checked again after CPI.
    #[account(
        mut,
        seeds = [
            GMTRADE_USER_SEED,
            perpetual_store.key().as_ref(),
            strategy_authority.key().as_ref()
        ],
        bump,
        seeds::program = perpetual_program.key()
    )]
    pub perpetual_user: UncheckedAccount<'info>,

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

pub fn handle_prepare_perpetual_user(
    ctx: Context<PreparePerpetualUserV1>,
    funding_lamports: u64,
) -> Result<()> {
    // Validate before funding or invoking the external program.
    {
        let data = ctx.accounts.perpetual_store.try_borrow_data()?;

        require!(
            data.get(..8) == Some(GMTRADE_STORE_DISCRIMINATOR.as_slice()),
            VaultError::InvalidExternalAccountData
        );
    }

    // Optional administrator-funded rent budget.
    // A later call can use zero if the authority already has enough SOL.
    if funding_lamports > 0 {
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.admin.to_account_info(),
                    to: ctx.accounts.strategy_authority.to_account_info(),
                },
            ),
            funding_lamports,
        )?;
    }

    let vault_key = ctx.accounts.vault_v1_state.key();
    let authority_bump = [ctx.bumps.strategy_authority];

    let authority_seeds: &[&[u8]] = &[STRATEGY_AUTHORITY_SEED, vault_key.as_ref(), &authority_bump];

    // Published prepare_user ABI:
    // owner: writable signer
    // store: readonly
    // user: writable
    // system_program: readonly
    // No instruction arguments.
    let instruction = Instruction {
        program_id: ctx.accounts.perpetual_program.key(),
        accounts: vec![
            AccountMeta::new(ctx.accounts.strategy_authority.key(), true),
            AccountMeta::new_readonly(ctx.accounts.perpetual_store.key(), false),
            AccountMeta::new(ctx.accounts.perpetual_user.key(), false),
            AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
        ],
        data: GMTRADE_PREPARE_USER_DISCRIMINATOR.to_vec(),
    };

    invoke_signed(
        &instruction,
        &[
            ctx.accounts.strategy_authority.to_account_info(),
            ctx.accounts.perpetual_store.to_account_info(),
            ctx.accounts.perpetual_user.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.perpetual_program.to_account_info(),
        ],
        &[authority_seeds],
    )?;

    // Validate the resulting account rather than treating CPI success
    // alone as proof that our expected account identity was established.
    let user_info = ctx.accounts.perpetual_user.to_account_info();

    require_keys_eq!(
        *user_info.owner,
        ctx.accounts.perpetual_program.key(),
        VaultError::InvalidPerpetualUser
    );

    {
        let data = user_info.try_borrow_data()?;

        // Published version-zero UserHeader prefix:
        // discriminator: 0..8
        // version, bump, flags, padding: 8..24
        // owner: 24..56
        // store: 56..88
        require!(data.len() >= 88, VaultError::InvalidExternalAccountData);

        require!(
            data.get(..8) == Some(GMTRADE_USER_DISCRIMINATOR.as_slice()),
            VaultError::InvalidPerpetualUser
        );

        require!(data[8] == 0, VaultError::InvalidExternalAccountData);

        require!(
            data[9] == ctx.bumps.perpetual_user,
            VaultError::InvalidPerpetualUser
        );

        require_keys_eq!(
            read_pubkey(&data, 24)?,
            ctx.accounts.strategy_authority.key(),
            VaultError::InvalidPerpetualUser
        );

        require_keys_eq!(
            read_pubkey(&data, 56)?,
            ctx.accounts.perpetual_store.key(),
            VaultError::InvalidPerpetualUser
        );
    }

    msg!(
        "Prepared GMTrade user {} for strategy authority {}",
        ctx.accounts.perpetual_user.key(),
        ctx.accounts.strategy_authority.key(),
    );

    Ok(())
}
