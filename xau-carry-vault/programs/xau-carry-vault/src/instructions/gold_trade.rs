//! Bounded Jupiter shared-account exact-input CPI. The router never receives writable vault state.
use crate::{
    constants::*,
    error::VaultError,
    state::{StrategyV1State, VaultV1State},
};
use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke_signed,
    },
};
use anchor_spl::{
    token::{self, Mint, Token, TokenAccount, TransferChecked},
    token_2022::Token2022,
    token_interface,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct GoldTradeParamsV1 {
    pub amount_in: u64,
    pub min_amount_out: u64,
    /// The Jupiter shared_accounts_route instruction bytes, not an entire transaction.
    pub route_data: Vec<u8>,
}
#[derive(Accounts)]
pub struct GoldTradeV1<'info> {
    pub admin: Signer<'info>,
    #[account(seeds=[VAULT_SEED,usdc_mint.key().as_ref()],bump=vault_v1_state.bump,
        has_one=admin @ VaultError::UnauthorizedStrategyAdmin,has_one=usdc_mint)]
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,
    #[account(seeds=[STRATEGY_SEED,vault_v1_state.key().as_ref()],bump=strategy_v1_state.bump,
        constraint=strategy_v1_state.vault==vault_v1_state.key() @ VaultError::InvalidStrategyVault,
        has_one=gold_mint @ VaultError::InvalidGoldMint)]
    pub strategy_v1_state: Box<Account<'info, StrategyV1State>>,
    #[account(address=crate::adapters::gmtrade_accounts::address(MAINNET_USDC_MINT)?,mint::decimals=6)]
    pub usdc_mint: Box<Account<'info, Mint>>,
    #[account(address=crate::adapters::gmtrade_accounts::address(PAXG_MINT_ADDRESS)?,mint::decimals=6,mint::token_program=gold_token_program)]
    pub gold_mint: Box<InterfaceAccount<'info, token_interface::Mint>>,
    #[account(mut,seeds=[TOKEN_VAULT_SEED,vault_v1_state.key().as_ref()],bump,token::mint=usdc_mint,token::authority=vault_v1_state)]
    pub vault_v1_usdc_account: Box<Account<'info, TokenAccount>>,
    #[account(mut,seeds=[GOLD_VAULT_SEED,vault_v1_state.key().as_ref()],bump,
        token::mint=gold_mint,token::authority=vault_v1_state,token::token_program=gold_token_program,
        constraint=strategy_v1_state.gold_custody==vault_v1_gold_account.key() @ VaultError::InvalidGoldMint)]
    pub vault_v1_gold_account: Box<InterfaceAccount<'info, token_interface::TokenAccount>>,
    /// CHECK: Fixed program address and executable status.
    #[account(address=crate::adapters::gmtrade_accounts::address(JUPITER_PROGRAM_ADDRESS)?,executable)]
    pub jupiter_program: UncheckedAccount<'info>,
    /// CHECK: Fully verified Pyth feed, freshness, confidence and slot checked in handler.
    pub gold_price: UncheckedAccount<'info>,
    /// CHECK: Fully verified Pyth USDC feed checked in handler.
    pub usdc_price: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub gold_token_program: Program<'info, Token2022>,
}
fn fee(mint: &AccountInfo, amount: u64, epoch: u64) -> Result<u64> {
    use anchor_spl::token_2022::spl_token_2022::{
        extension::{
            transfer_fee::TransferFeeConfig, BaseStateWithExtensions, ExtensionType,
            StateWithExtensions,
        },
        state::Mint,
    };
    let data = mint.try_borrow_data()?;
    let state = StateWithExtensions::<Mint>::unpack(&data)?;
    let types = state.get_extension_types()?;
    require!(
        !types.contains(&ExtensionType::TransferHook)
            && !types.contains(&ExtensionType::InterestBearingConfig)
            && !types.contains(&ExtensionType::ScaledUiAmount),
        VaultError::InvalidGoldMint
    );
    if types.contains(&ExtensionType::TransferFeeConfig) {
        state
            .get_extension::<TransferFeeConfig>()?
            .calculate_epoch_fee(epoch, amount)
            .ok_or_else(|| error!(VaultError::Overflow))
    } else {
        Ok(0)
    }
}
fn oracle_floor(amount: u64, input_lower: u64, output_upper: u64) -> Result<u64> {
    require!(output_upper > 0, VaultError::InvalidGoldPrice);
    let value = u128::from(amount)
        .checked_mul(u128::from(input_lower))
        .and_then(|x| x.checked_mul(10_000 - u128::from(MAX_TRADE_DEVIATION_BPS)))
        .ok_or(VaultError::Overflow)?;
    u64::try_from(value / u128::from(output_upper) / 10_000)
        .map_err(|_| error!(VaultError::Overflow))
}
fn same_control(
    a: &anchor_spl::token_2022::spl_token_2022::state::Account,
    b: &token_interface::TokenAccount,
) -> bool {
    a.owner == b.owner
        && a.mint == b.mint
        && a.delegate == b.delegate
        && a.delegated_amount == b.delegated_amount
        && a.close_authority == b.close_authority
        && a.state == b.state
        && a.is_native == b.is_native
}

