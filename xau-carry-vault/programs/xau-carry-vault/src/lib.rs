pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use instructions::*;
pub use state::*;

declare_id!("7UjnUp67WKZXAF3gRV4pZgaD6qsWE13gCu116VXsA5cb");

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