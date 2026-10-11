use anchor_lang::prelude::*;

use crate::constants::NUM_ASSETS;

#[event]
pub struct BasketInitialized {
    pub basket_config: Pubkey,
    pub vault: Pubkey,
    pub authority: Pubkey,
    pub token_mints: [Pubkey; NUM_ASSETS],
}

#[event]
pub struct BasketPauseChanged {
    pub basket_config: Pubkey,
    pub paused: bool,
}

#[event]
pub struct BasketDeposited {
    pub basket_config: Pubkey,
    pub user: Pubkey,
    pub amounts: [u64; NUM_ASSETS],
    pub shares: u64,
    pub position_shares: u64,
    pub total_shares: u64,
}

#[event]
pub struct AuctionOpened {
    pub basket_config: Pubkey,
    pub auction: Pubkey,
    pub nonce: u64,
    pub sell_index: u8,
    pub buy_index: u8,
    pub sell_total: u64,
    pub lot_usd_d18: u128,
    pub start_price_d18: u128,
    pub end_price_d18: u128,
    pub start_ts: i64,
    pub end_ts: i64,
}

#[event]
pub struct AuctionBid {
    pub basket_config: Pubkey,
    pub auction: Pubkey,
    pub bidder: Pubkey,
    pub sell_amount: u64,
    pub buy_amount: u64,
    pub price_d18: u128,
    pub sell_remaining: u64,
}

#[event]
pub struct AuctionClosed {
    pub basket_config: Pubkey,
    pub auction: Pubkey,
    pub nonce: u64,
    pub sell_sold: u64,
    pub buy_received: u64,
}
