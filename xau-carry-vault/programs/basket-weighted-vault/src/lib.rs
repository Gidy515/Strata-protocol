//! Strata Vault 2: a weighted basket of vXAU, PAXG, USDY and SPYx, rebalanced by Dutch auctions.

use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod math;
pub mod mint_policy;
pub mod oracle;
pub mod scaled_ui;
pub mod state;
pub mod validation;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("DN3quR3M9kWnw1v9bmY93vdWu3ciRPgb72HPKpQoTDC2");

#[program]
pub mod basket_weighted_vault {
    use super::*;

    /// Admin creates a basket: its token vaults, weights and auction settings.
    pub fn initialize_basket_config(
        ctx: Context<InitializeBasketConfig>,
        token_mints: [Pubkey; 4],
        weights: [u16; 4],
        settings: BasketSettings,
    ) -> Result<()> {
        initialize_basket_config_handler(ctx, token_mints, weights, settings)
    }

    /// User deposits all four tokens pro-rata and receives shares.
    pub fn deposit_to_basket_v2(
        ctx: Context<DepositToBasketV2>,
        max_amounts: [u64; 4],
        min_shares_out: u64,
    ) -> Result<()> {
        deposit_to_basket_v2_handler(ctx, max_amounts, min_shares_out)
    }

    /// Anyone starts a Dutch auction selling an overweight token for an underweight one.
    pub fn open_auction(ctx: Context<OpenAuction>, sell_index: u8, buy_index: u8) -> Result<()> {
        open_auction_handler(ctx, sell_index, buy_index)
    }

    /// Anyone buys from a live auction at the current falling price.
    pub fn bid(ctx: Context<Bid>, sell_amount: u64, max_buy_amount: u64) -> Result<()> {
        bid_handler(ctx, sell_amount, max_buy_amount)
    }

    /// Anyone closes a sold-out or expired auction.
    pub fn close_auction(ctx: Context<CloseAuction>) -> Result<()> {
        close_auction_handler(ctx)
    }

    /// Basket authority turns the emergency stop on or off.
    pub fn set_paused(ctx: Context<SetPaused>, paused: bool) -> Result<()> {
        set_paused_handler(ctx, paused)
    }
}
