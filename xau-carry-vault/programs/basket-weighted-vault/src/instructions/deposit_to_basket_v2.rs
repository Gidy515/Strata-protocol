use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::instructions::initialize_basket_config::BasketConfig;

#[derive(Accounts)]
pub struct DepositToBasketV2<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        seeds = [b"basket_config", vault.key().as_ref()],
        bump = basket_config.bump,
        has_one = vault,
    )]
    pub basket_config: Account<'info, BasketConfig>,

    /// CHECK: Vault account associated with basket
    pub vault: UncheckedAccount<'info>,

    // User token accounts for each of the 4 basket assets
    #[account(mut)]
    pub user_token_0: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_token_1: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_token_2: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_token_3: Account<'info, TokenAccount>,

    // Vault token accounts for each of the 4 basket assets
    #[account(mut)]
    pub vault_token_0: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_1: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_2: Account<'info, TokenAccount>,
    #[account(mut)]
    pub vault_token_3: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn deposit_to_basket_v2_handler(ctx: Context<DepositToBasketV2>, total_amount: u64) -> Result<()> {
    let basket_config = &ctx.accounts.basket_config;

    let user_tokens = [
        &ctx.accounts.user_token_0,
        &ctx.accounts.user_token_1,
        &ctx.accounts.user_token_2,
        &ctx.accounts.user_token_3,
    ];

    let vault_tokens = [
        &ctx.accounts.vault_token_0,
        &ctx.accounts.vault_token_1,
        &ctx.accounts.vault_token_2,
        &ctx.accounts.vault_token_3,
    ];

    for i in 0..4 {
        let weight = basket_config.weights[i] as u64;
        let deposit_amount = (total_amount * weight) / 10_000;

        if deposit_amount > 0 {
            let cpi_accounts = Transfer {
                from: user_tokens[i].to_account_info(),
                to: vault_tokens[i].to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            };

            let cpi_ctx = CpiContext::new(
                ctx.accounts.token_program.key(),
                cpi_accounts,
            );

            token::transfer(cpi_ctx, deposit_amount)?;
        }
    }

    Ok(())
}
