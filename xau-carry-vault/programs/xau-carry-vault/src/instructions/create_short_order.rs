use anchor_lang::{
    prelude::*,
    solana_program::program::invoke_signed,
    system_program::{self, Transfer},
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, TransferChecked},
};

use crate::{
    adapters::gmtrade::{create_short_order_instruction, ShortIncreaseParams, ShortOrderAccounts},
    constants::*,
    error::VaultError,
    state::{PendingShortOrderV1, StrategyV1State, VaultV1State},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreateShortOrderParamsV1 {
    pub collateral_amount: u64,
    pub size_delta_value: u128,
    pub acceptable_price: u128,
    pub execution_lamports: u64,
    pub funding_lamports: u64,
}

#[derive(Accounts)]
#[instruction(nonce: [u8; 32])]
pub struct CreateShortOrderV1<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [
            VAULT_SEED,
            usdc_mint.key().as_ref()
        ],
        bump = vault_v1_state.bump,
        has_one = admin @ VaultError::UnauthorizedStrategyAdmin,
        has_one = usdc_mint
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
        constraint = strategy_v1_state.execution_enabled
            @ VaultError::StrategyExecutionDisabled,
        has_one = perpetual_program
            @ VaultError::InvalidPerpetualProgram,
        has_one = perpetual_store
            @ VaultError::InvalidPerpetualStore,
        has_one = perpetual_market
            @ VaultError::InvalidPerpetualMarket
    )]
    pub strategy_v1_state: Box<Account<'info, StrategyV1State>>,

    #[account(
        constraint = usdc_mint.key().to_string()
            == MAINNET_USDC_MINT
            @ VaultError::InvalidStrategyDepositMint,
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
            STRATEGY_AUTHORITY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        constraint = strategy_authority.to_account_info().data_is_empty()
            @ VaultError::InvalidStrategyAuthority
    )]
    pub strategy_authority: SystemAccount<'info>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = strategy_authority
    )]
    pub strategy_usdc_source: Box<Account<'info, TokenAccount>>,

    /// CHECK: Exact program address and executable status are checked.
    #[account(
        constraint = perpetual_program.key().to_string()
            == GMTRADE_PROGRAM_ADDRESS
            @ VaultError::InvalidPerpetualProgram,
        executable
    )]
    pub perpetual_program: UncheckedAccount<'info>,

    /// CHECK: Address and owner are checked; discriminator in handler.
    #[account(
        constraint = perpetual_store.key().to_string()
            == GMTRADE_STORE_ADDRESS
            @ VaultError::InvalidPerpetualStore,
        owner = perpetual_program.key()
    )]
    pub perpetual_store: UncheckedAccount<'info>,

    /// CHECK: Address and owner are checked; metadata in handler.
    #[account(
        mut,
        constraint = perpetual_market.key().to_string()
            == GMTRADE_XAU_MARKET_ADDRESS
            @ VaultError::InvalidPerpetualMarket,
        owner = perpetual_program.key()
    )]
    pub perpetual_market: UncheckedAccount<'info>,

    /// CHECK: Owner and PDA are checked here.
    /// GMTrade additionally validates user initialization and identity.
    #[account(
        mut,
        owner = perpetual_program.key(),
        seeds = [
            GMTRADE_USER_SEED,
            perpetual_store.key().as_ref(),
            strategy_authority.key().as_ref()
        ],
        bump,
        seeds::program = perpetual_program.key()
    )]
    pub perpetual_user: UncheckedAccount<'info>,

    /// CHECK: Owner is checked here; PDA and identity in handler.
    #[account(
        mut,
        owner = perpetual_program.key()
    )]
    pub perpetual_position: UncheckedAccount<'info>,

    /// CHECK: The external order PDA is constrained.
    /// GMTrade initializes it through CPI.
    #[account(
        mut,
        seeds = [
            GMTRADE_ORDER_SEED,
            perpetual_store.key().as_ref(),
            strategy_authority.key().as_ref(),
            &nonce
        ],
        bump,
        seeds::program = perpetual_program.key()
    )]
    pub perpetual_order: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = perpetual_order
    )]
    pub order_usdc_escrow: Box<Account<'info, TokenAccount>>,

    // Singleton per vault. Existing pending state blocks another submission.
    #[account(
        init,
        payer = admin,
        space = 8 + PendingShortOrderV1::INIT_SPACE,
        seeds = [
            PENDING_SHORT_ORDER_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump
    )]
    pub pending_short_order: Account<'info, PendingShortOrderV1>,

    /// CHECK: Exact event-authority PDA is checked.
    #[account(
        seeds = [GMTRADE_EVENT_AUTHORITY_SEED],
        bump,
        seeds::program = perpetual_program.key()
    )]
    pub perpetual_event_authority: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
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

