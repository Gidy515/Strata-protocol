use anchor_lang::{prelude::*, solana_program::program::invoke_signed};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{self, Mint, Token, TokenAccount, TransferChecked},
};

use crate::{
    adapters::gmtrade::{close_short_order_instruction, ShortOrderAccounts},
    constants::*,
    error::VaultError,
    state::{PendingShortOrderV1, StrategyV1State, VaultV1State},
};

#[derive(Accounts)]
pub struct CancelShortOrderV1<'info> {
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

    // No execution_enabled constraint: recovery must work while disabled.
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
    pub strategy_v1_state: Box<Account<'info, StrategyV1State>>,

    #[account(
        mut,
        close = admin,
        seeds = [
            PENDING_SHORT_ORDER_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump = pending_short_order.bump,
        constraint = pending_short_order.vault
            == vault_v1_state.key()
            @ VaultError::InvalidPendingOrder,
        constraint = pending_short_order.order
            == perpetual_order.key()
            @ VaultError::InvalidPendingOrder,
        has_one = strategy_authority
            @ VaultError::InvalidPendingOrder
    )]
    pub pending_short_order: Box<Account<'info, PendingShortOrderV1>>,

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
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = strategy_authority
    )]
    pub strategy_usdc_source: Box<Account<'info, TokenAccount>>,

    /// CHECK: Program address and executable status checked in handler.
    pub perpetual_program: UncheckedAccount<'info>,

    /// CHECK: Address and owner checked in handler.
    #[account(mut)]
    pub perpetual_store: UncheckedAccount<'info>,

    // SystemAccount checks system ownership; PDA checked in handler.
    #[account(mut)]
    pub perpetual_store_wallet: SystemAccount<'info>,

    /// CHECK: External owner and user PDA checked in handler.
    #[account(mut)]
    pub perpetual_user: UncheckedAccount<'info>,

    /// CHECK: External owner, PDA, and order header checked in handler.
    #[account(mut)]
    pub perpetual_order: UncheckedAccount<'info>,

    // GMTrade closes this account during CPI. Do not reload afterward.
    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = perpetual_order
    )]
    pub order_usdc_escrow: Box<Account<'info, TokenAccount>>,

    /// CHECK: External event-authority PDA checked in handler.
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

#[inline(never)]
fn validate_external_accounts(ctx: &Context<CancelShortOrderV1>) -> Result<()> {
    let program = ctx.accounts.perpetual_program.key();
    let store = ctx.accounts.perpetual_store.key();
    let authority = ctx.accounts.strategy_authority.key();
    let nonce = ctx.accounts.pending_short_order.nonce;

    require!(
        program.to_string() == GMTRADE_PROGRAM_ADDRESS
            && ctx.accounts.perpetual_program.to_account_info().executable,
        VaultError::InvalidPerpetualProgram
    );

    require!(
        store.to_string() == GMTRADE_STORE_ADDRESS,
        VaultError::InvalidPerpetualStore
    );

    require_keys_eq!(
        *ctx.accounts.perpetual_store.to_account_info().owner,
        program,
        VaultError::InvalidPerpetualStore
    );

    require_keys_eq!(
        *ctx.accounts.perpetual_user.to_account_info().owner,
        program,
        VaultError::InvalidPerpetualUser
    );

    require_keys_eq!(
        *ctx.accounts.perpetual_order.to_account_info().owner,
        program,
        VaultError::InvalidPerpetualOrderAccounts
    );

    let (expected_wallet, _) =
        Pubkey::find_program_address(&[b"store_wallet", store.as_ref()], &program);

    require_keys_eq!(
        ctx.accounts.perpetual_store_wallet.key(),
        expected_wallet,
        VaultError::InvalidPerpetualOrderAccounts
    );

    let (expected_user, _) = Pubkey::find_program_address(
        &[GMTRADE_USER_SEED, store.as_ref(), authority.as_ref()],
        &program,
    );

    require_keys_eq!(
        ctx.accounts.perpetual_user.key(),
        expected_user,
        VaultError::InvalidPerpetualUser
    );

    let (expected_order, _) = Pubkey::find_program_address(
        &[
            GMTRADE_ORDER_SEED,
            store.as_ref(),
            authority.as_ref(),
            &nonce,
        ],
        &program,
    );

    require_keys_eq!(
        ctx.accounts.perpetual_order.key(),
        expected_order,
        VaultError::InvalidPerpetualOrderAccounts
    );

    let (expected_event_authority, _) =
        Pubkey::find_program_address(&[GMTRADE_EVENT_AUTHORITY_SEED], &program);

    require_keys_eq!(
        ctx.accounts.perpetual_event_authority.key(),
        expected_event_authority,
        VaultError::InvalidPerpetualOrderAccounts
    );

    Ok(())
}

