use anchor_lang::prelude::*;

use crate::{
    constants::{BPS_DENOMINATOR, PRICE_UPDATE_V2_DISCRIMINATOR, PYTH_RECEIVER_PROGRAM_ID},
    error::BasketError,
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerificationLevel {
    Partial { num_signatures: u8 },
    Full,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PriceFeedMessage {
    pub feed_id: [u8; 32],
    pub price: i64,
    pub conf: u64,
    pub exponent: i32,
    pub publish_time: i64,
    pub prev_publish_time: i64,
    pub ema_price: i64,
    pub ema_conf: u64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PriceUpdateV2 {
    pub write_authority: Pubkey,
    pub verification_level: VerificationLevel,
    pub price_message: PriceFeedMessage,
    pub posted_slot: u64,
}

pub fn load_pyth_price_usd_d18(
    info: &AccountInfo,
    feed_id: &[u8; 32],
    max_age_s: u32,
    max_conf_bps: u16,
    now: i64,
) -> Result<u128> {
    require_keys_eq!(
        *info.owner,
        PYTH_RECEIVER_PROGRAM_ID,
        BasketError::InvalidPriceAccount
    );
    let data = info.try_borrow_data()?;
    require!(
        data.len() > 8 && data[..8] == PRICE_UPDATE_V2_DISCRIMINATOR,
        BasketError::InvalidPriceAccount
    );
    let update = PriceUpdateV2::deserialize(&mut &data[8..])
        .map_err(|_| error!(BasketError::InvalidPriceAccount))?;
    validate_price_message(&update, feed_id, max_age_s, max_conf_bps, now)
}

pub fn validate_price_message(
    update: &PriceUpdateV2,
    feed_id: &[u8; 32],
    max_age_s: u32,
    max_conf_bps: u16,
    now: i64,
) -> Result<u128> {
    require!(
        update.verification_level == VerificationLevel::Full,
        BasketError::PriceNotVerified
    );
    let msg = &update.price_message;
    require!(msg.feed_id == *feed_id, BasketError::WrongPriceFeed);
    require!(msg.price > 0, BasketError::InvalidPrice);
    require!(
        now.saturating_sub(msg.publish_time) <= max_age_s as i64,
        BasketError::StalePrice
    );
    require!(
        msg.conf as u128 * BPS_DENOMINATOR as u128 <= max_conf_bps as u128 * msg.price as u128,
        BasketError::PriceConfidence
    );
    price_to_d18(msg.price as u64, msg.exponent)
}

pub fn price_to_d18(price: u64, exponent: i32) -> Result<u128> {
    let shift = 18i32
        .checked_add(exponent)
        .filter(|s| (0..=38).contains(s))
        .ok_or_else(|| error!(BasketError::InvalidPrice))?;
    (price as u128)
        .checked_mul(10u128.pow(shift as u32))
        .ok_or_else(|| error!(BasketError::MathOverflow))
}
