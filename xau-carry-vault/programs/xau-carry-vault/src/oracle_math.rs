use anchor_lang::prelude::*;

use crate::error::VaultError;

const UNIT: u128 = 1_000_000;
const BPS: u128 = 10_000;

/// Native oracle price: price * 10^exponent USD per whole token.
///
/// This is a math input, not proof of an authenticated oracle.
/// The account adapter must verify program ownership, account layout,
/// verification level, and the expected feed ID before constructing it.
#[derive(Clone, Copy, Debug)]
pub struct UsdPrice {
    pub price: i64,
    pub confidence: u64,
    pub exponent: i32,
    pub published_at: i64,
}

/// Validated confidence bounds in six-decimal USD units.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UsdBounds {
    pub(crate) lower: u128,
    pub(crate) upper: u128,
}

fn checked_pow10(exponent: u32) -> Result<u128> {
    10_u128
        .checked_pow(exponent)
        .ok_or_else(|| error!(VaultError::Overflow))
}

/// Validate native values before normalization.
///
/// Confidence is checked in native units so rounding cannot hide
/// an excessively uncertain price.
pub(crate) fn validated_bounds(
    observation: &UsdPrice,
    now: i64,
    max_age_seconds: u64,
    max_confidence_bps: u16,
) -> Result<UsdBounds> {
    require!(
        observation.price > 0 && max_age_seconds > 0 && max_confidence_bps < 10_000,
        VaultError::InvalidGoldPrice
    );

    require!(
        now >= 0 && observation.published_at >= 0 && observation.published_at <= now,
        VaultError::InvalidGoldPriceTimestamp
    );

    let age = now
        .checked_sub(observation.published_at)
        .ok_or(VaultError::InvalidGoldPriceTimestamp)?;

    require!(
        age as u64 <= max_age_seconds,
        VaultError::InvalidGoldPriceTimestamp
    );

    let price = observation.price as u128;
    let confidence = u128::from(observation.confidence);

    require!(
        confidence.checked_mul(BPS).ok_or(VaultError::Overflow)?
            <= price
                .checked_mul(u128::from(max_confidence_bps))
                .ok_or(VaultError::Overflow)?,
        VaultError::GoldPriceTooUncertain
    );

    let native_lower = price
        .checked_sub(confidence)
        .ok_or(VaultError::InvalidGoldPrice)?;

    let native_upper = price.checked_add(confidence).ok_or(VaultError::Overflow)?;

    require!(native_lower > 0, VaultError::InvalidGoldPrice);

    // Normalize USD prices to six decimals.
    // Restrict supported exponents rather than silently losing precision.
    let shift = observation
        .exponent
        .checked_add(6)
        .ok_or(VaultError::InvalidGoldPrice)?;

    require!((-18..=18).contains(&shift), VaultError::InvalidGoldPrice);

    let (lower, upper) = if shift >= 0 {
        let multiplier = checked_pow10(shift as u32)?;

        (
            native_lower
                .checked_mul(multiplier)
                .ok_or(VaultError::Overflow)?,
            native_upper
                .checked_mul(multiplier)
                .ok_or(VaultError::Overflow)?,
        )
    } else {
        let divisor = checked_pow10((-shift) as u32)?;

        // Lower bound rounds down.
        let lower = native_lower / divisor;

        // Upper bound rounds up without adding divisor - 1.
        let upper = (native_upper / divisor)
            .checked_add(u128::from(native_upper % divisor != 0))
            .ok_or(VaultError::Overflow)?;

        (lower, upper)
    };

    require!(lower > 0, VaultError::InvalidGoldPrice);

    Ok(UsdBounds { lower, upper })
}

/// Conservatively value six-decimal gold units in six-decimal USDC units.
///
/// gold value = gold amount * gold USD lower bound / USDC USD upper bound.
///
/// Both feeds must pass age and confidence checks.
/// The returned value rounds down.
///
/// Apply this function directly to the gold balance. Do not subtract
/// confidence again through conservative_gold_value().
pub fn gold_value_in_usdc(
    gold_amount: u64,
    gold_usd: &UsdPrice,
    usdc_usd: &UsdPrice,
    now: i64,
    max_age_seconds: u64,
    gold_max_confidence_bps: u16,
    usdc_max_confidence_bps: u16,
) -> Result<u64> {
    let gold = validated_bounds(gold_usd, now, max_age_seconds, gold_max_confidence_bps)?;

    let usdc = validated_bounds(usdc_usd, now, max_age_seconds, usdc_max_confidence_bps)?;

    let value = u128::from(gold_amount)
        .checked_mul(gold.lower)
        .ok_or(VaultError::Overflow)?
        / usdc.upper;

    u64::try_from(value).map_err(|_| error!(VaultError::Overflow))
}

/// Convert a six-decimal USD liability into six-decimal USDC units.
///
/// Liabilities use the USDC lower bound and round up.
/// This prevents underestimating a USD liability when USDC falls.
pub fn usd_liability_in_usdc(
    liability_usd: u64,
    usdc_usd: &UsdPrice,
    now: i64,
    max_age_seconds: u64,
    usdc_max_confidence_bps: u16,
) -> Result<u64> {
    let usdc = validated_bounds(usdc_usd, now, max_age_seconds, usdc_max_confidence_bps)?;

    let numerator = u128::from(liability_usd)
        .checked_mul(UNIT)
        .ok_or(VaultError::Overflow)?;

    let value = (numerator / usdc.lower)
        .checked_add(u128::from(numerator % usdc.lower != 0))
        .ok_or(VaultError::Overflow)?;

    u64::try_from(value).map_err(|_| error!(VaultError::Overflow))
}

