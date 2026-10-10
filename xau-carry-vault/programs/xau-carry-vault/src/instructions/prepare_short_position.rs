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
pub struct PrepareShortPositionV1<'info> {
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
            @ VaultError::InvalidPerpetualStore,
        has_one = perpetual_market
            @ VaultError::InvalidPerpetualMarket
    )]
    pub strategy_v1_state: Account<'info, StrategyV1State>,

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

    /// CHECK: Exact program address and executable status are checked.
    #[account(
        constraint = perpetual_program.key().to_string()
            == GMTRADE_PROGRAM_ADDRESS
            @ VaultError::InvalidPerpetualProgram,
        executable
    )]
    pub perpetual_program: UncheckedAccount<'info>,

    /// CHECK: Address and owner are constrained.
    /// The handler also checks the discriminator.
    #[account(
        constraint = perpetual_store.key().to_string()
            == GMTRADE_STORE_ADDRESS
            @ VaultError::InvalidPerpetualStore,
        owner = perpetual_program.key()
    )]
    pub perpetual_store: UncheckedAccount<'info>,

    /// CHECK: Address and owner are constrained.
    /// The handler checks market metadata and status flags.
    #[account(
        constraint = perpetual_market.key().to_string()
            == GMTRADE_XAU_MARKET_ADDRESS
            @ VaultError::InvalidPerpetualMarket,
        owner = perpetual_program.key()
    )]
    pub perpetual_market: UncheckedAccount<'info>,

    /// CHECK: The handler derives and checks the GMTrade position PDA.
    /// GMTrade initializes or validates it through CPI.
    /// Resulting ownership and identity are checked after CPI.
    #[account(mut)]
    pub perpetual_position: UncheckedAccount<'info>,

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

// Encodes CreateOrderParams for position preparation only.
// These zero-value parameters must not be reused for order creation.
fn preparation_instruction_data() -> Vec<u8> {
    let mut data = GMTRADE_PREPARE_POSITION_DISCRIMINATOR.to_vec();

    // OrderKind::MarketIncrease.
    data.push(3);

    // decrease_position_swap_type: None.
    data.push(0);

    // execution_lamports.
    data.extend_from_slice(&0u64.to_le_bytes());

    // swap_path_length.
    data.push(0);

    // initial_collateral_delta_amount.
    data.extend_from_slice(&0u64.to_le_bytes());

    // size_delta_value.
    data.extend_from_slice(&0u128.to_le_bytes());

    // is_long: false.
    data.push(0);

    // is_collateral_long: false.
    // Both collateral sides of the selected market are USDC.
    data.push(0);

    // min_output: None.
    data.push(0);

    // trigger_price: None.
    data.push(0);

    // acceptable_price: None.
    data.push(0);

    // should_unwrap_native_token: false.
    data.push(0);

    // valid_from_ts: None.
    data.push(0);

    data
}