fn validate_market_and_position(ctx: &Context<CreateShortOrderV1>) -> Result<()> {
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

        require!(
            data[10] & (1 << 0) != 0 && data[10] & (1 << 5) == 0,
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
            ctx.accounts.usdc_mint.key(),
            VaultError::InvalidPerpetualCollateral
        );

        require_keys_eq!(
            read_pubkey(&data, 184)?,
            ctx.accounts.usdc_mint.key(),
            VaultError::InvalidPerpetualCollateral
        );

        read_pubkey(&data, 88)?
    };

    let store = ctx.accounts.perpetual_store.key();
    let authority = ctx.accounts.strategy_authority.key();
    let collateral = ctx.accounts.usdc_mint.key();
    let kind = [GMTRADE_SHORT_POSITION_KIND];

    let (expected_position, position_bump) = Pubkey::find_program_address(
        &[
            GMTRADE_POSITION_SEED,
            store.as_ref(),
            authority.as_ref(),
            market_token.as_ref(),
            collateral.as_ref(),
            &kind,
        ],
        &ctx.accounts.perpetual_program.key(),
    );

    require_keys_eq!(
        ctx.accounts.perpetual_position.key(),
        expected_position,
        VaultError::InvalidPerpetualPosition
    );

    let data = ctx.accounts.perpetual_position.try_borrow_data()?;

    require!(data.len() >= 152, VaultError::InvalidExternalAccountData);

    require!(
        data.get(..8) == Some(GMTRADE_POSITION_DISCRIMINATOR.as_slice()),
        VaultError::InvalidPerpetualPosition
    );

    require!(
        data[8] == 0 && data[9] == position_bump && data[42] == GMTRADE_SHORT_POSITION_KIND,
        VaultError::InvalidPerpetualPosition
    );

    require_keys_eq!(
        read_pubkey(&data, 10)?,
        store,
        VaultError::InvalidPerpetualPosition
    );

    require_keys_eq!(
        read_pubkey(&data, 56)?,
        authority,
        VaultError::InvalidPerpetualPosition
    );

    require_keys_eq!(
        read_pubkey(&data, 88)?,
        market_token,
        VaultError::InvalidPerpetualPosition
    );

    require_keys_eq!(
        read_pubkey(&data, 120)?,
        collateral,
        VaultError::InvalidPerpetualPosition
    );

    Ok(())
}

