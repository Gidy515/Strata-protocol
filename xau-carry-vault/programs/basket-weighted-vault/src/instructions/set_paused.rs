use anchor_lang::prelude::*;

use crate::{constants::*, error::BasketError, events::BasketPauseChanged, state::BasketConfig};

/// Accounts for pausing; only the basket's authority may call.
#[derive(Accounts)]
pub struct SetPaused<'info> {
    /// Must match the basket's stored authority.
    pub authority: Signer<'info>,

    /// The basket to pause or unpause.
    #[account(
        mut,
        seeds = [BASKET_CONFIG_SEED, basket_config.vault.as_ref()],
        bump = basket_config.bump,
        has_one = authority @ BasketError::Unauthorized,
    )]
    pub basket_config: Box<Account<'info, BasketConfig>>,
}

/// Turns the emergency stop on or off; closing auctions still works while paused.
pub fn set_paused_handler(ctx: Context<SetPaused>, paused: bool) -> Result<()> {
    let basket_config = &mut ctx.accounts.basket_config;
    basket_config.paused = paused;
    emit!(BasketPauseChanged {
        basket_config: basket_config.key(),
        paused,
    });
    Ok(())
}
