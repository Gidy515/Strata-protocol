use anchor_lang::prelude::*;

pub mod instructions;
pub use instructions::*;

declare_id!("2nRrgbBQBr4NB81JnZx7ytnoug71HhFQunY5JyYwyTFt");

#[program]
pub mod basket_weighted_vault {
    use super::*;

    pub fn initialize_basket_config(
        ctx: Context<InitializeBasketConfig>,
        token_mints: [Pubkey; 4],
        weights: [u16; 4],
    ) -> Result<()> {
        initialize_basket_config_handler(ctx, token_mints, weights)
    }

    pub fn deposit_to_basket_v2(
        ctx: Context<DepositToBasketV2>,
        total_amount: u64,
    ) -> Result<()> {
        deposit_to_basket_v2_handler(ctx, total_amount)
    }
}
