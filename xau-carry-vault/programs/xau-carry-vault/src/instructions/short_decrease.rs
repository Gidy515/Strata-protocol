use anchor_lang::{
    prelude::*,
    solana_program::program::invoke_signed,
    system_program::{self, Transfer},
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount},
};

use crate::{
    adapters::gmtrade::{
        create_short_decrease_instruction, ShortDecreaseParams, ShortOrderAccounts,
    },
    constants::*,
    error::VaultError,
    state::{DecreaseBaselineV1, PendingShortOrderV1, StrategyV1State, VaultV1State},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct ShortDecreaseParamsV1 {
    pub collateral_withdrawal: u64,
    pub size_delta_value: u128,
    pub acceptable_price: u128,
    pub min_usdc_out: u64,
    pub allow_full_close: bool,
    pub execution_lamports: u64,
    pub funding_lamports: u64,
}
#[derive(Accounts)]
#[instruction(nonce: [u8; 32])]
pub struct CreateShortDecreaseV1<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    #[account(
        seeds = [
            VAULT_SEED,
            usdc_mint.key().as_ref()
        ],
        bump = vault_v1_state.bump,
        has_one = admin @ VaultError::UnauthorizedStrategyAdmin,
        has_one = usdc_mint
    )]
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,

    #[account(
        seeds = [
            STRATEGY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump = strategy_v1_state.bump,
        constraint = strategy_v1_state.vault
            == vault_v1_state.key()
            @ VaultError::InvalidStrategyVault,
        has_one = perpetual_program
            @ VaultError::InvalidPerpetualProgram,
        has_one = perpetual_store
            @ VaultError::InvalidPerpetualStore,
        has_one = perpetual_market
            @ VaultError::InvalidPerpetualMarket
    )]
    pub strategy_v1_state: Box<Account<'info, StrategyV1State>>,

    #[account(
        constraint = usdc_mint.key().to_string()
            == MAINNET_USDC_MINT
            @ VaultError::InvalidStrategyDepositMint,
        constraint = usdc_mint.decimals == RECEIPT_DECIMALS
            @ VaultError::InvalidDecimals
    )]
    pub usdc_mint: Box<Account<'info, Mint>>,

    #[account(
        mut,
        seeds = [
            TOKEN_VAULT_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        token::mint = usdc_mint,
        token::authority = vault_v1_state
    )]
    pub vault_v1_usdc_account: Box<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [
            STRATEGY_AUTHORITY_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump,
        constraint = strategy_authority.to_account_info().data_is_empty()
            @ VaultError::InvalidStrategyAuthority
    )]
    pub strategy_authority: SystemAccount<'info>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = strategy_authority
    )]
    pub strategy_usdc_source: Box<Account<'info, TokenAccount>>,

    /// CHECK: Exact program address and executable status are checked.
    #[account(
        constraint = perpetual_program.key().to_string()
            == GMTRADE_PROGRAM_ADDRESS
            @ VaultError::InvalidPerpetualProgram,
        executable
    )]
    pub perpetual_program: UncheckedAccount<'info>,

    /// CHECK: Address and owner are checked; discriminator in handler.
    #[account(
        constraint = perpetual_store.key().to_string()
            == GMTRADE_STORE_ADDRESS
            @ VaultError::InvalidPerpetualStore,
        owner = perpetual_program.key()
    )]
    pub perpetual_store: UncheckedAccount<'info>,

    /// CHECK: Address and owner are checked; metadata in handler.
    #[account(
        mut,
        constraint = perpetual_market.key().to_string()
            == GMTRADE_XAU_MARKET_ADDRESS
            @ VaultError::InvalidPerpetualMarket,
        owner = perpetual_program.key()
    )]
    pub perpetual_market: UncheckedAccount<'info>,

    /// CHECK: Owner and PDA are checked here.
    /// GMTrade additionally validates user initialization and identity.
    #[account(
        mut,
        owner = perpetual_program.key(),
        seeds = [
            GMTRADE_USER_SEED,
            perpetual_store.key().as_ref(),
            strategy_authority.key().as_ref()
        ],
        bump,
        seeds::program = perpetual_program.key()
    )]
    pub perpetual_user: UncheckedAccount<'info>,

    /// CHECK: Owner is checked here; PDA and identity in handler.
    #[account(
        mut,
        owner = perpetual_program.key()
    )]
    pub perpetual_position: UncheckedAccount<'info>,

    /// CHECK: The external order PDA is constrained.
    /// GMTrade initializes it through CPI.
    #[account(
        mut,
        seeds = [
            GMTRADE_ORDER_SEED,
            perpetual_store.key().as_ref(),
            strategy_authority.key().as_ref(),
            &nonce
        ],
        bump,
        seeds::program = perpetual_program.key()
    )]
    pub perpetual_order: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = perpetual_order
    )]
    pub order_usdc_escrow: Box<Account<'info, TokenAccount>>,

    // Singleton per vault. Existing pending state blocks another submission.
    #[account(
        init,
        payer = admin,
        space = 8 + PendingShortOrderV1::INIT_SPACE,
        seeds = [
            PENDING_SHORT_ORDER_SEED,
            vault_v1_state.key().as_ref()
        ],
        bump
    )]
    pub pending_short_order: Box<Account<'info, PendingShortOrderV1>>,

    /// CHECK: Canonical extension PDA. Created and serialized in a separate
    /// function to keep Anchor's generated account-validation stack below 4 KiB.
    #[account(mut, seeds = [DECREASE_BASELINE_SEED, perpetual_order.key().as_ref()], bump)]
    pub order_baseline: UncheckedAccount<'info>,

    /// CHECK: Exact event-authority PDA is checked.
    #[account(
        seeds = [GMTRADE_EVENT_AUTHORITY_SEED],
        bump,
        seeds::program = perpetual_program.key()
    )]
    pub perpetual_event_authority: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_create_decrease(
    ctx: Context<CreateShortDecreaseV1>,
    nonce: [u8; 32],
    p: ShortDecreaseParamsV1,
) -> Result<()> {
    require!(
        ctx.remaining_accounts.len() == 2,
        VaultError::FullStrategyNavRequired
    );
    let now = Clock::get()?;
    let index = crate::strategy_nav::price(
        &ctx.remaining_accounts[0],
        crate::strategy_nav::XAU_FEED,
        now.unix_timestamp,
    )?;
    let usdc = crate::strategy_nav::price(
        &ctx.remaining_accounts[1],
        crate::strategy_nav::USDC_FEED,
        now.unix_timestamp,
    )?;
    let cap = u128::from(index.upper_usd)
        .checked_mul(1_000_000)
        .and_then(|x| x.checked_mul(10_000 + u128::from(MAX_TRADE_DEVIATION_BPS)))
        .ok_or(VaultError::Overflow)?
        / 10_000;
    require!(
        p.acceptable_price > 0 && p.acceptable_price <= cap,
        VaultError::StrategyPriceBound
    );
    let (_, before) = crate::adapters::gmtrade_accounts::market_and_position(
        &ctx.accounts.perpetual_market.to_account_info(),
        &ctx.accounts.perpetual_position.to_account_info(),
        ctx.accounts.strategy_authority.key(),
        ctx.accounts.usdc_mint.key(),
    )?;
    require!(
        p.size_delta_value > 0
            && p.size_delta_value <= before.state.size_in_usd
            && u128::from(p.collateral_withdrawal) <= before.state.collateral_amount,
        VaultError::InvalidShortOrderParameters
    );
    require!(
        ctx.accounts.order_usdc_escrow.amount == 0,
        VaultError::OrderEscrowNotEmpty
    );
    let baseline = DecreaseBaselineV1 {
        vault: ctx.accounts.vault_v1_state.key(),
        order: ctx.accounts.perpetual_order.key(),
        position: ctx.accounts.perpetual_position.key(),
        nonce,
        refund: ctx.accounts.admin.key(),
        submitted_slot: now.slot,
        size_before: before.state.size_in_usd,
        collateral_before: before.state.collateral_amount,
        trade_id_before: before.state.trade_id,
        expected_size_delta: p.size_delta_value,
        min_usdc_out: p.min_usdc_out,
        allow_full_close: p.allow_full_close,
        bump: ctx.bumps.order_baseline,
    };
    persist_baseline(&ctx, &baseline)?;
    drop(before);
    let min_output_value = u128::from(p.min_usdc_out)
        .checked_mul(u128::from(usdc.upper_usd))
        .and_then(|x| x.checked_mul(100_000_000))
        .ok_or(VaultError::Overflow)?;
    let a = ShortOrderAccounts {
        perpetual_program: ctx.accounts.perpetual_program.key(),
        strategy_authority: ctx.accounts.strategy_authority.key(),
        store: ctx.accounts.perpetual_store.key(),
        market: ctx.accounts.perpetual_market.key(),
        user: ctx.accounts.perpetual_user.key(),
        order: ctx.accounts.perpetual_order.key(),
        position: ctx.accounts.perpetual_position.key(),
        usdc_mint: ctx.accounts.usdc_mint.key(),
        order_usdc_escrow: ctx.accounts.order_usdc_escrow.key(),
        strategy_usdc_source: ctx.accounts.strategy_usdc_source.key(),
    };
    let ix = create_short_decrease_instruction(
        &a,
        &nonce,
        &ShortDecreaseParams {
            collateral_withdrawal: p.collateral_withdrawal,
            size_delta_value: p.size_delta_value,
            acceptable_price: p.acceptable_price,
            min_output_value,
            execution_lamports: p.execution_lamports,
        },
    )?;
    if p.funding_lamports > 0 {
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                Transfer {
                    from: ctx.accounts.admin.to_account_info(),
                    to: ctx.accounts.strategy_authority.to_account_info(),
                },
            ),
            p.funding_lamports,
        )?;
    }
    let vault = ctx.accounts.vault_v1_state.key();
    let bump = [ctx.bumps.strategy_authority];
    let seeds: &[&[u8]] = &[STRATEGY_AUTHORITY_SEED, vault.as_ref(), &bump];
    invoke_signed(
        &ix,
        &[
            ctx.accounts.strategy_authority.to_account_info(),
            ctx.accounts.perpetual_store.to_account_info(),
            ctx.accounts.perpetual_market.to_account_info(),
            ctx.accounts.perpetual_user.to_account_info(),
            ctx.accounts.perpetual_order.to_account_info(),
            ctx.accounts.perpetual_position.to_account_info(),
            ctx.accounts.usdc_mint.to_account_info(),
            ctx.accounts.order_usdc_escrow.to_account_info(),
            ctx.accounts.strategy_usdc_source.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
            ctx.accounts.associated_token_program.to_account_info(),
            ctx.accounts.perpetual_event_authority.to_account_info(),
            ctx.accounts.perpetual_program.to_account_info(),
        ],
        &[seeds],
    )?;
    // Keep the authenticated position receipt even on full close, for exact reconciliation.
    let keep_ix = anchor_lang::solana_program::instruction::Instruction {
        program_id: a.perpetual_program,
        accounts: vec![
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(
                a.strategy_authority,
                true,
            ),
            anchor_lang::solana_program::instruction::AccountMeta::new(a.order, false),
        ],
        data: vec![139, 102, 38, 33, 46, 57, 158, 17, 1],
    };
    invoke_signed(
        &keep_ix,
        &[
            ctx.accounts.strategy_authority.to_account_info(),
            ctx.accounts.perpetual_order.to_account_info(),
            ctx.accounts.perpetual_program.to_account_info(),
        ],
        &[seeds],
    )?;
    ctx.accounts.order_usdc_escrow.reload()?;
    require!(
        ctx.accounts.order_usdc_escrow.amount == 0,
        VaultError::InvalidOrderCollateralAccounting
    );
    ctx.accounts
        .pending_short_order
        .set_inner(PendingShortOrderV1 {
            vault,
            order: a.order,
            position: a.position,
            strategy_authority: a.strategy_authority,
            nonce,
            collateral_amount: 0,
            size_delta_value: p.size_delta_value,
            acceptable_price: p.acceptable_price,
            execution_lamports: p.execution_lamports,
            submitted_at_slot: now.slot,
            bump: ctx.bumps.pending_short_order,
        });
    Ok(())
}
#[inline(never)]
fn persist_baseline(
    ctx: &Context<CreateShortDecreaseV1>,
    baseline: &DecreaseBaselineV1,
) -> Result<()> {
    use anchor_lang::system_program::{Allocate, Assign, CreateAccount};
    let info = ctx.accounts.order_baseline.to_account_info();
    require!(
        info.owner == &anchor_lang::system_program::ID && info.data_is_empty() && !info.executable,
        VaultError::InvalidOrderBaseline
    );
    let space = 8 + DecreaseBaselineV1::INIT_SPACE;
    let rent = Rent::get()?.minimum_balance(space);
    let order = baseline.order;
    let bump = [baseline.bump];
    let seeds: &[&[u8]] = &[DECREASE_BASELINE_SEED, order.as_ref(), &bump];
    if info.lamports() == 0 {
        system_program::create_account(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                CreateAccount {
                    from: ctx.accounts.admin.to_account_info(),
                    to: info.clone(),
                },
                &[seeds],
            ),
            rent,
            space as u64,
            &crate::ID,
        )?;
    } else {
        let extra = rent.saturating_sub(info.lamports());
        if extra > 0 {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.key(),
                    Transfer {
                        from: ctx.accounts.admin.to_account_info(),
                        to: info.clone(),
                    },
                ),
                extra,
            )?;
        }
        system_program::allocate(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Allocate {
                    account_to_allocate: info.clone(),
                },
                &[seeds],
            ),
            space as u64,
        )?;
        system_program::assign(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.key(),
                Assign {
                    account_to_assign: info.clone(),
                },
                &[seeds],
            ),
            &crate::ID,
        )?;
    }
    baseline.try_serialize(&mut &mut info.try_borrow_mut_data()?[..])?;
    Ok(())
}
