use anchor_lang::prelude::*;

use crate::constants::NUM_ASSETS;

/// One basket's tokens, settings, shares and auction state. Seeds: ["basket_config", vault].
#[account]
#[derive(InitSpace)]
pub struct BasketConfig {
    /// Admin who created the basket and can pause it.
    pub authority: Pubkey,
    /// Vault this basket belongs to; part of the PDA seeds.
    pub vault: Pubkey,
    /// Token mints in order: vXAU, PAXG, USDY, SPYx (PAXG, USDY, SPYx are mock mints on devnet).
    pub token_mints: [Pubkey; NUM_ASSETS],
    /// Target weight of each token in basis points, summing to 10,000.
    pub weights: [u16; NUM_ASSETS],
    pub bump: u8,

    /// Rebalancing details for each token, same order as token_mints.
    pub assets: [AssetConfig; NUM_ASSETS],
    /// Raw units of each token per share, used only for the first deposit.
    pub initial_units: [u64; NUM_ASSETS],
    /// All shares in existence, including the shares locked on first deposit.
    pub total_shares: u64,
    pub rebalance: RebalanceConfig,
    /// Number of the next auction; part of its PDA seeds.
    pub auction_nonce: u64,
    /// True while an auction is running (one at a time).
    pub active_auction: bool,
    /// When the last auction closed, for the cooldown.
    pub last_auction_end_ts: i64,
    /// Emergency stop for deposits, new auctions and bids.
    pub paused: bool,
}

/// Per-token settings besides its mint and weight.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub struct AssetConfig {
    /// Basket's own token account for this token.
    pub vault_token: Pubkey,
    /// SPL Token or Token-2022 program for this token.
    pub token_program: Pubkey,
    pub decimals: u8,
    pub price_source: PriceSource,
    /// True for tokens with a dividend/split multiplier (SPYx).
    pub scaled_ui: bool,
    /// Allowed drift above the target weight before an auction can sell it.
    pub band_bps: u16,
    /// Largest auction size for this token, USD with 6 decimals.
    pub max_lot_usd: u64,
}

/// Where a token's USD price comes from.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum PriceSource {
    /// Constant price, e.g. vXAU at $1 (18 decimals).
    Fixed { price_usd_d18: u128 },
    /// Live Pyth price for this feed id.
    Pyth { feed_id: [u8; 32] },
}

/// Auction settings shared by all tokens in a basket.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub struct RebalanceConfig {
    /// Max price age for the two tokens being traded.
    pub max_age_traded_s: u32,
    /// Max price age for tokens only used to value the basket.
    pub max_age_nav_s: u32,
    /// Max Pyth confidence relative to price.
    pub max_conf_bps: u16,
    /// Auction start price above the oracle rate.
    pub start_premium_bps: u16,
    /// Auction end price below the oracle rate; the worst fill.
    pub max_discount_bps: u16,
    pub auction_duration_s: u32,
    /// Wait after an auction closes before the next can open.
    pub cooldown_s: u32,
    /// Smallest auction worth running, USD with 6 decimals.
    pub min_lot_usd: u64,
}

/// A running auction. Seeds: ["auction", basket_config, nonce].
#[account]
#[derive(InitSpace)]
pub struct Auction {
    pub basket_config: Pubkey,
    pub nonce: u64,
    /// Paid the rent; refunded on close.
    pub opener: Pubkey,
    /// Index of the overweight token being sold.
    pub sell_index: u8,
    /// Index of the underweight token being bought.
    pub buy_index: u8,
    /// Raw units of the sell token on offer.
    pub sell_total: u64,
    pub sell_remaining: u64,
    pub buy_received: u64,
    /// Buy-token units per sell-token unit at the start (18 decimals).
    pub start_price_d18: u128,
    /// Lowest price, reached at end_ts.
    pub end_price_d18: u128,
    pub start_ts: i64,
    pub end_ts: i64,
    /// USD prices snapshotted when the auction opened.
    pub sell_price_usd_d18: u128,
    pub buy_price_usd_d18: u128,
    pub bump: u8,
}

/// A user's shares in a basket. Seeds: ["position", basket_config, user].
#[account]
#[derive(InitSpace)]
pub struct UserPosition {
    pub basket_config: Pubkey,
    pub owner: Pubkey,
    pub shares: u64,
    pub created_ts: i64,
    pub last_deposit_ts: i64,
    pub bump: u8,
}
