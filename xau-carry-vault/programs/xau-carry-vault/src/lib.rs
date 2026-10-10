pub mod adapters;
pub mod constants;
pub mod error;
pub mod instructions;
pub mod nav_math;
pub mod oracle_math;
pub mod pyth_reader;
pub mod share_math;
pub mod state;
pub mod strategy_accounting;
pub mod strategy_nav;

use anchor_lang::prelude::*;

pub use instructions::*;
pub use state::*;

declare_id!("G1FuDAbgPsVw95XnEjT68nqpswUtvxFBtxz4mdTJKsye");

#[program]
pub mod xau_carry_vault {
    use super::*;

    pub fn initialize_vault_v1(ctx: Context<InitializeVaultV1>) -> Result<()> {
        instructions::initialize::handle_initialize(ctx)
    }

    pub fn deposit_usdc_v1(
        ctx: Context<DepositUsdcV1>,
        amount: u64,
        min_shares_out: u64,
    ) -> Result<()> {
        instructions::deposit::handle_deposit(ctx, amount, min_shares_out)
    }

    pub fn withdraw_usdc_v1(
        ctx: Context<WithdrawUsdcV1>,
        amount: u64,
        min_assets_out: u64,
    ) -> Result<()> {
        instructions::withdraw::handle_withdraw(ctx, amount, min_assets_out)
    }

    pub fn configure_strategy_v1(ctx: Context<ConfigureStrategyV1>) -> Result<()> {
        instructions::configure_strategy::handle_configure_strategy(ctx)
    }

    pub fn prepare_perpetual_user_v1(
        ctx: Context<PreparePerpetualUserV1>,
        funding_lamports: u64,
    ) -> Result<()> {
        instructions::prepare_perpetual_user::handle_prepare_perpetual_user(ctx, funding_lamports)
    }

    pub fn prepare_short_position_v1(
        ctx: Context<PrepareShortPositionV1>,
        funding_lamports: u64,
    ) -> Result<()> {
        instructions::prepare_short_position::handle_prepare_short_position(ctx, funding_lamports)
    }

    pub fn create_short_order_v1(
        ctx: Context<CreateShortOrderV1>,
        nonce: [u8; 32],
        params: CreateShortOrderParamsV1,
    ) -> Result<()> {
        instructions::create_short_order::handle_create_short_order(ctx, nonce, params)
    }

    pub fn buy_gold_v1<'info>(
        ctx: Context<'info, GoldTradeV1<'info>>,
        params: GoldTradeParamsV1,
    ) -> Result<()> {
        instructions::gold_trade::handle_gold_trade(ctx, params, true)
    }
    pub fn sell_gold_v1<'info>(
        ctx: Context<'info, GoldTradeV1<'info>>,
        params: GoldTradeParamsV1,
    ) -> Result<()> {
        instructions::gold_trade::handle_gold_trade(ctx, params, false)
    }
    pub fn create_short_decrease_v1(
        ctx: Context<CreateShortDecreaseV1>,
        nonce: [u8; 32],
        params: ShortDecreaseParamsV1,
    ) -> Result<()> {
        instructions::short_decrease::handle_create_decrease(ctx, nonce, params)
    }
    pub fn reconcile_short_decrease_v1(ctx: Context<RecoverShortDecreaseV1>) -> Result<()> {
        instructions::recover_short_decrease::handle_recover_decrease(ctx, false)
    }
    pub fn cancel_short_decrease_v1(ctx: Context<RecoverShortDecreaseV1>) -> Result<()> {
        instructions::recover_short_decrease::handle_recover_decrease(ctx, true)
    }
    pub fn accept_full_close_decrease_v1(ctx: Context<RecoverShortDecreaseV1>) -> Result<()> {
        instructions::recover_short_decrease::handle_accept_full_close(ctx)
    }
    pub fn sweep_strategy_usdc_v1(ctx: Context<SweepStrategyUsdcV1>) -> Result<()> {
        instructions::gold_trade::handle_sweep(ctx)
    }

    pub fn cancel_short_order_v1(ctx: Context<CancelShortOrderV1>) -> Result<()> {
        instructions::cancel_short_order::handle_cancel_short_order(ctx)
    }

    pub fn set_strategy_execution_v1(
        ctx: Context<SetStrategyExecutionV1>,
        enabled: bool,
    ) -> Result<()> {
        instructions::reconcile_short_order::handle_set_strategy_execution(ctx, enabled)
    }
    pub fn reconcile_short_order_v1(ctx: Context<ReconcileShortOrderV1>) -> Result<()> {
        instructions::reconcile_short_order::handle_reconcile(ctx)
    }
    pub fn read_full_nav_v1(ctx: Context<ReadIdleNavV1>) -> Result<FullNavQuoteV1> {
        instructions::read_idle_nav::handle_read_full_nav(ctx)
    }

    pub fn read_idle_nav_v1(ctx: Context<ReadIdleNavV1>) -> Result<IdleNavQuoteV1> {
        instructions::read_idle_nav::handle_read_idle_nav(ctx)
    }

    pub fn set_vault_controls_v1(
        ctx: Context<ManageVaultV1>,
        controls: VaultControlsV1,
    ) -> Result<()> {
        instructions::vault_admin::handle_set_controls(ctx, controls)
    }

    pub fn propose_vault_admin_v1(ctx: Context<ManageVaultV1>, new_admin: Pubkey) -> Result<()> {
        instructions::vault_admin::handle_propose_admin(ctx, new_admin)
    }

    pub fn accept_vault_admin_v1(ctx: Context<AcceptVaultAdminV1>) -> Result<()> {
        instructions::vault_admin::handle_accept_admin(ctx)
    }

    pub fn recover_unowned_custody_v1(ctx: Context<RecoverUnownedCustodyV1>) -> Result<()> {
        instructions::vault_admin::handle_recover_unowned(ctx)
    }

    pub fn request_withdrawal_v1(
        ctx: Context<RequestWithdrawalV1>,
        nonce: u64,
        shares: u64,
        min_assets_out: u64,
    ) -> Result<()> {
        instructions::withdrawal_queue::handle_request(ctx, nonce, shares, min_assets_out)
    }

    pub fn settle_withdrawal_v1(ctx: Context<CompleteWithdrawalV1>) -> Result<()> {
        instructions::withdrawal_queue::handle_complete(ctx, false)
    }

    pub fn cancel_withdrawal_v1(ctx: Context<CompleteWithdrawalV1>) -> Result<()> {
        instructions::withdrawal_queue::handle_complete(ctx, true)
    }

    pub fn read_gold_component_v1(
        ctx: Context<ReadGoldComponentV1>,
    ) -> Result<GoldComponentQuoteV1> {
        instructions::read_gold_component::handle_read_gold_component(ctx)
    }
}
