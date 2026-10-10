anchor_lang::declare_id!("Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo");
// Narrow zero-copy account layouts generated from the pinned upstream IDL.
// No universal Account enum / large stack deserializer is compiled.
pub mod types {
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ActionFlagContainer {
 pub value: u8,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ActionHeader {
 pub version: u8,
 pub action_state: u8,
 pub bump: u8,
 pub flags: ActionFlagContainer,
 pub callback_kind: u8,
 pub callback_version: u8,
 pub padding_0: [u8; 2],
 pub id: u64,
 pub store: anchor_lang::prelude::Pubkey,
 pub market: anchor_lang::prelude::Pubkey,
 pub owner: anchor_lang::prelude::Pubkey,
 pub nonce: [u8; 32],
 pub max_execution_lamports: u64,
 pub updated_at: i64,
 pub updated_at_slot: u64,
 pub creator: anchor_lang::prelude::Pubkey,
 pub rent_receiver: anchor_lang::prelude::Pubkey,
 pub receiver: anchor_lang::prelude::Pubkey,
 pub callback_program_id: anchor_lang::prelude::Pubkey,
 pub callback_shared_data: anchor_lang::prelude::Pubkey,
 pub callback_partitioned_data: anchor_lang::prelude::Pubkey,
 pub reserved: [u8; 160],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Clocks {
 pub padding: [u8; 8],
 pub rev: u64,
 pub price_impact_distribution: i64,
 pub borrowing: i64,
 pub funding: i64,
 pub adl_for_long: i64,
 pub adl_for_short: i64,
 pub reserved: [i64; 3],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Indexer {
 pub trade_count: u64,
 pub deposit_count: u64,
 pub withdrawal_count: u64,
 pub order_count: u64,
 pub shift_count: u64,
 pub glv_deposit_count: u64,
 pub glv_withdrawal_count: u64,
 pub padding_0: [u8; 8],
 pub reserved: [u8; 128],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Market {
 pub version: u8,
 pub bump: u8,
 pub flags: MarketFlagContainer,
 pub padding: [u8; 5],
 pub closed_state_updated_at: i64,
 pub name: [u8; 64],
 pub meta: MarketMeta,
 pub store: anchor_lang::prelude::Pubkey,
 pub config: MarketConfig,
 pub indexer: Indexer,
 pub state: State,
 pub buffer: RevertibleBuffer,
 pub virtual_inventory_for_swaps: anchor_lang::prelude::Pubkey,
 pub virtual_inventory_for_positions: anchor_lang::prelude::Pubkey,
 pub reserved: [u8; 192],
}
impl anchor_lang::Discriminator for Market { const DISCRIMINATOR: &'static [u8] = &[219, 190, 213, 55, 0, 227, 198, 154]; }
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MarketConfig {
 pub flag: MarketConfigFlagContainer,
 pub swap_impact_exponent: u128,
 pub swap_impact_positive_factor: u128,
 pub swap_impact_negative_factor: u128,
 pub swap_fee_receiver_factor: u128,
 pub swap_fee_factor_for_positive_impact: u128,
 pub swap_fee_factor_for_negative_impact: u128,
 pub min_position_size_usd: u128,
 pub min_collateral_value: u128,
 pub min_collateral_factor: u128,
 pub min_collateral_factor_for_open_interest_multiplier_for_long: u128,
 pub min_collateral_factor_for_open_interest_multiplier_for_short: u128,
 pub max_positive_position_impact_factor: u128,
 pub max_negative_position_impact_factor: u128,
 pub max_position_impact_factor_for_liquidations: u128,
 pub position_impact_exponent: u128,
 pub position_impact_positive_factor: u128,
 pub position_impact_negative_factor: u128,
 pub order_fee_receiver_factor: u128,
 pub order_fee_factor_for_positive_impact: u128,
 pub order_fee_factor_for_negative_impact: u128,
 pub liquidation_fee_receiver_factor: u128,
 pub liquidation_fee_factor: u128,
 pub position_impact_distribute_factor: u128,
 pub min_position_impact_pool_amount: u128,
 pub borrowing_fee_receiver_factor: u128,
 pub borrowing_fee_factor_for_long: u128,
 pub borrowing_fee_factor_for_short: u128,
 pub borrowing_fee_exponent_for_long: u128,
 pub borrowing_fee_exponent_for_short: u128,
 pub borrowing_fee_optimal_usage_factor_for_long: u128,
 pub borrowing_fee_optimal_usage_factor_for_short: u128,
 pub borrowing_fee_base_factor_for_long: u128,
 pub borrowing_fee_base_factor_for_short: u128,
 pub borrowing_fee_above_optimal_usage_factor_for_long: u128,
 pub borrowing_fee_above_optimal_usage_factor_for_short: u128,
 pub funding_fee_exponent: u128,
 pub funding_fee_factor: u128,
 pub funding_fee_max_factor_per_second: u128,
 pub funding_fee_min_factor_per_second: u128,
 pub funding_fee_increase_factor_per_second: u128,
 pub funding_fee_decrease_factor_per_second: u128,
 pub funding_fee_threshold_for_stable_funding: u128,
 pub funding_fee_threshold_for_decrease_funding: u128,
 pub reserve_factor: u128,
 pub open_interest_reserve_factor: u128,
 pub max_pnl_factor_for_long_deposit: u128,
 pub max_pnl_factor_for_short_deposit: u128,
 pub max_pnl_factor_for_long_withdrawal: u128,
 pub max_pnl_factor_for_short_withdrawal: u128,
 pub max_pnl_factor_for_long_trader: u128,
 pub max_pnl_factor_for_short_trader: u128,
 pub max_pnl_factor_for_long_adl: u128,
 pub max_pnl_factor_for_short_adl: u128,
 pub min_pnl_factor_after_long_adl: u128,
 pub min_pnl_factor_after_short_adl: u128,
 pub max_pool_amount_for_long_token: u128,
 pub max_pool_amount_for_short_token: u128,
 pub max_pool_value_for_deposit_for_long_token: u128,
 pub max_pool_value_for_deposit_for_short_token: u128,
 pub max_open_interest_for_long: u128,
 pub max_open_interest_for_short: u128,
 pub min_tokens_for_first_deposit: u128,
 pub min_collateral_factor_for_liquidation: u128,
 pub market_closed_min_collateral_factor_for_liquidation: u128,
 pub market_closed_borrowing_fee_base_factor: u128,
 pub market_closed_borrowing_fee_above_optimal_usage_factor: u128,
 pub reserved: [u128; 28],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MarketConfigFlagContainer {
 pub value: u128,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MarketFlagContainer {
 pub value: u8,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MarketMeta {
 pub market_token_mint: anchor_lang::prelude::Pubkey,
 pub index_token_mint: anchor_lang::prelude::Pubkey,
 pub long_token_mint: anchor_lang::prelude::Pubkey,
 pub short_token_mint: anchor_lang::prelude::Pubkey,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Order {
 pub header: ActionHeader,
 pub market_token: anchor_lang::prelude::Pubkey,
 pub tokens: OrderTokenAccounts,
 pub swap: SwapActionParams,
 pub padding_0: [u8; 4],
 pub params: OrderActionParams,
 pub gt_reward: u64,
 pub builder_fee_amount: u64,
 pub builder: anchor_lang::prelude::Pubkey,
 pub builder_fee_factor: u128,
 pub reserved: [u8; 80],
}
impl anchor_lang::Discriminator for Order { const DISCRIMINATOR: &'static [u8] = &[134, 173, 223, 185, 77, 86, 28, 51]; }
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OrderActionParams {
 pub kind: u8,
 pub side: u8,
 pub decrease_position_swap_type: u8,
 pub flags: OrderFlagContainer,
 pub padding_1: [u8; 4],
 pub collateral_token: anchor_lang::prelude::Pubkey,
 pub position: anchor_lang::prelude::Pubkey,
 pub initial_collateral_delta_amount: u64,
 pub size_delta_value: u128,
 pub min_output: u128,
 pub trigger_price: u128,
 pub acceptable_price: u128,
 pub valid_from_ts: i64,
 pub padding_2: [u8; 8],
 pub reserved: [u8; 64],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OrderFlagContainer {
 pub value: u8,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OrderTokenAccounts {
 pub initial_collateral: TokenAndAccount,
 pub final_output_token: TokenAndAccount,
 pub long_token: TokenAndAccount,
 pub short_token: TokenAndAccount,
 pub reserved: [u8; 128],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OtherState {
 pub padding: [u8; 16],
 pub rev: u64,
 pub trade_count: u64,
 pub long_token_balance: u64,
 pub short_token_balance: u64,
 pub funding_factor_per_second: i128,
 pub reserved: [u8; 256],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Pool {
 pub is_pure: u8,
 pub padding: [u8; 15],
 pub long_token_amount: u128,
 pub short_token_amount: u128,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PoolStorage {
 pub rev: u64,
 pub padding: [u8; 8],
 pub pool: Pool,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Pools {
 pub primary: PoolStorage,
 pub swap_impact: PoolStorage,
 pub claimable_fee: PoolStorage,
 pub open_interest_for_long: PoolStorage,
 pub open_interest_for_short: PoolStorage,
 pub open_interest_in_tokens_for_long: PoolStorage,
 pub open_interest_in_tokens_for_short: PoolStorage,
 pub position_impact: PoolStorage,
 pub borrowing_factor: PoolStorage,
 pub funding_amount_per_size_for_long: PoolStorage,
 pub funding_amount_per_size_for_short: PoolStorage,
 pub claimable_funding_amount_per_size_for_long: PoolStorage,
 pub claimable_funding_amount_per_size_for_short: PoolStorage,
 pub collateral_sum_for_long: PoolStorage,
 pub collateral_sum_for_short: PoolStorage,
 pub total_borrowing: PoolStorage,
 pub reserved: [PoolStorage; 16],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Position {
 pub version: u8,
 pub bump: u8,
 pub store: anchor_lang::prelude::Pubkey,
 pub kind: u8,
 pub padding_0: [u8; 5],
 pub created_at: i64,
 pub owner: anchor_lang::prelude::Pubkey,
 pub market_token: anchor_lang::prelude::Pubkey,
 pub collateral_token: anchor_lang::prelude::Pubkey,
 pub state: PositionState,
 pub reserved: [u8; 256],
}
impl anchor_lang::Discriminator for Position { const DISCRIMINATOR: &'static [u8] = &[170, 188, 143, 228, 122, 64, 247, 208]; }
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PositionState {
 pub trade_id: u64,
 pub increased_at: i64,
 pub updated_at_slot: u64,
 pub decreased_at: i64,
 pub size_in_tokens: u128,
 pub collateral_amount: u128,
 pub size_in_usd: u128,
 pub borrowing_factor: u128,
 pub funding_fee_amount_per_size: u128,
 pub long_token_claimable_funding_amount_per_size: u128,
 pub short_token_claimable_funding_amount_per_size: u128,
 pub reserved: [u8; 128],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RevertibleBuffer {
 pub rev: u64,
 pub padding: [u8; 8],
 pub state: State,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RevertiblePoolBuffer {
 pub rev: u64,
 pub padding: [u8; 8],
 pub pool: PoolStorage,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct State {
 pub pools: Pools,
 pub clocks: Clocks,
 pub other: OtherState,
 pub reserved: [u8; 1024],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SwapActionParams {
 pub primary_length: u8,
 pub secondary_length: u8,
 pub num_tokens: u8,
 pub padding_0: [u8; 1],
 pub current_market_token: anchor_lang::prelude::Pubkey,
 pub paths: [anchor_lang::prelude::Pubkey; 10],
 pub tokens: [anchor_lang::prelude::Pubkey; 25],
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TokenAndAccount {
 pub token: anchor_lang::prelude::Pubkey,
 pub account: anchor_lang::prelude::Pubkey,
}
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VirtualInventory {
 pub version: u8,
 pub bump: u8,
 pub flags: VirtualInventoryFlagContainer,
 pub long_amount_decimals: u8,
 pub short_amount_decimals: u8,
 pub padding_0: [u8; 3],
 pub ref_count: u32,
 pub index: u32,
 pub padding_1: [u8; 16],
 pub store: anchor_lang::prelude::Pubkey,
 pub pool: PoolStorage,
 pub buffer: RevertiblePoolBuffer,
 pub reserved_0: [u8; 128],
}
impl anchor_lang::Discriminator for VirtualInventory { const DISCRIMINATOR: &'static [u8] = &[115, 193, 34, 238, 66, 28, 198, 164]; }
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VirtualInventoryFlagContainer {
 pub value: u8,
}
#[derive(Debug,Clone,Copy)] pub struct Price<T> {pub min:T,pub max:T}
#[derive(Debug,Clone,Copy)] pub struct Prices<T> {pub index_token_price:Price<T>,pub long_token_price:Price<T>,pub short_token_price:Price<T>}
}
pub mod accounts { pub use super::types::{Market, Position, Order, VirtualInventory}; }