pub fn handle_gold_trade<'info>(
    ctx: Context<'info, GoldTradeV1<'info>>,
    p: GoldTradeParamsV1,
    buy: bool,
) -> Result<()> {
    if buy {
        require!(
            !ctx.accounts.vault_v1_state.strategy_paused,
            VaultError::StrategyPaused
        );
        require!(
            ctx.accounts.strategy_v1_state.execution_enabled,
            VaultError::StrategyExecutionDisabled
        );
        require!(
            p.amount_in <= ctx.accounts.vault_v1_state.max_order_collateral,
            VaultError::OrderLimitExceeded
        );
    }
    require!(
        p.amount_in > 0 && p.min_amount_out > 0,
        VaultError::InvalidShortOrderParameters
    );
    require!(
        ctx.accounts.strategy_v1_state.gold_token_program == ctx.accounts.gold_token_program.key(),
        VaultError::InvalidGoldMint
    );
    let clock = Clock::get()?;
    let gold = crate::strategy_nav::price(
        &ctx.accounts.gold_price,
        crate::strategy_nav::PAXG_FEED,
        clock.unix_timestamp,
    )?;
    let usdc = crate::strategy_nav::price(
        &ctx.accounts.usdc_price,
        crate::strategy_nav::USDC_FEED,
        clock.unix_timestamp,
    )?;
    let gold_before = *ctx.accounts.vault_v1_gold_account.clone().into_inner();
    let usdc_before = ctx.accounts.vault_v1_usdc_account.amount;
    let (input, output, inmint, outmint) = if buy {
        (
            ctx.accounts.vault_v1_usdc_account.key(),
            ctx.accounts.vault_v1_gold_account.key(),
            ctx.accounts.usdc_mint.key(),
            ctx.accounts.gold_mint.key(),
        )
    } else {
        (
            ctx.accounts.vault_v1_gold_account.key(),
            ctx.accounts.vault_v1_usdc_account.key(),
            ctx.accounts.gold_mint.key(),
            ctx.accounts.usdc_mint.key(),
        )
    };
    let input_balance = if buy { usdc_before } else { gold_before.amount };
    require!(
        input_balance >= p.amount_in,
        VaultError::InsufficientLiquidity
    );
    let oracle_min = if buy {
        let raw = oracle_floor(p.amount_in, usdc.lower_usd, gold.upper_usd)?;
        raw.checked_sub(fee(
            &ctx.accounts.gold_mint.to_account_info(),
            raw,
            clock.epoch,
        )?)
        .ok_or(VaultError::Overflow)?
    } else {
        let net = p
            .amount_in
            .checked_sub(fee(
                &ctx.accounts.gold_mint.to_account_info(),
                p.amount_in,
                clock.epoch,
            )?)
            .ok_or(VaultError::Overflow)?;
        oracle_floor(net, gold.lower_usd, usdc.upper_usd)?
    };
    require!(
        oracle_min > 0 && p.min_amount_out >= oracle_min,
        VaultError::StrategyPriceBound
    );
    let a = ctx.remaining_accounts;
    // Jupiter v6 shared_accounts_route header: 13 accounts followed by router-specific pools.
    require!(
        a.len() >= 13 && a.len() <= 64 && p.route_data.len() >= 32 && p.route_data.len() <= 2048,
        VaultError::InvalidGoldRoute
    );
    let expected_disc: [u8; 8] = [193, 32, 155, 51, 65, 214, 156, 129];
    require!(
        p.route_data[..8] == expected_disc,
        VaultError::InvalidGoldRoute
    );
    let tail = &p.route_data[p.route_data.len() - 19..];
    require!(
        u64::from_le_bytes(tail[..8].try_into().unwrap()) == p.amount_in
            && u16::from_le_bytes(tail[16..18].try_into().unwrap())
                <= MAX_TRADE_DEVIATION_BPS as u16
            && tail[18] == 0,
        VaultError::InvalidGoldRoute
    );
    let router = ctx.accounts.jupiter_program.key();
    let authority = ctx.accounts.vault_v1_state.key();
    let (event, _) = Pubkey::find_program_address(&[b"__event_authority"], &router);
    require!(
        a[0].key() == token::ID
            && a[2].key() == authority
            && a[3].key() == input
            && a[6].key() == output
            && a[7].key() == inmint
            && a[8].key() == outmint
            && a[9].key() == router
            && a[10].key() == ctx.accounts.gold_token_program.key()
            && a[11].key() == event
            && a[12].key() == router,
        VaultError::InvalidGoldRoute
    );
    let mut metas = Vec::with_capacity(a.len());
    for (i, info) in a.iter().enumerate() {
        require!(
            !info.is_signer || info.key() == ctx.accounts.admin.key(),
            VaultError::InvalidGoldRoute
        );
        require!(
            info.key() != ctx.accounts.vault_v1_state.vxau_mint,
            VaultError::InvalidGoldRoute
        );
        let mut writable = info.is_writable;
        // Never expose vault state/mints as writable, even if duplicates in outer transaction are writable.
        if info.key() == authority || info.key() == inmint || info.key() == outmint {
            writable = false;
        }
        require!(
            !writable || *info.owner != crate::ID,
            VaultError::InvalidGoldRoute
        );
        if (*info.owner == token::ID || *info.owner == ctx.accounts.gold_token_program.key())
            && info.key() != input
            && info.key() != output
        {
            if let Ok(t) =
                token_interface::TokenAccount::try_deserialize(&mut &info.try_borrow_data()?[..])
            {
                require!(t.owner != authority, VaultError::InvalidGoldRoute);
            }
        }
        metas.push(if writable {
            AccountMeta::new(info.key(), i == 2)
        } else {
            AccountMeta::new_readonly(info.key(), i == 2)
        });
    }
    let ix = Instruction {
        program_id: router,
        accounts: metas,
        data: p.route_data,
    };
    let bump = [ctx.accounts.vault_v1_state.bump];
    let mint = ctx.accounts.usdc_mint.key();
    let seeds: &[&[u8]] = &[VAULT_SEED, mint.as_ref(), &bump];
    let mut infos = a.to_vec();
    infos.push(ctx.accounts.jupiter_program.to_account_info());
    invoke_signed(&ix, &infos, &[seeds])?;
    ctx.accounts.vault_v1_usdc_account.reload()?;
    ctx.accounts.vault_v1_gold_account.reload()?;
    require!(
        same_control(&gold_before, &ctx.accounts.vault_v1_gold_account),
        VaultError::StrategyAccountChanged
    );
    let after_usdc = &ctx.accounts.vault_v1_usdc_account;
    require!(
        after_usdc.owner == authority
            && after_usdc.mint == ctx.accounts.usdc_mint.key()
            && after_usdc.delegate.is_none()
            && after_usdc.close_authority.is_none(),
        VaultError::StrategyAccountChanged
    );
    let (spent, received) = if buy {
        (
            usdc_before.checked_sub(after_usdc.amount),
            ctx.accounts
                .vault_v1_gold_account
                .amount
                .checked_sub(gold_before.amount),
        )
    } else {
        (
            gold_before
                .amount
                .checked_sub(ctx.accounts.vault_v1_gold_account.amount),
            after_usdc.amount.checked_sub(usdc_before),
        )
    };
    require!(
        spent == Some(p.amount_in) && received.is_some_and(|x| x >= p.min_amount_out),
        VaultError::MinimumOutputNotMet
    );
    Ok(())
}

