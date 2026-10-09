use anchor_lang::prelude::*;

#[constant]
pub const BASKET_CONFIG_SEED: &[u8] = b"basket_config";

#[constant]
pub const AUCTION_SEED: &[u8] = b"auction";

#[constant]
pub const ASSET_VAULT_SEED: &[u8] = b"asset_vault";

#[constant]
pub const POSITION_SEED: &[u8] = b"position";

#[constant]
pub const SHARE_DECIMALS: u8 = 6;

#[constant]
pub const MIN_LOCKED_SHARES: u64 = 1_000;

pub use xau_carry_vault::{constants::VXAU_MINT_SEED, ID as XAU_CARRY_VAULT_PROGRAM_ID};

pub const MAX_DISCOUNT_CAP_BPS: u16 = 1_000;
pub const MAX_START_PREMIUM_CAP_BPS: u16 = 1_000;
pub const MAX_CONF_CAP_BPS: u16 = 500;
pub const MAX_AGE_TRADED_CAP_S: u32 = 300;
pub const MAX_AGE_NAV_CAP_S: u32 = 7 * 24 * 60 * 60;
pub const MIN_AUCTION_DURATION_S: u32 = 60;
pub const MAX_AUCTION_DURATION_S: u32 = 24 * 60 * 60;
pub const MAX_MINT_DECIMALS: u8 = 18;

pub const NUM_ASSETS: usize = 4;

#[constant]
pub const BPS_DENOMINATOR: u16 = 10_000;

pub const D18: u128 = 1_000_000_000_000_000_000;

pub const USD_D6_TO_D18: u128 = 1_000_000_000_000;

pub const MULTIPLIER_CHANGE_BUFFER_S: i64 = 900;

pub const PYTH_RECEIVER_PROGRAM_ID: Pubkey =
    Pubkey::from_str_const("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");

pub const PRICE_UPDATE_V2_DISCRIMINATOR: [u8; 8] = [34, 241, 35, 99, 157, 126, 244, 205];
