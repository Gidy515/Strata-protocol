use anchor_lang::prelude::*;

pub const VAULT_SEED: &[u8] = b"vault_v1";
pub const TOKEN_VAULT_SEED: &[u8] = b"token_vault";
pub const VXAU_MINT_SEED: &[u8] = b"vxau_mint";

pub const RECEIPT_DECIMALS: u8 = 6;

pub const STRATEGY_SEED: &[u8] = b"strategy_v1";
pub const GOLD_VAULT_SEED: &[u8] = b"gold_vault";

pub const MAINNET_USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

pub const PAXG_MINT_ADDRESS: &str = "5GgRAEmv8ZxF2PR5hY72Qs5x1bnQ6UK2RbTPoqJ3wSwW";

pub const GMTRADE_PROGRAM_ADDRESS: &str = "Gmso1uvJnLbawvw7yezdfCDcPydwW2s2iqG3w6MDucLo";

pub const GMTRADE_STORE_ADDRESS: &str = "CTDLvGGXnoxvqLyTpGzdGLg9pD6JexKxKXSV8tqqo8bN";

pub const GMTRADE_XAU_MARKET_ADDRESS: &str = "59uFARJWg7B8wcEuXzvkafiT4DuKemdNCN5bshDbwun9";

pub const GMTRADE_XAU_INDEX_ADDRESS: &str = "Xauxf2VJhbue14FGbp3W8XwfQSECHYpiPMNMWSCwjSR";

pub const GMTRADE_MARKET_DISCRIMINATOR: [u8; 8] = [219, 190, 213, 55, 0, 227, 198, 154];

pub const GMTRADE_STORE_DISCRIMINATOR: [u8; 8] = [130, 48, 247, 244, 182, 191, 30, 26];

pub const GOLD_DECIMALS: u8 = 6;

pub const STRATEGY_AUTHORITY_SEED: &[u8] = b"strategy_authority";
pub const GMTRADE_USER_SEED: &[u8] = b"user";

pub const GMTRADE_PREPARE_USER_DISCRIMINATOR: [u8; 8] = [190, 173, 143, 193, 139, 80, 231, 133];

pub const GMTRADE_USER_DISCRIMINATOR: [u8; 8] = [12, 78, 211, 244, 225, 77, 209, 249];

pub const GMTRADE_POSITION_SEED: &[u8] = b"position";
pub const GMTRADE_SHORT_POSITION_KIND: u8 = 2;

pub const GMTRADE_PREPARE_POSITION_DISCRIMINATOR: [u8; 8] = [178, 215, 55, 90, 137, 15, 108, 15];

pub const GMTRADE_POSITION_DISCRIMINATOR: [u8; 8] = [170, 188, 143, 228, 122, 64, 247, 208];

pub const PENDING_SHORT_ORDER_SEED: &[u8] = b"pending_short_order";
pub const GMTRADE_ORDER_SEED: &[u8] = b"order";
pub const GMTRADE_EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";

pub const GMTRADE_ORDER_DISCRIMINATOR: [u8; 8] = [134, 173, 223, 185, 77, 86, 28, 51];

pub const WITHDRAWAL_REQUEST_SEED: &[u8] = b"withdrawal_request";
pub const WITHDRAWAL_ESCROW_SEED: &[u8] = b"withdrawal_escrow";
