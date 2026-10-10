use anchor_lang::prelude::*;

use crate::error::VaultError;

/// Six-decimal gold token and USDC accounting units.
///
/// A price of 3_000_000_000 represents 3,000 USDC per whole gold token.
/// The oracle adapter must normalize its native price into this format.
const UNIT: u128 = 1_000_000;
const BPS: u128 = 10_000;

/// Normalized oracle observation.
///
/// These fields are not independently authenticated by this module.
/// An oracle adapter must validate the source account and feed identity.
pub struct GoldPriceObservation {
    pub price_usdc_per_token: u64,
    pub confidence_usdc_per_token: u64,
    pub published_at: i64,
}

/// Components must represent disjoint balances.
///
/// In particular:
/// - pending collateral belongs in escrow, not position equity;
/// - executed collateral belongs in position equity, not pending escrow;
/// - USDC already counted in custody cannot also be strategy-held USDC.
pub struct NavComponents {
    pub custody_usdc: u64,
    pub strategy_usdc: u64,
    pub gold_value_usdc: u64,
    pub pending_escrow_usdc: u64,

    /// Position collateral + PnL + funding receivable
    /// - borrowing fees - funding payable - other position liabilities.
    ///
    /// Negative equity must remain negative.
    pub perpetual_equity_usdc: i128,

    /// Liabilities outside the position-equity calculation.
    /// Do not include fees already deducted from perpetual equity.
    pub additional_liabilities_usdc: u64,
}

