use anchor_lang::prelude::*;

use crate::{
    constants::*,
    error::BasketError,
    events::AuctionClosed,
    state::{Auction, BasketConfig},
};

/// Accounts for closing an auction; anyone may call.
#[derive(Accounts)]
pub struct CloseAuction<'info> {
    /// The basket; freed for the next auction.
    #[account(
        mut,
        seeds = [BASKET_CONFIG_SEED, basket_config.vault.as_ref()],
        bump = basket_config.bump,
    )]
    pub basket_config: Box<Account<'info, BasketConfig>>,
    /// The auction; deleted and its rent sent to the opener.
    #[account(
        mut,
        close = opener,
        has_one = opener,
        seeds = [AUCTION_SEED, basket_config.key().as_ref(), &auction.nonce.to_le_bytes()],
        bump = auction.bump,
        constraint = auction.basket_config == basket_config.key() @ BasketError::InvalidAuction,
    )]
    pub auction: Box<Account<'info, Auction>>,
    /// CHECK: rent refund receiver, verified by has_one = opener.
    #[account(mut)]
    pub opener: UncheckedAccount<'info>,
}

/// Closes an auction once it is sold out or past its end time.
pub fn close_auction_handler(ctx: Context<CloseAuction>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let auction = &ctx.accounts.auction;
    require!(
        auction.sell_remaining == 0 || now > auction.end_ts,
        BasketError::AuctionStillLive
    );

    emit!(AuctionClosed {
        basket_config: auction.basket_config,
        auction: auction.key(),
        nonce: auction.nonce,
        sell_sold: auction
            .sell_total
            .checked_sub(auction.sell_remaining)
            .ok_or_else(|| error!(BasketError::MathOverflow))?,
        buy_received: auction.buy_received,
    });

    // Free the basket and start the cooldown.
    let basket = &mut ctx.accounts.basket_config;
    basket.active_auction = false;
    basket.last_auction_end_ts = now;
    Ok(())
}
