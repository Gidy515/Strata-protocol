use anchor_lang::prelude::*;
use anchor_spl::token_interface::TokenAccount;

use crate::{
    constants::*,
    error::BasketError,
    events::AuctionOpened,
    math,
    oracle::load_pyth_price_usd_d18,
    scaled_ui::scaled_ui_multiplier_d18,
    state::{AssetConfig, Auction, BasketConfig, PriceSource, RebalanceConfig},
    validation::validate_basket_config,
};

/// Accounts for opening an auction; balances and prices come as remaining accounts.
#[derive(Accounts)]
pub struct OpenAuction<'info> {
    /// Anyone; pays the auction account's rent and gets it back on close.
    #[account(mut)]
    pub opener: Signer<'info>,
    /// The basket being rebalanced.
    #[account(
        mut,
        seeds = [BASKET_CONFIG_SEED, basket_config.vault.as_ref()],
        bump = basket_config.bump,
    )]
    pub basket_config: Box<Account<'info, BasketConfig>>,
    /// New auction account, numbered by the basket's auction_nonce.
    #[account(
        init,
        payer = opener,
        space = 8 + Auction::INIT_SPACE,
        seeds = [AUCTION_SEED, basket_config.key().as_ref(), &basket_config.auction_nonce.to_le_bytes()],
        bump,
    )]
    pub auction: Box<Account<'info, Auction>>,
    pub system_program: Program<'info, System>,
}

/// Remaining accounts: 4 basket token accounts, 3 Pyth accounts, then the SPYx mint.
pub fn open_auction_handler(
    ctx: Context<OpenAuction>,
    sell_index: u8,
    buy_index: u8,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let basket = &ctx.accounts.basket_config;
    let cfg = basket.rebalance;

    // Not paused, no live auction, cooldown over, two different tokens, valid settings.
    require!(!basket.paused, BasketError::Paused);
    require!(!basket.active_auction, BasketError::AuctionActive);
    require!(
        now >= basket
            .last_auction_end_ts
            .saturating_add(cfg.cooldown_s as i64),
        BasketError::Cooldown
    );
    let (sell, buy) = (sell_index as usize, buy_index as usize);
    require!(
        sell < NUM_ASSETS && buy < NUM_ASSETS && sell != buy,
        BasketError::InvalidAssetIndex
    );
    validate_basket_config(&basket.weights, &basket.assets, &cfg)?;

    // Basket holdings and USD prices, all validated.
    let (balances, prices) = load_basket(
        &basket.token_mints,
        &basket.assets,
        &cfg,
        ctx.remaining_accounts,
        sell,
        buy,
        now,
    )?;

    // USD value of each holding and of the whole basket.
    let mut values = [0u128; NUM_ASSETS];
    let mut total = 0u128;
    for i in 0..NUM_ASSETS {
        values[i] = math::value_usd_d18(balances[i], prices[i], basket.assets[i].decimals)?;
        total = total
            .checked_add(values[i])
            .ok_or_else(|| error!(BasketError::MathOverflow))?;
    }
    require!(total > 0, BasketError::EmptyBasket);

    // Lot = smallest of sell surplus, buy deficit and cap; fails unless out of balance.
    let (sell_asset, buy_asset) = (&basket.assets[sell], &basket.assets[buy]);
    let usd_d18 = |usd_d6: u64| {
        (usd_d6 as u128)
            .checked_mul(USD_D6_TO_D18)
            .ok_or_else(|| error!(BasketError::MathOverflow))
    };
    let lot_usd_d18 = math::auction_lot_usd_d18(
        values[sell],
        values[buy],
        total,
        basket.weights[sell],
        sell_asset.band_bps,
        basket.weights[buy],
        usd_d18(sell_asset.max_lot_usd)?,
    )?;
    require!(
        lot_usd_d18 >= usd_d18(cfg.min_lot_usd)?,
        BasketError::LotTooSmall
    );
    // Convert the USD lot into sell-token units, rounded down.
    let sell_total = math::raw_for_usd_floor(lot_usd_d18, prices[sell], sell_asset.decimals)?;
    require!(sell_total > 0, BasketError::LotTooSmall);

    // Price line: oracle rate plus premium at the start, minus max discount at the end.
    let ratio = math::price_ratio_d18(
        prices[sell],
        sell_asset.decimals,
        prices[buy],
        buy_asset.decimals,
    )?;
    let bps = BPS_DENOMINATOR as u128;
    let start_factor = bps
        .checked_add(cfg.start_premium_bps as u128)
        .ok_or_else(|| error!(BasketError::MathOverflow))?;
    let end_factor = bps
        .checked_sub(cfg.max_discount_bps as u128)
        .ok_or_else(|| error!(BasketError::InvalidConfig))?;
    let start_price_d18 = math::mul_div_ceil(ratio, start_factor, bps)?;
    let end_price_d18 = math::mul_div_ceil(ratio, end_factor, bps)?;
    require!(end_price_d18 > 0, BasketError::InvalidConfig);

    // Save the auction and mark the basket busy.
    let basket_key = ctx.accounts.basket_config.key();
    let auction_key = ctx.accounts.auction.key();
    let nonce = ctx.accounts.basket_config.auction_nonce;
    let end_ts = now
        .checked_add(cfg.auction_duration_s as i64)
        .ok_or_else(|| error!(BasketError::MathOverflow))?;

    ctx.accounts.auction.set_inner(Auction {
        basket_config: basket_key,
        nonce,
        opener: ctx.accounts.opener.key(),
        sell_index,
        buy_index,
        sell_total,
        sell_remaining: sell_total,
        buy_received: 0,
        start_price_d18,
        end_price_d18,
        start_ts: now,
        end_ts,
        sell_price_usd_d18: prices[sell],
        buy_price_usd_d18: prices[buy],
        bump: ctx.bumps.auction,
    });

    let basket = &mut ctx.accounts.basket_config;
    basket.active_auction = true;
    basket.auction_nonce = nonce
        .checked_add(1)
        .ok_or_else(|| error!(BasketError::MathOverflow))?;

    // Bidder bots listen for this.
    emit!(AuctionOpened {
        basket_config: basket_key,
        auction: auction_key,
        nonce,
        sell_index,
        buy_index,
        sell_total,
        lot_usd_d18,
        start_price_d18,
        end_price_d18,
        start_ts: now,
        end_ts,
    });
    Ok(())
}

