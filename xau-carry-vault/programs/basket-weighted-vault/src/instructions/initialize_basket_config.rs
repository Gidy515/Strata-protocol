use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use xau_carry_vault::state::VaultV1State;

use crate::{
    constants::*,
    error::BasketError,
    events::BasketInitialized,
    mint_policy::inspect_component_mint,
    program::BasketWeightedVault,
    state::{AssetConfig, PriceSource, RebalanceConfig},
    validation::validate_basket_config,
};

pub use crate::state::BasketConfig;

/// Admin-chosen rebalancing settings for one token.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct AssetParams {
    pub price_source: PriceSource,
    pub band_bps: u16,
    pub max_lot_usd: u64,
}

/// Settings passed alongside token_mints and weights.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct BasketSettings {
    pub assets: [AssetParams; NUM_ASSETS],
    pub initial_units: [u64; NUM_ASSETS],
    pub rebalance: RebalanceConfig,
}

/// Accounts for creating a basket; tokens are 0 vXAU, 1 PAXG, 2 USDY, 3 SPYx.
#[derive(Accounts)]
pub struct InitializeBasketConfig<'info> {
    /// Must be this program's upgrade authority; pays rent.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// This program, used to find its ProgramData account.
    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
            @ BasketError::Unauthorized
    )]
    pub program: Program<'info, BasketWeightedVault>,

    /// Holds the upgrade authority that gates basket creation.
    #[account(
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ BasketError::Unauthorized
    )]
    pub program_data: Account<'info, ProgramData>,

    /// CHECK: Vault account linked to this basket configuration
    pub vault: UncheckedAccount<'info>,

    /// New basket account.
    #[account(
        init,
        payer = authority,
        space = 8 + BasketConfig::INIT_SPACE,
        seeds = [BASKET_CONFIG_SEED, vault.key().as_ref()],
        bump
    )]
    pub basket_config: Box<Account<'info, BasketConfig>>,

    /// Vault 1 state, used to prove token 0 is the real vXAU.
    pub vault_v1_state: Box<Account<'info, VaultV1State>>,

    /// vXAU mint.
    #[account(mint::token_program = token_program_0)]
    pub mint_0: Box<InterfaceAccount<'info, Mint>>,
    /// Basket's vXAU account, owned by the basket config.
    #[account(
        init,
        payer = authority,
        seeds = [ASSET_VAULT_SEED, basket_config.key().as_ref(), mint_0.key().as_ref()],
        bump,
        token::mint = mint_0,
        token::authority = basket_config,
        token::token_program = token_program_0,
    )]
    pub vault_token_0: Box<InterfaceAccount<'info, TokenAccount>>,
    pub token_program_0: Interface<'info, TokenInterface>,

    /// PAXG mint (devnet: mock mint, as Paxos has no devnet PAXG; priced by the real Pyth feed).
    #[account(mint::token_program = token_program_1)]
    pub mint_1: Box<InterfaceAccount<'info, Mint>>,
    /// Basket's PAXG account.
    #[account(
        init,
        payer = authority,
        seeds = [ASSET_VAULT_SEED, basket_config.key().as_ref(), mint_1.key().as_ref()],
        bump,
        token::mint = mint_1,
        token::authority = basket_config,
        token::token_program = token_program_1,
    )]
    pub vault_token_1: Box<InterfaceAccount<'info, TokenAccount>>,
    pub token_program_1: Interface<'info, TokenInterface>,

    /// USDY mint (devnet: mock mint, as Ondo has no devnet USDY; priced by the real Pyth feed).
    #[account(mint::token_program = token_program_2)]
    pub mint_2: Box<InterfaceAccount<'info, Mint>>,
    /// Basket's USDY account.
    #[account(
        init,
        payer = authority,
        seeds = [ASSET_VAULT_SEED, basket_config.key().as_ref(), mint_2.key().as_ref()],
        bump,
        token::mint = mint_2,
        token::authority = basket_config,
        token::token_program = token_program_2,
    )]
    pub vault_token_2: Box<InterfaceAccount<'info, TokenAccount>>,
    pub token_program_2: Interface<'info, TokenInterface>,

    /// SPYx mint (devnet: mock Token-2022 mint with Scaled UI; priced by the real Pyth feed).
    #[account(mint::token_program = token_program_3)]
    pub mint_3: Box<InterfaceAccount<'info, Mint>>,
    /// Basket's SPYx account.
    #[account(
        init,
        payer = authority,
        seeds = [ASSET_VAULT_SEED, basket_config.key().as_ref(), mint_3.key().as_ref()],
        bump,
        token::mint = mint_3,
        token::authority = basket_config,
        token::token_program = token_program_3,
    )]
    pub vault_token_3: Box<InterfaceAccount<'info, TokenAccount>>,
    pub token_program_3: Interface<'info, TokenInterface>,

    pub system_program: Program<'info, System>,
}