pub fn handle_create_short_order(
    ctx: Context<CreateShortOrderV1>,
    nonce: [u8; 32],
    params: CreateShortOrderParamsV1,
) -> Result<()> {
    require!(
        !ctx.accounts.vault_v1_state.strategy_paused,
        VaultError::StrategyPaused
    );

    require!(
        params.collateral_amount <= ctx.accounts.vault_v1_state.max_order_collateral
            && params.size_delta_value <= ctx.accounts.vault_v1_state.max_order_size,
        VaultError::OrderLimitExceeded
    );

    require!(
        params.collateral_amount > 0 && params.size_delta_value > 0 && params.acceptable_price > 0,
        VaultError::InvalidShortOrderParameters
    );

    validate_market_and_position(&ctx)?;

    require!(
        ctx.accounts.vault_v1_usdc_account.amount >= params.collateral_amount,
        VaultError::InsufficientStrategyCollateral
    );

    require!(
        ctx.accounts.order_usdc_escrow.amount == 0,
        VaultError::OrderEscrowNotEmpty
    );

    let custody_before = ctx.accounts.vault_v1_usdc_account.amount;
    let source_before = ctx.accounts.strategy_usdc_source.amount;

    let instruction = create_short_order_instruction(
        &ShortOrderAccounts {
            perpetual_program: ctx.accounts.perpetual_program.key(),
            strategy_authority: ctx.accounts.strategy_authority.key(),
            store: ctx.accounts.perpetual_store.key(),
            market: ctx.accounts.perpetual_market.key(),
            user: ctx.accounts.perpetual_user.key(),
            order: ctx.accounts.perpetual_order.key(),
            position: ctx.accounts.perpetual_position.key(),
            usdc_mint: ctx.accounts.usdc_mint.key(),
            order_usdc_escrow: ctx.accounts.order_usdc_escrow.key(),
            strategy_usdc_source: ctx.accounts.strategy_usdc_source.key(),
        },
        &nonce,
        &ShortIncreaseParams {
            collateral_amount: params.collateral_amount,
            size_delta_value: params.size_delta_value,
            acceptable_price: params.acceptable_price,
            execution_lamports: params.execution_lamports,
        },
    )?;

    if params.funding_lamports > 0 {
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.admin.to_account_info(),
                    to: ctx.accounts.strategy_authority.to_account_info(),
                },
            ),
            params.funding_lamports,
        )?;
    }

    // First signer: vault state authority, controlling idle USDC custody.
    let usdc_key = ctx.accounts.usdc_mint.key();
    let vault_bump = [ctx.accounts.vault_v1_state.bump];

    let vault_seeds: &[&[u8]] = &[VAULT_SEED, usdc_key.as_ref(), &vault_bump];

    token::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.vault_v1_usdc_account.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.strategy_usdc_source.to_account_info(),
                authority: ctx.accounts.vault_v1_state.to_account_info(),
            },
            &[vault_seeds],
        ),
        params.collateral_amount,
        RECEIPT_DECIMALS,
    )?;

    // Second signer: system-owned strategy authority, owning the
    // GMTrade user, position, order, and collateral-source ATA.
    let vault_key = ctx.accounts.vault_v1_state.key();
    let authority_bump = [ctx.bumps.strategy_authority];

    let authority_seeds: &[&[u8]] = &[STRATEGY_AUTHORITY_SEED, vault_key.as_ref(), &authority_bump];

    invoke_signed(
        &instruction,
        &[
            ctx.accounts.strategy_authority.to_account_info(),
            ctx.accounts.perpetual_store.to_account_info(),
            ctx.accounts.perpetual_market.to_account_info(),
            ctx.accounts.perpetual_user.to_account_info(),
            ctx.accounts.perpetual_order.to_account_info(),
            ctx.accounts.perpetual_position.to_account_info(),
            ctx.accounts.usdc_mint.to_account_info(),
            ctx.accounts.order_usdc_escrow.to_account_info(),
            ctx.accounts.strategy_usdc_source.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
            ctx.accounts.associated_token_program.to_account_info(),
            ctx.accounts.perpetual_event_authority.to_account_info(),
            ctx.accounts.perpetual_program.to_account_info(),
        ],
        &[authority_seeds],
    )?;

    {
        let order_info = ctx.accounts.perpetual_order.to_account_info();

        require_keys_eq!(
            *order_info.owner,
            ctx.accounts.perpetual_program.key(),
            VaultError::InvalidPerpetualOrderAccounts
        );

        let data = order_info.try_borrow_data()?;

        require!(
            data.get(..8) == Some(GMTRADE_ORDER_DISCRIMINATOR.as_slice()),
            VaultError::InvalidPerpetualOrderAccounts
        );
    }

    ctx.accounts.vault_v1_usdc_account.reload()?;
    ctx.accounts.strategy_usdc_source.reload()?;
    ctx.accounts.order_usdc_escrow.reload()?;

    let expected_custody = custody_before
        .checked_sub(params.collateral_amount)
        .ok_or(VaultError::InvalidOrderCollateralAccounting)?;

    require!(
        ctx.accounts.vault_v1_usdc_account.amount == expected_custody
            && ctx.accounts.strategy_usdc_source.amount == source_before
            && ctx.accounts.order_usdc_escrow.amount == params.collateral_amount,
        VaultError::InvalidOrderCollateralAccounting
    );

    ctx.accounts
        .pending_short_order
        .set_inner(PendingShortOrderV1 {
            vault: vault_key,
            order: ctx.accounts.perpetual_order.key(),
            position: ctx.accounts.perpetual_position.key(),
            strategy_authority: ctx.accounts.strategy_authority.key(),
            nonce,
            collateral_amount: params.collateral_amount,
            size_delta_value: params.size_delta_value,
            acceptable_price: params.acceptable_price,
            execution_lamports: params.execution_lamports,
            submitted_at_slot: Clock::get()?.slot,
            bump: ctx.bumps.pending_short_order,
        });

    Ok(())
}
