use anchor_lang::prelude::*;

use crate::{
    constants::{
        BPS_DENOMINATOR, MAX_AGE_NAV_CAP_S, MAX_AGE_TRADED_CAP_S, MAX_AUCTION_DURATION_S,
        MAX_CONF_CAP_BPS, MAX_DISCOUNT_CAP_BPS, MAX_START_PREMIUM_CAP_BPS, MIN_AUCTION_DURATION_S,
        NUM_ASSETS,
    },
    error::BasketError,
    state::{AssetConfig, PriceSource, RebalanceConfig},
};

pub fn validate_basket_config(
    weights: &[u16; NUM_ASSETS],
    assets: &[AssetConfig; NUM_ASSETS],
    cfg: &RebalanceConfig,
) -> Result<()> {
    let total_weight: u32 = weights.iter().map(|&w| w as u32).sum();
    require!(
        total_weight == BPS_DENOMINATOR as u32,
        BasketError::InvalidWeightSum
    );

    for (weight, asset) in weights.iter().zip(assets) {
        require!(
            *weight > 0 && asset.band_bps < *weight,
            BasketError::InvalidTargetWeights
        );
        match asset.price_source {
            PriceSource::Fixed { price_usd_d18 } => {
                require!(price_usd_d18 > 0, BasketError::InvalidPriceSource)
            }
            PriceSource::Pyth { feed_id } => {
                require!(feed_id != [0; 32], BasketError::InvalidPriceSource)
            }
        }
        require!(
            asset.max_lot_usd >= cfg.min_lot_usd,
            BasketError::InvalidConfig
        );
    }

    require!(
        (MIN_AUCTION_DURATION_S..=MAX_AUCTION_DURATION_S).contains(&cfg.auction_duration_s)
            && cfg.max_discount_bps <= MAX_DISCOUNT_CAP_BPS
            && cfg.start_premium_bps <= MAX_START_PREMIUM_CAP_BPS
            && cfg.max_age_traded_s > 0
            && cfg.max_age_traded_s <= MAX_AGE_TRADED_CAP_S
            && cfg.max_age_traded_s <= cfg.max_age_nav_s
            && cfg.max_age_nav_s <= MAX_AGE_NAV_CAP_S
            && cfg.max_conf_bps > 0
            && cfg.max_conf_bps <= MAX_CONF_CAP_BPS
            && cfg.min_lot_usd > 0,
        BasketError::InvalidConfig
    );
    Ok(())
}