/// Conservative gold valuation using price minus confidence.
///
/// Gold amount and returned USDC value use six-decimal base units.
/// This applies a lower confidence bound, not a liquidation quote.
pub fn conservative_gold_value(
    gold_amount: u64,
    observation: &GoldPriceObservation,
    now: i64,
    max_age_seconds: u64,
    max_confidence_bps: u16,
) -> Result<u64> {
    require!(
        observation.price_usdc_per_token > 0 && max_age_seconds > 0 && max_confidence_bps < 10_000,
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

    // Cross-multiply to avoid truncating the confidence ratio.
    let confidence_scaled = u128::from(observation.confidence_usdc_per_token)
        .checked_mul(BPS)
        .ok_or(VaultError::Overflow)?;

    let allowed_confidence = u128::from(observation.price_usdc_per_token)
        .checked_mul(u128::from(max_confidence_bps))
        .ok_or(VaultError::Overflow)?;

    require!(
        confidence_scaled <= allowed_confidence,
        VaultError::GoldPriceTooUncertain
    );

    let conservative_price = observation
        .price_usdc_per_token
        .checked_sub(observation.confidence_usdc_per_token)
        .ok_or(VaultError::InvalidGoldPrice)?;

    require!(conservative_price > 0, VaultError::InvalidGoldPrice);

    let value = u128::from(gold_amount)
        .checked_mul(u128::from(conservative_price))
        .ok_or(VaultError::Overflow)?
        / UNIT;

    u64::try_from(value).map_err(|_| error!(VaultError::Overflow))
}

/// Returns total net assets in USDC base units.
///
/// Zero NAV is returned as zero. Share pricing separately rejects
/// deposits against zero NAV when outstanding shares exist.
///
/// Negative NAV is rejected. Negative position equity is never
/// silently clamped to zero.
pub fn calculate_nav(components: &NavComponents) -> Result<u64> {
    let mut net = i128::from(components.custody_usdc);

    for asset in [
        components.strategy_usdc,
        components.gold_value_usdc,
        components.pending_escrow_usdc,
    ] {
        net = net
            .checked_add(i128::from(asset))
            .ok_or(VaultError::Overflow)?;
    }

    net = net
        .checked_add(components.perpetual_equity_usdc)
        .ok_or(VaultError::Overflow)?;

    net = net
        .checked_sub(i128::from(components.additional_liabilities_usdc))
        .ok_or(VaultError::Overflow)?;

    require!(net >= 0, VaultError::VaultInsolvent);

    u64::try_from(net).map_err(|_| error!(VaultError::Overflow))
}

#[cfg(test)]
mod tests {
    use super::*;

    const USDC: u64 = 1_000_000;

    fn observation(price: u64, confidence: u64, published_at: i64) -> GoldPriceObservation {
        GoldPriceObservation {
            price_usdc_per_token: price,
            confidence_usdc_per_token: confidence,
            published_at,
        }
    }

    fn components() -> NavComponents {
        NavComponents {
            custody_usdc: 100 * USDC,
            strategy_usdc: 7 * USDC,
            gold_value_usdc: 0,
            pending_escrow_usdc: 0,
            perpetual_equity_usdc: 0,
            additional_liabilities_usdc: 0,
        }
    }

    #[test]
    fn gold_valuation_uses_lower_confidence_bound() {
        // Half a gold token. Price 3,000 USDC, confidence 3 USDC.
        let price = observation(3_000 * USDC, 3 * USDC, 1_000);

        assert_eq!(
            conservative_gold_value(500_000, &price, 1_010, 30, 100,).unwrap(),
            1_498_500_000,
        );
    }

    #[test]
    fn gold_valuation_rounds_down() {
        // One gold base unit, at 1.5 USDC per whole token.
        let price = observation(1_500_000, 0, 1_000);

        assert_eq!(
            conservative_gold_value(1, &price, 1_000, 30, 100).unwrap(),
            1,
        );
    }

    #[test]
    fn stale_future_and_uncertain_prices_are_rejected() {
        let fresh = observation(3_000 * USDC, 0, 1_000);

        assert!(conservative_gold_value(USDC, &fresh, 1_031, 30, 100,).is_err());

        assert!(conservative_gold_value(USDC, &fresh, 999, 30, 100,).is_err());

        let uncertain = observation(3_000 * USDC, 31 * USDC, 1_000);

        assert!(conservative_gold_value(USDC, &uncertain, 1_000, 30, 100,).is_err());

        // Exactly the permitted age and confidence limits.
        let boundary = observation(3_000 * USDC, 30 * USDC, 1_000);

        assert!(conservative_gold_value(USDC, &boundary, 1_030, 30, 100,).is_ok());
    }

    #[test]
    fn order_collateral_movement_preserves_nav() {
        let idle = components();
        let initial_nav = calculate_nav(&idle).unwrap();

        let mut pending = components();
        pending.custody_usdc -= 40 * USDC;
        pending.pending_escrow_usdc = 40 * USDC;

        assert_eq!(calculate_nav(&pending).unwrap(), initial_nav);

        let mut executed = components();
        executed.custody_usdc -= 40 * USDC;
        executed.perpetual_equity_usdc = i128::from(40 * USDC);

        assert_eq!(calculate_nav(&executed).unwrap(), initial_nav);
    }

    #[test]
    fn position_losses_and_other_liabilities_reduce_nav() {
        let mut values = components();

        values.gold_value_usdc = 50 * USDC;
        values.perpetual_equity_usdc = -i128::from(20 * USDC);
        values.additional_liabilities_usdc = 3 * USDC;

        assert_eq!(calculate_nav(&values).unwrap(), 134 * USDC,);
    }

    #[test]
    fn zero_negative_and_overflowing_nav_are_distinguished() {
        let mut values = components();

        values.perpetual_equity_usdc = -i128::from(107 * USDC);
        assert_eq!(calculate_nav(&values).unwrap(), 0);

        values.perpetual_equity_usdc -= 1;
        assert!(calculate_nav(&values).is_err());

        values = components();
        values.perpetual_equity_usdc = i128::MAX;
        assert!(calculate_nav(&values).is_err());

        values = components();
        values.custody_usdc = u64::MAX;
        assert!(calculate_nav(&values).is_err());
    }
}
