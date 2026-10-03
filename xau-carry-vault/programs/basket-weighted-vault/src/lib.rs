use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("Fg6PaFq6N8kwfV46T53vqZFSe2WM5GE5G28Vd75534B");

#[program]
pub mod basket_weighted_vault {
    use super::*;

    pub fn initialize_basket_config(
        ctx: Context<InitializeBasketConfig>,
        vxau_weight: u16,
        rwa_weight: u16,
    ) -> Result<()> {
        let basket_config = &mut ctx.accounts.basket_config;
        basket_config.authority = ctx.accounts.authority.key();
        basket_config.vxau_weight = vxau_weight;
        basket_config.rwa_weight = rwa_weight;
        basket_config.bump = ctx.bumps.basket_config;
        Ok(())
    }

    pub fn deposit_to_basket_v2(ctx: Context<DepositToBasketV2>, amount: u64) -> Result<()> {
        let cpi_accounts = Transfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.vault_token_account.to_account_info(),
            authority: ctx.accounts.user.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            cpi_accounts,
        );

        token::transfer(cpi_ctx, amount)?;

        let vault_state = &mut ctx.accounts.vault_v2_state;
        vault_state.total_deposited = vault_state
            .total_deposited
            .checked_add(amount)
            .ok_or(ErrorCode::Overflow)?;

        msg!("Deposited {} tokens into Vault V2", amount);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeBasketConfig<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        init,
        payer = authority,
        space = 8 + 32 + 2 + 2 + 1,
        seeds = [b"basket_config"],
        bump
    )]
    pub basket_config: Account<'info, BasketConfig>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositToBasketV2<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        seeds = [b"basket_config"],
        bump = basket_config.bump,
    )]
    pub basket_config: Account<'info, BasketConfig>,

    #[account(
        mut,
        seeds = [b"vault_v2", basket_config.key().as_ref()],
        bump = vault_v2_state.bump,
    )]
    pub vault_v2_state: Account<'info, VaultV2State>,

    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub vault_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct BasketConfig {
    pub authority: Pubkey,
    pub vxau_weight: u16,
    pub rwa_weight: u16,
    pub bump: u8,
}

#[account]
pub struct VaultV2State {
    pub basket_config: Pubkey,
    pub total_deposited: u64,
    pub bump: u8,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Arithmetic Overflow")]
    Overflow,
}
