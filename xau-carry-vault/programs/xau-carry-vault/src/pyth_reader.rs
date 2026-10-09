use anchor_lang::prelude::*;
use std::str::FromStr;

use crate::{
    error::VaultError,
    oracle_math::{validate_usd_price, UsdPrice},
};

const RECEIVER_ADDRESS: &str = "rec2HHDDnjLfj4kE7VyEtFA1HPGQLK33259532cRyHp";

const DISCRIMINATOR: [u8; 8] = [34, 241, 35, 99, 157, 126, 244, 205];

const ACCOUNT_LENGTH: usize = 134;
const FULL_VERIFICATION: u8 = 1;

/// Account offsets for a fully verified PriceUpdateV2.
///
/// Partial verification has a different serialized width.
/// It is rejected before reading these offsets.
const VERIFICATION_OFFSET: usize = 40;
const FEED_OFFSET: usize = 41;
const PRICE_OFFSET: usize = 73;
const CONFIDENCE_OFFSET: usize = 81;
const EXPONENT_OFFSET: usize = 89;
const PUBLISH_TIME_OFFSET: usize = 93;
const PREVIOUS_TIME_OFFSET: usize = 101;
const POSTED_SLOT_OFFSET: usize = 125;

fn receiver_id() -> Result<Pubkey> {
    Pubkey::from_str(RECEIVER_ADDRESS).map_err(|_| error!(VaultError::InvalidOracleOwner))
}

fn bytes<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    let end = offset
        .checked_add(N)
        .ok_or(VaultError::InvalidOracleAccount)?;

    data.get(offset..end)
        .ok_or_else(|| error!(VaultError::InvalidOracleAccount))?
        .try_into()
        .map_err(|_| error!(VaultError::InvalidOracleAccount))
}

/// Internal parser. Production callers must use read_price().
fn parse_price(
    owner: &Pubkey,
    executable: bool,
    data: &[u8],
    expected_feed: &[u8; 32],
    clock: &Clock,
    max_age_seconds: u64,
    max_confidence_bps: u16,
) -> Result<UsdPrice> {
    require_keys_eq!(*owner, receiver_id()?, VaultError::InvalidOracleOwner);

    require!(
        !executable && data.len() == ACCOUNT_LENGTH,
        VaultError::InvalidOracleAccount
    );

    require!(
        bytes::<8>(data, 0)? == DISCRIMINATOR,
        VaultError::InvalidOracleAccount
    );

    require!(
        data[VERIFICATION_OFFSET] == FULL_VERIFICATION,
        VaultError::OracleNotFullyVerified
    );

    // The expected feed must come from trusted protocol configuration,
    // never from a depositor-supplied instruction argument.
    require!(
        *expected_feed != [0; 32] && bytes::<32>(data, FEED_OFFSET)? == *expected_feed,
        VaultError::InvalidOracleFeed
    );

    let posted_slot = u64::from_le_bytes(bytes::<8>(data, POSTED_SLOT_OFFSET)?);

    require!(posted_slot <= clock.slot, VaultError::InvalidOracleSlot);

    let observation = UsdPrice {
        price: i64::from_le_bytes(bytes::<8>(data, PRICE_OFFSET)?),
        confidence: u64::from_le_bytes(bytes::<8>(data, CONFIDENCE_OFFSET)?),
        exponent: i32::from_le_bytes(bytes::<4>(data, EXPONENT_OFFSET)?),
        published_at: i64::from_le_bytes(bytes::<8>(data, PUBLISH_TIME_OFFSET)?),
    };

    let previous_time = i64::from_le_bytes(bytes::<8>(data, PREVIOUS_TIME_OFFSET)?);

    require!(
        previous_time <= observation.published_at,
        VaultError::InvalidOracleAccount
    );

    validate_usd_price(
        &observation,
        clock.unix_timestamp,
        max_age_seconds,
        max_confidence_bps,
    )?;

    Ok(observation)
}