/// Anyone can return liquid strategy USDC to canonical custody. No value leaves the vault.
#[derive(Accounts)]
pub struct SweepStrategyUsdcV1<'info> {
    #[account(seeds=[VAULT_SEED,usdc_mint.key().as_ref()],bump=vault_v1_state.bump,has_one=usdc_mint)]
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,
    pub usdc_mint: Box<Account<'info, Mint>>,
    /// CHECK: System-owned empty canonical signer PDA.
    #[account(seeds=[STRATEGY_AUTHORITY_SEED,vault_v1_state.key().as_ref()],bump,owner=anchor_lang::system_program::ID,
        constraint=strategy_authority.data_is_empty() @ VaultError::InvalidStrategyAuthority)]
    pub strategy_authority: UncheckedAccount<'info>,
    #[account(mut,associated_token::mint=usdc_mint,associated_token::authority=strategy_authority)]
    pub strategy_usdc_source: Box<Account<'info, TokenAccount>>,
    #[account(mut,seeds=[TOKEN_VAULT_SEED,vault_v1_state.key().as_ref()],bump,token::mint=usdc_mint,token::authority=vault_v1_state)]
    pub vault_v1_usdc_account: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}
pub fn handle_sweep(ctx: Context<SweepStrategyUsdcV1>) -> Result<()> {
    let amount = ctx.accounts.strategy_usdc_source.amount;
    let vault = ctx.accounts.vault_v1_state.key();
    let bump = [ctx.bumps.strategy_authority];
    let seeds: &[&[u8]] = &[STRATEGY_AUTHORITY_SEED, vault.as_ref(), &bump];
    if amount > 0 {
        token::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.strategy_usdc_source.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.vault_v1_usdc_account.to_account_info(),
                    authority: ctx.accounts.strategy_authority.to_account_info(),
                },
                &[seeds],
            ),
            amount,
            RECEIPT_DECIMALS,
        )?;
    }
    Ok(())
}