pub fn handle_cancel_short_order(ctx: Context<CancelShortOrderV1>) -> Result<()> {
    validate_external_accounts(&ctx)?;

    let authority = ctx.accounts.strategy_authority.key();
    {
        let data = ctx.accounts.perpetual_order.try_borrow_data()?;

        // Version-zero Order starts with ActionHeader.
        // Offsets include the eight-byte account discriminator.
        require!(data.len() >= 272, VaultError::InvalidExternalAccountData);

        require!(
            data.get(..8) == Some(GMTRADE_ORDER_DISCRIMINATOR.as_slice()),
            VaultError::InvalidPerpetualOrderAccounts
        );

        require!(data[8] == 0, VaultError::InvalidExternalAccountData);

        // ActionState::Pending = 0.
        // Completed orders need a separate reconciliation path.
        require!(data[9] == 0, VaultError::OrderNotPending);

        require_keys_eq!(
            read_pubkey(&data, 24)?,
            ctx.accounts.perpetual_store.key(),
            VaultError::InvalidPendingOrder
        );

        require_keys_eq!(
            read_pubkey(&data, 56)?,
            ctx.accounts.strategy_v1_state.perpetual_market,
            VaultError::InvalidPendingOrder
        );

        require_keys_eq!(
            read_pubkey(&data, 88)?,
            authority,
            VaultError::InvalidPendingOrder
        );

        require!(
            data[120..152] == ctx.accounts.pending_short_order.nonce[..],
            VaultError::InvalidPendingOrder
        );

        // A zero stored receiver falls back to the order owner/creator.
        let rent_receiver = read_pubkey(&data, 208)?;
        let output_receiver = read_pubkey(&data, 240)?;

        require!(
            (rent_receiver == Pubkey::default() || rent_receiver == authority)
                && (output_receiver == Pubkey::default() || output_receiver == authority),
            VaultError::InvalidPendingOrder
        );
    }

    let collateral = ctx.accounts.pending_short_order.collateral_amount;

    require!(
        collateral > 0 && ctx.accounts.order_usdc_escrow.amount >= collateral,
        VaultError::InvalidCancellationRefund
    );

    let source_before = ctx.accounts.strategy_usdc_source.amount;
    let custody_before = ctx.accounts.vault_v1_usdc_account.amount;

    let expected_source = source_before
        .checked_add(collateral)
        .ok_or(VaultError::Overflow)?;

    let expected_custody = custody_before
        .checked_add(collateral)
        .ok_or(VaultError::Overflow)?;

    let instruction = close_short_order_instruction(
        &ShortOrderAccounts {
            perpetual_program: ctx.accounts.perpetual_program.key(),
            strategy_authority: authority,
            store: ctx.accounts.perpetual_store.key(),
            market: ctx.accounts.strategy_v1_state.perpetual_market,
            user: ctx.accounts.perpetual_user.key(),
            order: ctx.accounts.perpetual_order.key(),
            position: ctx.accounts.pending_short_order.position,
            usdc_mint: ctx.accounts.usdc_mint.key(),
            order_usdc_escrow: ctx.accounts.order_usdc_escrow.key(),
            strategy_usdc_source: ctx.accounts.strategy_usdc_source.key(),
        },
        &ctx.accounts.pending_short_order.nonce,
    )?;

    let vault_key = ctx.accounts.vault_v1_state.key();
    let authority_bump = [ctx.bumps.strategy_authority];

    let authority_seeds: &[&[u8]] = &[STRATEGY_AUTHORITY_SEED, vault_key.as_ref(), &authority_bump];

    invoke_signed(
        &instruction,
        &[
            ctx.accounts.strategy_authority.to_account_info(),
            ctx.accounts.perpetual_store.to_account_info(),
            ctx.accounts.perpetual_store_wallet.to_account_info(),
            ctx.accounts.perpetual_user.to_account_info(),
            ctx.accounts.perpetual_order.to_account_info(),
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

    // CPI success alone does not prove the external close completed.
    require!(
        ctx.accounts.perpetual_order.to_account_info().lamports() == 0
            && ctx.accounts.order_usdc_escrow.to_account_info().lamports() == 0,
        VaultError::OrderClosureIncomplete
    );

    ctx.accounts.strategy_usdc_source.reload()?;

    // Permit unsolicited extra tokens, but require recovery of at least
    // the collateral recorded by this vault.
    require!(
        ctx.accounts.strategy_usdc_source.amount >= expected_source,
        VaultError::InvalidCancellationRefund
    );

    token::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.strategy_usdc_source.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.vault_v1_usdc_account.to_account_info(),
                authority: ctx.accounts.strategy_authority.to_account_info(),
            },
            &[authority_seeds],
        ),
        collateral,
        RECEIPT_DECIMALS,
    )?;

    ctx.accounts.vault_v1_usdc_account.reload()?;
    ctx.accounts.strategy_usdc_source.reload()?;

    require!(
        ctx.accounts.vault_v1_usdc_account.amount == expected_custody
            && ctx.accounts.strategy_usdc_source.amount >= source_before,
        VaultError::InvalidCancellationRefund
    );

    // Anchor closes our pending record on successful instruction exit.
    // Any failure above rolls back cancellation and all token transfers.
    Ok(())
}