/// Read a fully verified Pyth Core USD price.
///
/// expected_feed and validation limits must come from protocol
/// constants or a validated configuration PDA.
///
/// This reads the transaction's Clock sysvar. The client cannot
/// supply a timestamp to bypass freshness checks.
pub fn read_price(
    account: &AccountInfo<'_>,
    expected_feed: &[u8; 32],
    max_age_seconds: u64,
    max_confidence_bps: u16,
) -> Result<UsdPrice> {
    let clock = Clock::get()?;
    let data = account.try_borrow_data()?;

    parse_price(
        account.owner,
        account.executable,
        &data,
        expected_feed,
        &clock,
        max_age_seconds,
        max_confidence_bps,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic test ID, not the production PAXG feed ID.
    const FEED: [u8; 32] = [7; 32];

    fn clock() -> Clock {
        Clock {
            slot: 100,
            unix_timestamp: 1_000,
            ..Clock::default()
        }
    }

    fn fixture() -> Vec<u8> {
        let mut data = vec![0_u8; ACCOUNT_LENGTH];

        data[..8].copy_from_slice(&DISCRIMINATOR);
        data[VERIFICATION_OFFSET] = FULL_VERIFICATION;
        data[FEED_OFFSET..FEED_OFFSET + 32].copy_from_slice(&FEED);

        data[PRICE_OFFSET..PRICE_OFFSET + 8].copy_from_slice(&300_000_000_000_i64.to_le_bytes());

        data[CONFIDENCE_OFFSET..CONFIDENCE_OFFSET + 8]
            .copy_from_slice(&300_000_000_u64.to_le_bytes());

        data[EXPONENT_OFFSET..EXPONENT_OFFSET + 4].copy_from_slice(&(-8_i32).to_le_bytes());

        data[PUBLISH_TIME_OFFSET..PUBLISH_TIME_OFFSET + 8].copy_from_slice(&990_i64.to_le_bytes());

        data[PREVIOUS_TIME_OFFSET..PREVIOUS_TIME_OFFSET + 8]
            .copy_from_slice(&989_i64.to_le_bytes());

        data[POSTED_SLOT_OFFSET..POSTED_SLOT_OFFSET + 8].copy_from_slice(&99_u64.to_le_bytes());

        data
    }

    fn read(data: &[u8]) -> Result<UsdPrice> {
        parse_price(
            &receiver_id().unwrap(),
            false,
            data,
            &FEED,
            &clock(),
            30,
            100,
        )
    }

    #[test]
    fn fully_verified_price_is_decoded() {
        let observation = read(&fixture()).unwrap();

        assert_eq!(observation.price, 300_000_000_000);
        assert_eq!(observation.confidence, 300_000_000);
        assert_eq!(observation.exponent, -8);
        assert_eq!(observation.published_at, 990);
    }

    #[test]
    fn wrong_owner_and_executable_accounts_are_rejected() {
        let data = fixture();

        assert!(parse_price(
            &Pubkey::new_unique(),
            false,
            &data,
            &FEED,
            &clock(),
            30,
            100,
        )
        .is_err());

        assert!(parse_price(
            &receiver_id().unwrap(),
            true,
            &data,
            &FEED,
            &clock(),
            30,
            100,
        )
        .is_err());
    }

    #[test]
    fn malformed_layouts_are_rejected_without_panicking() {
        let original = fixture();

        for length in 0..ACCOUNT_LENGTH {
            assert!(read(&original[..length]).is_err());
        }

        let mut bad_discriminator = original.clone();
        bad_discriminator[0] ^= 1;
        assert!(read(&bad_discriminator).is_err());

        let mut oversized = original;
        oversized.push(0);
        assert!(read(&oversized).is_err());
    }

    #[test]
    fn partial_and_unknown_verification_are_rejected() {
        let mut data = fixture();

        data[VERIFICATION_OFFSET] = 0;
        assert!(read(&data).is_err());

        data[VERIFICATION_OFFSET] = 2;
        assert!(read(&data).is_err());
    }

    #[test]
    fn different_feed_is_rejected() {
        let mut data = fixture();
        data[FEED_OFFSET] ^= 1;

        assert!(read(&data).is_err());
    }

    #[test]
    fn stale_future_and_uncertain_updates_are_rejected() {
        let mut data = fixture();

        data[PUBLISH_TIME_OFFSET..PUBLISH_TIME_OFFSET + 8].copy_from_slice(&969_i64.to_le_bytes());
        data[PREVIOUS_TIME_OFFSET..PREVIOUS_TIME_OFFSET + 8]
            .copy_from_slice(&968_i64.to_le_bytes());

        assert!(read(&data).is_err());

        data = fixture();
        data[PUBLISH_TIME_OFFSET..PUBLISH_TIME_OFFSET + 8]
            .copy_from_slice(&1_001_i64.to_le_bytes());

        assert!(read(&data).is_err());

        data = fixture();
        data[CONFIDENCE_OFFSET..CONFIDENCE_OFFSET + 8]
            .copy_from_slice(&3_000_000_001_u64.to_le_bytes());

        assert!(read(&data).is_err());
    }

    #[test]
    fn future_posted_slot_is_rejected() {
        let mut data = fixture();

        data[POSTED_SLOT_OFFSET..POSTED_SLOT_OFFSET + 8].copy_from_slice(&101_u64.to_le_bytes());

        assert!(read(&data).is_err());
    }
}
