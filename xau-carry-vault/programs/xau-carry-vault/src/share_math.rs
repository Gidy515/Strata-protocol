use anchor_lang::prelude::*;

use crate::error::VaultError;

/// Calculates shares for a deposit.
///
/// All amounts use six-decimal base units:
/// - deposit_amount and net_assets_before: USDC units;
/// - share_supply: vXAU units.
///
/// NAV must be measured BEFORE including the new deposit.
/// Shares round down so a depositor cannot receive excess ownership.
pub fn shares_for_deposit(
    deposit_amount: u64,
    share_supply: u64,
    net_assets_before: u64,
) -> Result<u64> {
    require!(deposit_amount > 0, VaultError::ZeroAmount);

    if share_supply == 0 {
        require!(net_assets_before == 0, VaultError::InvalidShareBootstrap);

        // Initial price: one USDC unit per share unit.
        return Ok(deposit_amount);
    }

    require!(net_assets_before > 0, VaultError::VaultInsolvent);

    let numerator = u128::from(deposit_amount)
        .checked_mul(u128::from(share_supply))
        .ok_or(VaultError::Overflow)?;

    let shares = numerator / u128::from(net_assets_before);

    let shares = u64::try_from(shares).map_err(|_| error!(VaultError::Overflow))?;

    require!(shares > 0, VaultError::DepositTooSmall);

    Ok(shares)
}

/// Calculates USDC value represented by shares being redeemed.
///
/// This calculates entitlement, not available withdrawal liquidity.
/// A handler must separately validate liquidity and minimum output.
///
/// Assets round down. Redeeming the entire supply returns all net assets.
pub fn assets_for_redemption(
    shares_to_redeem: u64,
    share_supply: u64,
    net_assets_before: u64,
) -> Result<u64> {
    require!(shares_to_redeem > 0, VaultError::ZeroAmount);

    require!(
        shares_to_redeem <= share_supply,
        VaultError::InsufficientReceipts
    );

    require!(net_assets_before > 0, VaultError::VaultInsolvent);

    let numerator = u128::from(shares_to_redeem)
        .checked_mul(u128::from(net_assets_before))
        .ok_or(VaultError::Overflow)?;

    let assets = numerator / u128::from(share_supply);

    u64::try_from(assets).map_err(|_| error!(VaultError::Overflow))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: u64 = 1_000_000;

    #[test]
    fn empty_vault_bootstraps_one_to_one() {
        assert_eq!(shares_for_deposit(100 * UNIT, 0, 0).unwrap(), 100 * UNIT,);

        assert!(shares_for_deposit(100 * UNIT, 0, UNIT).is_err());
    }

    #[test]
    fn profit_changes_deposit_and_redemption_prices() {
        // 100 shares now represent 120 USDC.
        let supply = 100 * UNIT;
        let assets = 120 * UNIT;

        // A 12-USDC deposit receives 10 shares.
        assert_eq!(
            shares_for_deposit(12 * UNIT, supply, assets).unwrap(),
            10 * UNIT,
        );

        // Redeeming 10 existing shares returns 12 USDC.
        assert_eq!(
            assets_for_redemption(10 * UNIT, supply, assets).unwrap(),
            12 * UNIT,
        );
    }

    #[test]
    fn loss_changes_deposit_and_redemption_prices() {
        // 100 shares now represent 80 USDC.
        let supply = 100 * UNIT;
        let assets = 80 * UNIT;

        assert_eq!(
            shares_for_deposit(8 * UNIT, supply, assets).unwrap(),
            10 * UNIT,
        );

        assert_eq!(
            assets_for_redemption(10 * UNIT, supply, assets).unwrap(),
            8 * UNIT,
        );
    }

    #[test]
    fn rounding_cannot_profit_a_deposit_then_redemption() {
        let supply = 7;
        let assets = 11;
        let deposit = 5;

        let minted = shares_for_deposit(deposit, supply, assets).unwrap();

        assert_eq!(minted, 3);

        let redeemed = assets_for_redemption(minted, supply + minted, assets + deposit).unwrap();

        assert_eq!(redeemed, 4);
        assert!(redeemed <= deposit);
    }

    #[test]
    fn final_redemption_returns_all_assets() {
        assert_eq!(assets_for_redemption(7, 7, 11).unwrap(), 11,);
    }

    #[test]
    fn invalid_and_insolvent_inputs_are_rejected() {
        assert!(shares_for_deposit(0, 100, 100).is_err());
        assert!(shares_for_deposit(1, 100, 0).is_err());

        // Would round to zero shares.
        assert!(shares_for_deposit(1, 1, 100).is_err());

        assert!(assets_for_redemption(0, 100, 100).is_err());
        assert!(assets_for_redemption(101, 100, 100).is_err());
        assert!(assets_for_redemption(1, 100, 0).is_err());
        assert!(assets_for_redemption(1, 0, 100).is_err());
    }

    #[test]
    fn large_values_do_not_overflow_intermediate_math() {
        assert_eq!(
            shares_for_deposit(u64::MAX, u64::MAX, u64::MAX).unwrap(),
            u64::MAX,
        );

        assert_eq!(
            assets_for_redemption(u64::MAX, u64::MAX, u64::MAX).unwrap(),
            u64::MAX,
        );

        // The calculated share count cannot fit in u64.
        assert!(shares_for_deposit(u64::MAX, u64::MAX, 1).is_err());
    }
}
