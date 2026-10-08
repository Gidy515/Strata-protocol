use anchor_lang::prelude::*;

use crate::{
    constants::{BPS_DENOMINATOR, D18, NUM_ASSETS, SHARE_DECIMALS},
    error::BasketError,
};

pub use wide::U256;

#[allow(clippy::manual_div_ceil)]
mod wide {
    uint::construct_uint! {
        pub struct U256(4);
    }
}

fn to_u128(x: U256) -> Result<u128> {
    if x > U256::from(u128::MAX) {
        return err!(BasketError::MathOverflow);
    }
    Ok(x.as_u128())
}

pub fn mul_div_floor(a: u128, b: u128, d: u128) -> Result<u128> {
    require!(d != 0, BasketError::MathOverflow);
    to_u128(U256::from(a) * U256::from(b) / U256::from(d))
}

pub fn mul_div_ceil(a: u128, b: u128, d: u128) -> Result<u128> {
    require!(d != 0, BasketError::MathOverflow);
    let n = U256::from(a) * U256::from(b);
    let d = U256::from(d);
    let q = n / d;
    to_u128(if (n % d).is_zero() { q } else { q + 1 })
}

pub fn pow10(exp: u8) -> Result<u128> {
    10u128
        .checked_pow(exp as u32)
        .ok_or_else(|| error!(BasketError::MathOverflow))
}

pub fn value_usd_d18(raw: u64, price_usd_d18: u128, decimals: u8) -> Result<u128> {
    mul_div_floor(raw as u128, price_usd_d18, pow10(decimals)?)
}

pub fn raw_for_usd_floor(usd_d18: u128, price_usd_d18: u128, decimals: u8) -> Result<u64> {
    let raw = mul_div_floor(usd_d18, pow10(decimals)?, price_usd_d18)?;
    u64::try_from(raw).map_err(|_| error!(BasketError::MathOverflow))
}

pub fn price_ratio_d18(
    sell_price_usd_d18: u128,
    sell_decimals: u8,
    buy_price_usd_d18: u128,
    buy_decimals: u8,
) -> Result<u128> {
    let num = U256::from(sell_price_usd_d18) * U256::from(pow10(buy_decimals)?) * U256::from(D18);
    let den = U256::from(buy_price_usd_d18) * U256::from(pow10(sell_decimals)?);
    require!(!den.is_zero(), BasketError::MathOverflow);
    to_u128(num / den)
}

#[allow(clippy::too_many_arguments)]
pub fn auction_lot_usd_d18(
    value_sell: u128,
    value_buy: u128,
    total: u128,
    target_sell_bps: u16,
    band_sell_bps: u16,
    target_buy_bps: u16,
    max_lot_usd_d18: u128,
) -> Result<u128> {
    let bps = U256::from(BPS_DENOMINATOR);
    let upper_sell = target_sell_bps as u128 + band_sell_bps as u128;
    require!(
        U256::from(value_sell) * bps > U256::from(total) * U256::from(upper_sell),
        BasketError::NotOverweight
    );
    require!(
        U256::from(value_buy) * bps < U256::from(total) * U256::from(target_buy_bps),
        BasketError::NotUnderweight
    );

    let target_sell_value = mul_div_ceil(total, target_sell_bps as u128, BPS_DENOMINATOR as u128)?;
    let target_buy_value = mul_div_floor(total, target_buy_bps as u128, BPS_DENOMINATOR as u128)?;
    let surplus = value_sell.saturating_sub(target_sell_value);
    let deficit = target_buy_value.saturating_sub(value_buy);
    Ok(surplus.min(deficit).min(max_lot_usd_d18))
}

pub fn current_price_d18(
    start: u128,
    end: u128,
    start_ts: i64,
    end_ts: i64,
    now: i64,
) -> Result<u128> {
    if now <= start_ts {
        return Ok(start);
    }
    if now >= end_ts {
        return Ok(end);
    }
    let elapsed = (now - start_ts) as u128;
    let duration = (end_ts - start_ts) as u128;
    let drop = mul_div_floor(start.saturating_sub(end), elapsed, duration)?;
    start
        .checked_sub(drop)
        .ok_or_else(|| error!(BasketError::MathOverflow))
}

pub fn buy_amount_ceil(fill: u64, price_d18: u128) -> Result<u64> {
    let amount = mul_div_ceil(fill as u128, price_d18, D18)?;
    u64::try_from(amount).map_err(|_| error!(BasketError::MathOverflow))
}

pub fn deposit_quote(
    max_amounts: &[u64; NUM_ASSETS],
    balances: &[u64; NUM_ASSETS],
    total_shares: u64,
    initial_units: &[u64; NUM_ASSETS],
) -> Result<(u64, [u64; NUM_ASSETS])> {
    let (numerators, denominators): ([u64; NUM_ASSETS], [u128; NUM_ASSETS]) = if total_shares == 0 {
        let one_share = pow10(SHARE_DECIMALS)?;
        (*initial_units, [one_share; NUM_ASSETS])
    } else {
        (*balances, [total_shares as u128; NUM_ASSETS])
    };

    let mut shares: Option<u128> = None;
    for i in 0..NUM_ASSETS {
        if numerators[i] == 0 {
            continue;
        }
        let s = mul_div_floor(
            max_amounts[i] as u128,
            denominators[i],
            numerators[i] as u128,
        )?;
        shares = Some(shares.map_or(s, |m| m.min(s)));
    }
    let shares = shares.ok_or_else(|| error!(BasketError::EmptyBasket))?;
    let shares = u64::try_from(shares).map_err(|_| error!(BasketError::MathOverflow))?;

    let mut amounts = [0u64; NUM_ASSETS];
    for i in 0..NUM_ASSETS {
        let amount = mul_div_ceil(shares as u128, numerators[i] as u128, denominators[i])?;
        amounts[i] = u64::try_from(amount).map_err(|_| error!(BasketError::MathOverflow))?;
    }
    Ok((shares, amounts))
}