use anchor_lang::prelude::*;

#[error_code]
pub enum VaultError {
    #[msg("Only the program upgrade authority may initialize a vault")]
    Unauthorized,

    #[msg("The deposit mint must have six decimals")]
    InvalidDecimals,

    #[msg("Deposit amount must be greater than zero")]
    ZeroAmount,

    #[msg("Cumulative deposits overflowed")]
    Overflow,
}