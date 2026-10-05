use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct VaultV1State {
    pub admin: Pubkey,
    pub usdc_mint: Pubkey,
    pub vxau_mint: Pubkey,
    pub total_deposited: u64,
    pub bump: u8,
}