pub fn handle_prepare_short_position(
    ctx: Context<PrepareShortPositionV1>,
    funding_lamports: u64,
) -> Result<()> {
    {
        let data = ctx.accounts.perpetual_store.try_borrow_data()?;

        require!(
            data.get(..8) == Some(GMTRADE_STORE_DISCRIMINATOR.as_slice()),
            VaultError::InvalidExternalAccountData
        );
    }

    let market_token = {
        let data = ctx.accounts.perpetual_market.try_borrow_data()?;

        require!(data.len() >= 248, VaultError::InvalidExternalAccountData);

        require!(
            data.get(..8) == Some(GMTRADE_MARKET_DISCRIMINATOR.as_slice()),
            VaultError::InvalidExternalAccountData
        );

        require!(data[8] == 0, VaultError::InvalidExternalAccountData);

        let flags = data[10];

        require!(
            flags & (1 << 0) != 0 && flags & (1 << 5) == 0,
            VaultError::PerpetualMarketUnavailable
        );

        require_keys_eq!(
            read_pubkey(&data, 216)?,
            ctx.accounts.perpetual_store.key(),
            VaultError::InvalidPerpetualStore
        );

        require!(
            read_pubkey(&data, 120)?.to_string() == GMTRADE_XAU_INDEX_ADDRESS,
            VaultError::InvalidPerpetualIndex
        );

        require_keys_eq!(
            read_pubkey(&data, 152)?,
            ctx.accounts.vault_v1_state.usdc_mint,
            VaultError::InvalidPerpetualCollateral
        );

        require_keys_eq!(
            read_pubkey(&data, 184)?,
            ctx.accounts.vault_v1_state.usdc_mint,
            VaultError::InvalidPerpetualCollateral
        );

        read_pubkey(&data, 88)?
    };

    let store_key = ctx.accounts.perpetual_store.key();
    let authority_key = ctx.accounts.strategy_authority.key();
    let collateral_key = ctx.accounts.vault_v1_state.usdc_mint;
    let program_key = ctx.accounts.perpetual_program.key();
    let position_kind = [GMTRADE_SHORT_POSITION_KIND];

    let (expected_position, position_bump) = Pubkey::find_program_address(
        &[
            GMTRADE_POSITION_SEED,
            store_key.as_ref(),
            authority_key.as_ref(),
            market_token.as_ref(),
            collateral_key.as_ref(),
            &position_kind,
        ],
        &program_key,
    );

    require_keys_eq!(
        ctx.accounts.perpetual_position.key(),
        expected_position,
        VaultError::InvalidPerpetualPosition
    );

    // The administrator supplies SOL for external account rent.
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

    let instruction = Instruction {
        program_id: program_key,
        accounts: vec![
            AccountMeta::new(authority_key, true),
            AccountMeta::new_readonly(store_key, false),
            AccountMeta::new_readonly(ctx.accounts.perpetual_market.key(), false),
            AccountMeta::new(expected_position, false),
            AccountMeta::new_readonly(ctx.accounts.system_program.key(), false),
        ],
        data: preparation_instruction_data(),
    };

    invoke_signed(
        &instruction,
        &[
            ctx.accounts.strategy_authority.to_account_info(),
            ctx.accounts.perpetual_store.to_account_info(),
            ctx.accounts.perpetual_market.to_account_info(),
            ctx.accounts.perpetual_position.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.perpetual_program.to_account_info(),
        ],
        &[authority_seeds],
    )?;

    let position_info = ctx.accounts.perpetual_position.to_account_info();

    require_keys_eq!(
        *position_info.owner,
        program_key,
        VaultError::InvalidPerpetualPosition
    );

    {
        let data = position_info.try_borrow_data()?;

        // Published version-zero Position prefix:
        // discriminator: 0..8
        // version: 8
        // bump: 9
        // store: 10..42
        // kind: 42
        // padding: 43..48
        // created_at: 48..56
        // owner: 56..88
        // market_token: 88..120
        // collateral_token: 120..152
        require!(data.len() >= 152, VaultError::InvalidExternalAccountData);

        require!(
            data.get(..8) == Some(GMTRADE_POSITION_DISCRIMINATOR.as_slice()),
            VaultError::InvalidPerpetualPosition
        );

        require!(data[8] == 0, VaultError::InvalidExternalAccountData);

        require!(
            data[9] == position_bump && data[42] == GMTRADE_SHORT_POSITION_KIND,
            VaultError::InvalidPerpetualPosition
        );

        require_keys_eq!(
            read_pubkey(&data, 10)?,
            store_key,
            VaultError::InvalidPerpetualPosition
        );

        require_keys_eq!(
            read_pubkey(&data, 56)?,
            authority_key,
            VaultError::InvalidPerpetualPosition
        );

        require_keys_eq!(
            read_pubkey(&data, 88)?,
            market_token,
            VaultError::InvalidPerpetualPosition
        );

        require_keys_eq!(
            read_pubkey(&data, 120)?,
            collateral_key,
            VaultError::InvalidPerpetualPosition
        );
    }

    msg!("Prepared GMTrade XAU short position {}", expected_position);

    Ok(())
}