/// Reads and checks the remaining accounts; returns raw balances and USD prices.
fn load_basket<'info>(
    token_mints: &[Pubkey; NUM_ASSETS],
    assets: &[AssetConfig; NUM_ASSETS],
    cfg: &RebalanceConfig,
    remaining: &'info [AccountInfo<'info>],
    sell: usize,
    buy: usize,
    now: i64,
) -> Result<([u64; NUM_ASSETS], [u128; NUM_ASSETS])> {
    let mut accounts = remaining.iter();
    let mut next = || {
        accounts
            .next()
            .ok_or_else(|| error!(BasketError::InvalidRemainingAccounts))
    };

    // Basket token accounts must match the config exactly.
    let mut balances = [0u64; NUM_ASSETS];
    for (i, asset) in assets.iter().enumerate() {
        let info = next()?;
        require_keys_eq!(
            info.key(),
            asset.vault_token,
            BasketError::InvalidVaultAccount
        );
        require_keys_eq!(
            *info.owner,
            asset.token_program,
            BasketError::InvalidVaultAccount
        );
        let token_account = InterfaceAccount::<TokenAccount>::try_from(info)?;
        require_keys_eq!(
            token_account.mint,
            token_mints[i],
            BasketError::InvalidVaultAccount
        );
        balances[i] = token_account.amount;
    }

    // Traded tokens need fresh prices; valuation-only tokens may use older ones.
    let mut prices = [0u128; NUM_ASSETS];
    for (i, asset) in assets.iter().enumerate() {
        prices[i] = match asset.price_source {
            PriceSource::Fixed { price_usd_d18 } => price_usd_d18,
            PriceSource::Pyth { feed_id } => {
                let max_age = if i == sell || i == buy {
                    cfg.max_age_traded_s
                } else {
                    cfg.max_age_nav_s
                };
                load_pyth_price_usd_d18(next()?, &feed_id, max_age, cfg.max_conf_bps, now)?
            }
        };
        require!(prices[i] > 0, BasketError::InvalidPrice);
    }

    for (i, asset) in assets.iter().enumerate() {
        // SPYx: apply its multiplier; no change may fall inside the auction.
        if asset.scaled_ui {
            let mint_info = next()?;
            require_keys_eq!(
                mint_info.key(),
                token_mints[i],
                BasketError::InvalidScaledUiMint
            );
            let valid_until = if i == sell || i == buy {
                now.saturating_add(cfg.auction_duration_s as i64)
            } else {
                now
            };
            let multiplier = scaled_ui_multiplier_d18(mint_info, now, valid_until)?;
            prices[i] = math::mul_div_floor(prices[i], multiplier, D18)?;
        }
    }

    require!(
        accounts.next().is_none(),
        BasketError::InvalidRemainingAccounts
    );
    Ok((balances, prices))
}
