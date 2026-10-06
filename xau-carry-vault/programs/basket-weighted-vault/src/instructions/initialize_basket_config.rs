use anchor_lang::prelude::*;

#[derive(Accounts)]
pub struct InitializeBasketConfig<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    /// CHECK: Vault account linked to this basket configuration
    pub vault: UncheckedAccount<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + BasketConfig::INIT_SPACE,
        seeds = [b"basket_config", vault.key().as_ref()],
        bump
    )]
    pub basket_config: Account<'info, BasketConfig>,

    pub system_program: Program<'info, System>,
}

#[account]
#[derive(InitSpace)]
pub struct BasketConfig {
    pub authority: Pubkey,
    pub vault: Pubkey,
    pub token_mints: [Pubkey; 4],
    pub weights: [u16; 4], // Basis points e.g. [2500, 2500, 2500, 2500] = 100%
    pub bump: u8,
}

#[error_code]
pub enum BasketError {
    #[msg("Total weight sum must equal 10,000 basis points (100%).")]
    InvalidWeightSum,
}

pub fn initialize_basket_config_handler(
    ctx: Context<InitializeBasketConfig>,
    token_mints: [Pubkey; 4],
    weights: [u16; 4],
) -> Result<()> {
    let total_weight: u32 = weights.iter().map(|&w| w as u32).sum();
    require!(total_weight == 10_000, BasketError::InvalidWeightSum);

    let basket_config = &mut ctx.accounts.basket_config;
    basket_config.authority = ctx.accounts.authority.key();
    basket_config.vault = ctx.accounts.vault.key();
    basket_config.token_mints = token_mints;
    basket_config.weights = weights;
    basket_config.bump = ctx.bumps.basket_config;

    Ok(())
}