/// Validate an observation without calculating an asset value.
pub fn validate_usd_price(
    observation: &UsdPrice,
    now: i64,
    max_age_seconds: u64,
    max_confidence_bps: u16,
) -> Result<()> {
    validated_bounds(observation, now, max_age_seconds, max_confidence_bps)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000;
    const USDC: u64 = 1_000_000;

    fn price(value: i64, confidence: u64, exponent: i32) -> UsdPrice {
        UsdPrice {
            price: value,
            confidence,
            exponent,
            published_at: NOW,
        }
    }

    fn value(gold: &UsdPrice, usdc: &UsdPrice) -> Result<u64> {
        gold_value_in_usdc(USDC, gold, usdc, NOW, 30, 100, 100)
    }

    #[test]
    fn dollar_peg_preserves_gold_value() {
        let gold = price(300_000_000_000, 0, -8);
        let usdc = price(100_000_000, 0, -8);

        assert_eq!(value(&gold, &usdc).unwrap(), 3_000 * USDC);
    }

    #[test]
    fn usdc_depeg_changes_the_usdc_value() {
        let gold = price(300_000_000_000, 0, -8);

        let discounted_usdc = price(80_000_000, 0, -8);
        assert_eq!(value(&gold, &discounted_usdc).unwrap(), 3_750 * USDC,);

        let premium_usdc = price(120_000_000, 0, -8);
        assert_eq!(value(&gold, &premium_usdc).unwrap(), 2_500 * USDC,);
    }

    #[test]
    fn confidence_uses_lower_gold_and_upper_usdc() {
        let gold = price(300_000_000_000, 300_000_000, -8);
        let usdc = price(100_000_000, 100_000, -8);

        // 2,997 USD / 1.001 USD per USDC, rounded down.
        let expected = (2_997_u128 * 1_000_000 * 1_000_000 / 1_001_000) as u64;

        assert_eq!(value(&gold, &usdc).unwrap(), expected);
    }

    #[test]
    fn different_feed_exponents_are_supported() {
        let gold = price(300_000, 0, -2);
        let usdc = price(100_000_000, 0, -8);

        assert_eq!(value(&gold, &usdc).unwrap(), 3_000 * USDC);

        let gold_positive_exponent = price(3, 0, 3);
        assert_eq!(value(&gold_positive_exponent, &usdc).unwrap(), 3_000 * USDC,);
    }

    #[test]
    fn normalization_rounds_bounds_outward() {
        let gold = price(150_000_099, 0, -8);
        let usdc = price(100_000_001, 0, -8);

        // Gold becomes 1_500_000; USDC upper becomes 1_000_001.
        assert_eq!(value(&gold, &usdc).unwrap(), 1_499_998,);
    }

    #[test]
    fn either_stale_or_future_feed_is_rejected() {
        let gold = price(300_000_000_000, 0, -8);
        let usdc = price(100_000_000, 0, -8);

        let mut stale_gold = gold;
        stale_gold.published_at = NOW - 31;
        assert!(value(&stale_gold, &usdc).is_err());

        let mut stale_usdc = usdc;
        stale_usdc.published_at = NOW - 31;
        assert!(value(&gold, &stale_usdc).is_err());

        let mut future_usdc = usdc;
        future_usdc.published_at = NOW + 1;
        assert!(value(&gold, &future_usdc).is_err());

        let mut boundary = usdc;
        boundary.published_at = NOW - 30;
        assert!(value(&gold, &boundary).is_ok());
    }

    #[test]
    fn uncertain_and_nonpositive_prices_are_rejected() {
        let gold = price(300_000_000_000, 0, -8);
        let usdc = price(100_000_000, 0, -8);

        let uncertain_gold = price(300_000_000_000, 3_000_000_001, -8);
        assert!(value(&uncertain_gold, &usdc).is_err());

        let uncertain_usdc = price(100_000_000, 1_000_001, -8);
        assert!(value(&gold, &uncertain_usdc).is_err());

        assert!(value(&price(0, 0, -8), &usdc).is_err());
        assert!(value(&gold, &price(-1, 0, -8)).is_err());
        assert!(value(&gold, &price(1, 0, -24)).is_err());
        assert!(value(&price(1, 0, i32::MAX), &usdc).is_err());
    }

    #[test]
    fn liabilities_round_up_and_use_lower_usdc_bound() {
        let discounted_usdc = price(80_000_000, 0, -8);

        assert_eq!(
            usd_liability_in_usdc(USDC, &discounted_usdc, NOW, 30, 100,).unwrap(),
            1_250_000,
        );

        let premium_usdc = price(120_000_000, 0, -8);

        assert_eq!(
            usd_liability_in_usdc(1, &premium_usdc, NOW, 30, 100,).unwrap(),
            1,
        );
    }

    #[test]
    fn zero_balance_still_requires_valid_prices() {
        let gold = price(300_000_000_000, 0, -8);
        let usdc = price(100_000_000, 0, -8);

        assert_eq!(
            gold_value_in_usdc(0, &gold, &usdc, NOW, 30, 100, 100,).unwrap(),
            0,
        );

        let invalid_usdc = price(0, 0, -8);

        assert!(gold_value_in_usdc(0, &gold, &invalid_usdc, NOW, 30, 100, 100,).is_err());
    }

    #[test]
    fn oversized_output_is_rejected() {
        let gold = price(300_000_000_000, 0, -8);
        let usdc = price(100_000_000, 0, -8);

        assert!(gold_value_in_usdc(u64::MAX, &gold, &usdc, NOW, 30, 100, 100,).is_err());
    }
}
