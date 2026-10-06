pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use instructions::*;
pub use state::*;

declare_id!("8FUda9RyAkR2KR6pyBb6h7AVciNQawFRgu5WHZUifTto");

#[program]
pub mod xau_carry_vault {
    use super::*;

    pub fn initialize_vault_v1(
        ctx: Context<InitializeVaultV1>,
    ) -> Result<()> {
        instructions::initialize::handle_initialize(ctx)
    }

    pub fn deposit_usdc_v1(
        ctx: Context<DepositUsdcV1>,
        amount: u64,
    ) -> Result<()> {
        instructions::deposit::handle_deposit(ctx, amount)
    }
}