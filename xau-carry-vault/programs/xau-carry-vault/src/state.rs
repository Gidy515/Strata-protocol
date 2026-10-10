use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct VaultV1State {
    pub admin: Pubkey,
    pub usdc_mint: Pubkey,
    pub vxau_mint: Pubkey,
    pub total_deposited: u64,
    pub bump: u8,

    pub pending_admin: Pubkey,
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
    pub strategy_paused: bool,

    // Per-order ceilings, not leverage or liquidation protection.
    pub max_order_collateral: u64,

    // Native GMTrade size_delta_value units.
    pub max_order_size: u128,
}

#[account]
#[derive(InitSpace)]
pub struct StrategyV1State {
    pub vault: Pubkey,
    pub gold_mint: Pubkey,
    pub gold_custody: Pubkey,
    pub gold_token_program: Pubkey,
    pub perpetual_program: Pubkey,
    pub perpetual_store: Pubkey,
    pub perpetual_market: Pubkey,
    pub configured_at_slot: u64,
    pub execution_enabled: bool,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct PendingShortOrderV1 {
    pub vault: Pubkey,
    pub order: Pubkey,
    pub position: Pubkey,
    pub strategy_authority: Pubkey,
    pub nonce: [u8; 32],
    pub collateral_amount: u64,
    pub size_delta_value: u128,
    pub acceptable_price: u128,
    pub execution_lamports: u64,
    pub submitted_at_slot: u64,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct WithdrawalRequestV1 {
    pub vault: Pubkey,
    pub owner: Pubkey,
    pub nonce: u64,
    pub shares: u64,
    pub min_assets_out: u64,
    pub requested_at_slot: u64,
    pub bump: u8,
}

/// Separate extension PDA: existing Vault/Strategy/Pending layouts remain stable.
#[account]
#[derive(InitSpace)]
pub struct OrderBaselineV1 {
    pub vault: Pubkey,
    pub order: Pubkey,
    pub position: Pubkey,
    pub nonce: [u8; 32],
    pub refund: Pubkey,
    pub submitted_slot: u64,
    pub size_before: u128,
    pub collateral_before: u128,
    pub trade_id_before: u64,
    pub expected_size_delta: u128,
    pub bump: u8,
}

/// Decrease orders have a separate discriminator, preserving existing increase baselines.
#[account]
#[derive(InitSpace)]
pub struct DecreaseBaselineV1 {
    pub vault: Pubkey,
    pub order: Pubkey,
    pub position: Pubkey,
    pub nonce: [u8; 32],
    pub refund: Pubkey,
    pub submitted_slot: u64,
    pub size_before: u128,
    pub collateral_before: u128,
    pub trade_id_before: u64,
    pub expected_size_delta: u128,
    pub min_usdc_out: u64,
    pub allow_full_close: bool,
    pub bump: u8,
}
