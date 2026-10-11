use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{
    constants::*, error::BasketError, events::BasketDeposited,
    instructions::initialize_basket_config::BasketConfig, math, state::UserPosition,
};

/// Accounts for a deposit; constraints pin every token account to the basket's config.
#[derive(Accounts)]
pub struct DepositToBasketV2<'info> {
    /// Depositor; signs the transfers and pays rent for a new position.
    #[account(mut)]
    pub user: Signer<'info>,

    /// The basket; its total shares go up.
    #[account(
        mut,
        seeds = [BASKET_CONFIG_SEED, vault.key().as_ref()],
        bump = basket_config.bump,
        has_one = vault,
    )]
    pub basket_config: Box<Account<'info, BasketConfig>>,

    /// CHECK: Vault account associated with basket
    pub vault: UncheckedAccount<'info>,

    /// User's position, created on first deposit.
    #[account(
        init_if_needed,
        payer = user,
        space = 8 + UserPosition::INIT_SPACE,
        seeds = [POSITION_SEED, basket_config.key().as_ref(), user.key().as_ref()],
        bump,
    )]
    pub position: Box<Account<'info, UserPosition>>,

    /// vXAU mint, the user's vXAU account, the basket's vXAU account and its token program.
    #[account(address = basket_config.token_mints[0] @ BasketError::MintMismatch)]
    pub mint_0: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut, token::mint = mint_0, token::authority = user, token::token_program = token_program_0)]
    pub user_token_0: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, address = basket_config.assets[0].vault_token @ BasketError::InvalidVaultAccount)]
    pub vault_token_0: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = basket_config.assets[0].token_program)]
    pub token_program_0: Interface<'info, TokenInterface>,

    /// PAXG accounts.
    #[account(address = basket_config.token_mints[1] @ BasketError::MintMismatch)]
    pub mint_1: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut, token::mint = mint_1, token::authority = user, token::token_program = token_program_1)]
    pub user_token_1: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, address = basket_config.assets[1].vault_token @ BasketError::InvalidVaultAccount)]
    pub vault_token_1: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = basket_config.assets[1].token_program)]
    pub token_program_1: Interface<'info, TokenInterface>,

    /// USDY accounts.
    #[account(address = basket_config.token_mints[2] @ BasketError::MintMismatch)]
    pub mint_2: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut, token::mint = mint_2, token::authority = user, token::token_program = token_program_2)]
    pub user_token_2: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, address = basket_config.assets[2].vault_token @ BasketError::InvalidVaultAccount)]
    pub vault_token_2: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = basket_config.assets[2].token_program)]
    pub token_program_2: Interface<'info, TokenInterface>,

    /// SPYx accounts (Token-2022).
    #[account(address = basket_config.token_mints[3] @ BasketError::MintMismatch)]
    pub mint_3: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut, token::mint = mint_3, token::authority = user, token::token_program = token_program_3)]
    pub user_token_3: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut, address = basket_config.assets[3].vault_token @ BasketError::InvalidVaultAccount)]
    pub vault_token_3: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = basket_config.assets[3].token_program)]
    pub token_program_3: Interface<'info, TokenInterface>,

    pub system_program: Program<'info, System>,
}

/// Takes at most max_amounts of each token and credits the user with shares.
pub fn deposit_to_basket_v2_handler(
    mut ctx: Context<DepositToBasketV2>,
    max_amounts: [u64; NUM_ASSETS],
    min_shares_out: u64,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let basket_config = &ctx.accounts.basket_config;
    require!(!basket_config.paused, BasketError::Paused);

    // Basket holdings before the deposit.
    let balances = [
        ctx.accounts.vault_token_0.amount,
        ctx.accounts.vault_token_1.amount,
        ctx.accounts.vault_token_2.amount,
        ctx.accounts.vault_token_3.amount,
    ];

    // Shares and exact amounts: recipe on first deposit, pro-rata after.
    let first_deposit = basket_config.total_shares == 0;
    let (shares, amounts) = math::deposit_quote(
        &max_amounts,
        &balances,
        basket_config.total_shares,
        &basket_config.initial_units,
    )?;
    // First deposit locks MIN_LOCKED_SHARES forever against share-price manipulation.
    let user_shares = if first_deposit {
        require!(
            shares > MIN_LOCKED_SHARES,
            BasketError::FirstDepositTooSmall
        );
        shares
            .checked_sub(MIN_LOCKED_SHARES)
            .ok_or_else(|| error!(BasketError::MathOverflow))?
    } else {
        shares
    };
    require!(user_shares > 0, BasketError::ZeroShares);
    // User's slippage protection.
    require!(user_shares >= min_shares_out, BasketError::SlippageExceeded);

    // Move each token from the user to the basket.
    let a = &ctx.accounts;
    let legs = [
        (
            &a.mint_0,
            &a.user_token_0,
            &a.vault_token_0,
            &a.token_program_0,
        ),
        (
            &a.mint_1,
            &a.user_token_1,
            &a.vault_token_1,
            &a.token_program_1,
        ),
        (
            &a.mint_2,
            &a.user_token_2,
            &a.vault_token_2,
            &a.token_program_2,
        ),
        (
            &a.mint_3,
            &a.user_token_3,
            &a.vault_token_3,
            &a.token_program_3,
        ),
    ];
    for (i, (mint, user_token, vault_token, token_program)) in legs.into_iter().enumerate() {
        if amounts[i] == 0 {
            continue;
        }
        transfer_checked(
            CpiContext::new(
                token_program.key(),
                TransferChecked {
                    from: user_token.to_account_info(),
                    mint: mint.to_account_info(),
                    to: vault_token.to_account_info(),
                    authority: a.user.to_account_info(),
                },
            ),
            amounts[i],
            mint.decimals,
        )?;
    }

    // Each basket balance must rise by exactly the amount sent (no fee skimmed).
    let a = &mut ctx.accounts;
    let vault_tokens = [
        &mut a.vault_token_0,
        &mut a.vault_token_1,
        &mut a.vault_token_2,
        &mut a.vault_token_3,
    ];
    for (i, vault_token) in vault_tokens.into_iter().enumerate() {
        vault_token.reload()?;
        require!(
            vault_token.amount.checked_sub(balances[i]) == Some(amounts[i]),
            BasketError::TransferAmountMismatch
        );
    }

    // Record the shares in the user's position.
    let basket_config_key = ctx.accounts.basket_config.key();
    let user_key = ctx.accounts.user.key();
    let position = &mut ctx.accounts.position;
    // New positions are filled in; existing ones must belong to this user and basket.
    if position.owner == Pubkey::default() {
        position.basket_config = basket_config_key;
        position.owner = user_key;
        position.created_ts = now;
        position.bump = ctx.bumps.position;
    } else {
        require!(
            position.owner == user_key && position.basket_config == basket_config_key,
            BasketError::PositionMismatch
        );
    }
    position.shares = position
        .shares
        .checked_add(user_shares)
        .ok_or_else(|| error!(BasketError::MathOverflow))?;
    position.last_deposit_ts = now;
    let position_shares = position.shares;

    // The basket total includes the locked shares.
    let basket_config = &mut ctx.accounts.basket_config;
    basket_config.total_shares = basket_config
        .total_shares
        .checked_add(shares)
        .ok_or_else(|| error!(BasketError::MathOverflow))?;

    emit!(BasketDeposited {
        basket_config: basket_config_key,
        user: user_key,
        amounts,
        shares: user_shares,
        position_shares,
        total_shares: basket_config.total_shares,
    });
    Ok(())
}
