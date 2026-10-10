use anchor_lang::{prelude::*, solana_program::program::invoke_signed};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{
    adapters::gmtrade::{close_short_order_instruction, ShortOrderAccounts},
    constants::*,
    error::VaultError,
    state::{OrderBaselineV1, PendingShortOrderV1, StrategyV1State, VaultV1State},
};

#[derive(Accounts)]
pub struct ReconcileShortOrderV1<'info> {
    #[account(mut)]
    pub keeper: Signer<'info>,

    #[account(
        seeds = [
            VAULT_SEED,
            usdc_mint.key().as_ref()
        ],
        bump = vault_v1_state.bump,
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
        close = refund,
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

    // Baseline is mandatory for execution proof; no post-execution reconstruction.
    #[account(mut, close = refund, seeds = [ORDER_BASELINE_SEED, perpetual_order.key().as_ref()],
        bump = order_baseline.bump,
        constraint = order_baseline.order == pending_short_order.order @ VaultError::InvalidOrderBaseline,
        constraint = order_baseline.vault == vault_v1_state.key() @ VaultError::InvalidOrderBaseline,
        has_one = refund @ VaultError::InvalidOrderBaseline)]
    pub order_baseline: Box<Account<'info, OrderBaselineV1>>,

    /// CHECK: Rent refunds go to the original baseline payer, never the keeper.
    #[account(mut)]
    pub refund: UncheckedAccount<'info>,

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

    /// CHECK: Fixed market owner/layout validated in handler.
    pub perpetual_market: UncheckedAccount<'info>,
    /// CHECK: Canonical position identity and complete state validated in handler.
    pub perpetual_position: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[inline(never)]
fn validate_external_accounts(ctx: &Context<ReconcileShortOrderV1>) -> Result<()> {
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

pub fn handle_reconcile(ctx: Context<ReconcileShortOrderV1>) -> Result<()> {
    validate_external_accounts(&ctx)?;
    let authority = ctx.accounts.strategy_authority.key();
    let slot = Clock::get()?.slot;
    let b = &ctx.accounts.order_baseline;
    let pending = &ctx.accounts.pending_short_order;
    require!(
        b.nonce == pending.nonce
            && b.submitted_slot == pending.submitted_at_slot
            && b.expected_size_delta == pending.size_delta_value
            && b.position == pending.position
            && b.position == ctx.accounts.perpetual_position.key(),
        VaultError::InvalidOrderBaseline
    );
    let order = crate::adapters::gmtrade_accounts::executed_order(
        &ctx.accounts.perpetual_order.to_account_info(),
        b,
        authority,
        slot,
    )?;
    require!(
        order.params.initial_collateral_delta_amount == pending.collateral_amount
            && order.params.collateral_token.to_bytes() == ctx.accounts.usdc_mint.key().to_bytes(),
        VaultError::UnprovenOrderExecution
    );
    let (_, position) = crate::adapters::gmtrade_accounts::market_and_position(
        &ctx.accounts.perpetual_market.to_account_info(),
        &ctx.accounts.perpetual_position.to_account_info(),
        authority,
        ctx.accounts.usdc_mint.key(),
    )?;
    require!(
        position.state.trade_id > b.trade_id_before
            && position.state.updated_at_slot == order.header.updated_at_slot,
        VaultError::UnprovenOrderExecution
    );
    require!(
        position.state.size_in_usd
            == b.size_before
                .checked_add(b.expected_size_delta)
                .ok_or(VaultError::Overflow)?,
        VaultError::UnprovenOrderExecution
    );
    let mut tracker = crate::strategy_accounting::OrderTracker {
        pending: Some(crate::strategy_accounting::PendingOrder {
            order: b.order.to_bytes(),
            position: b.position.to_bytes(),
            nonce: b.nonce,
            submitted_slot: b.submitted_slot,
            size_before: b.size_before,
            expected_size_delta: b.expected_size_delta,
        }),
    };
    // Successful increases may leave funding credits in order escrow.
    // Record them before closing; they must arrive in the canonical strategy ATA.
    let escrow_before = ctx.accounts.order_usdc_escrow.amount;
    let source_before = ctx.accounts.strategy_usdc_source.amount;
    let expected_source = source_before
        .checked_add(escrow_before)
        .ok_or(VaultError::Overflow)?;
    let execution_slot = order.header.updated_at_slot;
    let size_after = position.state.size_in_usd;
    drop(order);
    drop(position);
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
    require!(
        ctx.accounts.strategy_usdc_source.amount == expected_source,
        VaultError::InvalidOrderCollateralAccounting
    );
    tracker
        .reconcile(
            crate::strategy_accounting::OrderEvidence {
                order: ctx.accounts.perpetual_order.key().to_bytes(),
                position: ctx.accounts.perpetual_position.key().to_bytes(),
                nonce: ctx.accounts.pending_short_order.nonce,
                status: crate::strategy_accounting::OrderStatus::Executed,
                execution_slot,
                observed_slot: slot,
                size_after,
                remaining_escrow: 0,
            },
            slot,
        )
        .map_err(crate::strategy_nav::accounting_error)?;
    // Anchor closes pending and baseline records only after every check/CPI succeeds.
    Ok(())
}

/// Enabling strategy submissions requires a current authenticated full NAV.
/// Disabling always works without oracle accounts, including during an outage.
#[derive(Accounts)]
pub struct SetStrategyExecutionV1<'info> {
    pub admin: Signer<'info>,
    #[account(seeds=[VAULT_SEED,usdc_mint.key().as_ref()],bump=vault_v1_state.bump,
  has_one=admin @ VaultError::UnauthorizedStrategyAdmin,has_one=usdc_mint)]
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,
    #[account(mut,seeds=[STRATEGY_SEED,vault_v1_state.key().as_ref()],bump=strategy_v1_state.bump,
  constraint=strategy_v1_state.vault==vault_v1_state.key() @ VaultError::InvalidStrategyVault)]
    pub strategy_v1_state: Box<Account<'info, StrategyV1State>>,
    pub usdc_mint: Box<Account<'info, Mint>>,
    #[account(seeds=[TOKEN_VAULT_SEED,vault_v1_state.key().as_ref()],bump,
  token::mint=usdc_mint,token::authority=vault_v1_state)]
    pub vault_v1_usdc_account: Box<Account<'info, TokenAccount>>,
}
pub fn handle_set_strategy_execution(
    ctx: Context<SetStrategyExecutionV1>,
    enabled: bool,
) -> Result<()> {
    if enabled {
        require!(
            !ctx.accounts.vault_v1_state.strategy_paused,
            VaultError::StrategyPaused
        );
        crate::strategy_nav::quote(
            ctx.accounts.vault_v1_state.key(),
            &ctx.accounts.vault_v1_state,
            &ctx.accounts.strategy_v1_state.to_account_info(),
            ctx.accounts.vault_v1_usdc_account.amount,
            ctx.remaining_accounts,
        )?;
    }
    ctx.accounts.strategy_v1_state.execution_enabled = enabled;
    Ok(())
}
