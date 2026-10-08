//! SPYx reflects dividends and splits via a mint multiplier, so its USD value is raw balance x multiplier x price.

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::{
    extension::{
        scaled_ui_amount::ScaledUiAmountConfig, BaseStateWithExtensions, StateWithExtensions,
    },
    state::Mint,
};

use crate::{
    constants::{D18, MULTIPLIER_CHANGE_BUFFER_S},
    error::BasketError,
};

/// Returns SPYx's current multiplier (scaled by 10^18); fails if it changes from 15 min before now to 15 min after valid_until.
pub fn scaled_ui_multiplier_d18(
    mint_info: &AccountInfo,
    now: i64,
    valid_until: i64,
) -> Result<u128> {
    require_keys_eq!(
        *mint_info.owner,
        anchor_spl::token_2022::ID,
        BasketError::InvalidScaledUiMint
    );
    let data = mint_info.try_borrow_data()?;
    let mint = StateWithExtensions::<Mint>::unpack(&data)
        .map_err(|_| error!(BasketError::InvalidScaledUiMint))?;
    let config = mint
        .get_extension::<ScaledUiAmountConfig>()
        .map_err(|_| error!(BasketError::InvalidScaledUiMint))?;

    let change_ts = i64::from(config.new_multiplier_effective_timestamp);
    require!(
        multiplier_change_is_clear(change_ts, now, valid_until),
        BasketError::MultiplierChangeWindow
    );
    let multiplier = if now >= change_ts {
        f64::from(config.new_multiplier)
    } else {
        f64::from(config.multiplier)
    };
    multiplier_to_d18(multiplier)
}

pub fn multiplier_change_is_clear(change_ts: i64, now: i64, valid_until: i64) -> bool {
    change_ts <= now.saturating_sub(MULTIPLIER_CHANGE_BUFFER_S)
        || change_ts >= valid_until.saturating_add(MULTIPLIER_CHANGE_BUFFER_S)
}

pub fn multiplier_to_d18(multiplier: f64) -> Result<u128> {
    let scaled = multiplier * D18 as f64;
    require!(
        multiplier.is_finite() && multiplier > 0.0 && scaled < u128::MAX as f64,
        BasketError::InvalidScaledUiMint
    );
    Ok(scaled as u128)
}