/// Validates the tokens and settings, then saves the new basket.
pub fn initialize_basket_config_handler(
    ctx: Context<InitializeBasketConfig>,
    token_mints: [Pubkey; NUM_ASSETS],
    weights: [u16; NUM_ASSETS],
    settings: BasketSettings,
) -> Result<()> {
    let a = &ctx.accounts;
    let mints = [&a.mint_0, &a.mint_1, &a.mint_2, &a.mint_3];
    let vault_tokens = [
        &a.vault_token_0,
        &a.vault_token_1,
        &a.vault_token_2,
        &a.vault_token_3,
    ];
    let programs = [
        &a.token_program_0,
        &a.token_program_1,
        &a.token_program_2,
        &a.token_program_3,
    ];

    // token_mints must match the mint accounts passed in.
    for (expected, mint) in token_mints.iter().zip(mints) {
        require_keys_eq!(*expected, mint.key(), BasketError::MintMismatch);
    }

    // Token 0 must be the vXAU mint recorded by Vault 1, minted only by Vault 1, unfreezable.
    require_keys_eq!(
        a.mint_0.key(),
        a.vault_v1_state.vxau_mint,
        BasketError::InvalidVxauMint
    );
    require!(
        a.mint_0.mint_authority == Some(a.vault_v1_state.key()).into()
            && a.mint_0.freeze_authority.is_none(),
        BasketError::InvalidVxauMint
    );

    require!(
        mints.iter().all(|m| m.decimals <= MAX_MINT_DECIMALS),
        BasketError::InvalidMintDecimals
    );

    require!(
        settings.initial_units.iter().all(|&u| u > 0),
        BasketError::InvalidInitialUnits
    );

    // Reject unsupported Token-2022 extensions and detect multiplier tokens.
    let mut scaled_ui = [false; NUM_ASSETS];
    for (flag, mint) in scaled_ui.iter_mut().zip(mints) {
        *flag = inspect_component_mint(&mint.to_account_info())?;
    }

    // Admin settings plus details read from the accounts.
    let assets: [AssetConfig; NUM_ASSETS] = std::array::from_fn(|i| AssetConfig {
        vault_token: vault_tokens[i].key(),
        token_program: programs[i].key(),
        decimals: mints[i].decimals,
        price_source: settings.assets[i].price_source,
        scaled_ui: scaled_ui[i],
        band_bps: settings.assets[i].band_bps,
        max_lot_usd: settings.assets[i].max_lot_usd,
    });

    // Weights sum to 100%, prices and auction settings are within safe limits.
    validate_basket_config(&weights, &assets, &settings.rebalance)?;

    let basket_config_key = a.basket_config.key();
    let authority = a.authority.key();
    let vault = a.vault.key();
    // Save the basket; it starts with no shares and no auctions.
    ctx.accounts.basket_config.set_inner(BasketConfig {
        authority,
        vault,
        token_mints,
        weights,
        bump: ctx.bumps.basket_config,
        assets,
        initial_units: settings.initial_units,
        total_shares: 0,
        rebalance: settings.rebalance,
        auction_nonce: 0,
        active_auction: false,
        last_auction_end_ts: 0,
        paused: false,
    });

    emit!(BasketInitialized {
        basket_config: basket_config_key,
        vault,
        authority,
        token_mints,
    });
    Ok(())
}
