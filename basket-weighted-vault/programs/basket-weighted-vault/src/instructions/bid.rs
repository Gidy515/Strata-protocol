use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{
    constants::*,
    error::BasketError,
    events::AuctionBid,
    math,
    state::{Auction, BasketConfig},
};

/// Accounts for a bid; every token account is pinned to the basket's config.
#[derive(Accounts)]
pub struct Bid<'info> {
    /// Buyer; signs the payment.
    pub bidder: Signer<'info>,
    /// The basket; signs the payout as a PDA.
    #[account(
        seeds = [BASKET_CONFIG_SEED, basket_config.vault.as_ref()],
        bump = basket_config.bump,
    )]
    pub basket_config: Box<Account<'info, BasketConfig>>,
    /// The auction being bid on; must belong to this basket.
    #[account(
        mut,
        seeds = [AUCTION_SEED, basket_config.key().as_ref(), &auction.nonce.to_le_bytes()],
        bump = auction.bump,
        constraint = auction.basket_config == basket_config.key() @ BasketError::InvalidAuction,
    )]
    pub auction: Box<Account<'info, Auction>>,
    /// Mint of the token the basket sells.
    #[account(address = basket_config.token_mints[auction.sell_index as usize])]
    pub sell_mint: Box<InterfaceAccount<'info, Mint>>,
    /// Mint of the token the basket buys.
    #[account(address = basket_config.token_mints[auction.buy_index as usize])]
    pub buy_mint: Box<InterfaceAccount<'info, Mint>>,
    /// Basket account the sold tokens leave from.
    #[account(
        mut,
        address = basket_config.assets[auction.sell_index as usize].vault_token @ BasketError::InvalidVaultAccount,
    )]
    pub vault_sell_account: Box<InterfaceAccount<'info, TokenAccount>>,
    /// Basket account the payment goes into.
    #[account(
        mut,
        address = basket_config.assets[auction.buy_index as usize].vault_token @ BasketError::InvalidVaultAccount,
    )]
    pub vault_buy_account: Box<InterfaceAccount<'info, TokenAccount>>,
    /// Receives the sold tokens; must not be the basket's own account.
    #[account(
        mut,
        token::mint = sell_mint,
        token::token_program = sell_token_program,
        constraint = bidder_sell_account.key() != vault_sell_account.key() @ BasketError::SameAccount,
    )]
    pub bidder_sell_account: Box<InterfaceAccount<'info, TokenAccount>>,
    /// Bidder's own account that pays.
    #[account(
        mut,
        token::mint = buy_mint,
        token::authority = bidder,
        token::token_program = buy_token_program,
        constraint = bidder_buy_account.key() != vault_buy_account.key() @ BasketError::SameAccount,
    )]
    pub bidder_buy_account: Box<InterfaceAccount<'info, TokenAccount>>,
    /// Token programs for each side, pinned to the config.
    #[account(address = basket_config.assets[auction.sell_index as usize].token_program)]
    pub sell_token_program: Interface<'info, TokenInterface>,
    #[account(address = basket_config.assets[auction.buy_index as usize].token_program)]
    pub buy_token_program: Interface<'info, TokenInterface>,
}

/// Buys up to sell_amount at the current price, paying at most max_buy_amount.
pub fn bid_handler(ctx: Context<Bid>, sell_amount: u64, max_buy_amount: u64) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let basket = &ctx.accounts.basket_config;
    let auction = &ctx.accounts.auction;

    // Auction must be live and have something left to sell.
    require!(!basket.paused, BasketError::Paused);
    require!(
        now >= auction.start_ts && now <= auction.end_ts,
        BasketError::AuctionNotLive
    );
    require!(auction.sell_remaining > 0, BasketError::AuctionFilled);

    // Current point on the falling price line.
    let price_d18 = math::current_price_d18(
        auction.start_price_d18,
        auction.end_price_d18,
        auction.start_ts,
        auction.end_ts,
        now,
    )?;
    let fill = sell_amount.min(auction.sell_remaining);
    require!(fill > 0, BasketError::ZeroAmount);
    // Payment owed, rounded up in the basket's favour.
    let buy_amount = math::buy_amount_ceil(fill, price_d18)?;
    require!(buy_amount > 0, BasketError::ZeroAmount);
    // Bidder's slippage protection.
    require!(buy_amount <= max_buy_amount, BasketError::SlippageExceeded);

    // Payment first: bidder to basket.
    let vault_buy_before = ctx.accounts.vault_buy_account.amount;
    transfer_checked(
        CpiContext::new(
            ctx.accounts.buy_token_program.key(),
            TransferChecked {
                from: ctx.accounts.bidder_buy_account.to_account_info(),
                mint: ctx.accounts.buy_mint.to_account_info(),
                to: ctx.accounts.vault_buy_account.to_account_info(),
                authority: ctx.accounts.bidder.to_account_info(),
            },
        ),
        buy_amount,
        ctx.accounts.buy_mint.decimals,
    )?;
    // The basket must receive the full payment (no fee skimmed).
    ctx.accounts.vault_buy_account.reload()?;
    require!(
        ctx.accounts
            .vault_buy_account
            .amount
            .checked_sub(vault_buy_before)
            == Some(buy_amount),
        BasketError::TransferAmountMismatch
    );

    // Payout: basket to bidder, signed with the basket PDA's seeds.
    let signer_seeds: &[&[&[u8]]] = &[&[BASKET_CONFIG_SEED, basket.vault.as_ref(), &[basket.bump]]];
    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.sell_token_program.key(),
            TransferChecked {
                from: ctx.accounts.vault_sell_account.to_account_info(),
                mint: ctx.accounts.sell_mint.to_account_info(),
                to: ctx.accounts.bidder_sell_account.to_account_info(),
                authority: ctx.accounts.basket_config.to_account_info(),
            },
            signer_seeds,
        ),
        fill,
        ctx.accounts.sell_mint.decimals,
    )?;

    // Record the sale.
    let auction = &mut ctx.accounts.auction;
    auction.sell_remaining = auction
        .sell_remaining
        .checked_sub(fill)
        .ok_or_else(|| error!(BasketError::MathOverflow))?;
    auction.buy_received = auction
        .buy_received
        .checked_add(buy_amount)
        .ok_or_else(|| error!(BasketError::MathOverflow))?;

    emit!(AuctionBid {
        basket_config: auction.basket_config,
        auction: auction.key(),
        bidder: ctx.accounts.bidder.key(),
        sell_amount: fill,
        buy_amount,
        price_d18,
        sell_remaining: auction.sell_remaining,
    });
    Ok(())
}
