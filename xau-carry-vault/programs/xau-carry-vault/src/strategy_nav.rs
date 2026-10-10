//! Remaining-account contract shared by deposit, redemption and queue settlement.
//! No caller-provided balances, prices, PnL or cached NAV are accepted.
use crate::{
    adapters::gmtrade_accounts as gm,
    constants::*,
    error::VaultError,
    state::{PendingShortOrderV1, StrategyV1State, VaultV1State},
    strategy_accounting::{self, NavInput, NavQuote, PriceBounds},
};
use anchor_lang::prelude::*;
use anchor_spl::{
    token::{Mint, TokenAccount},
    token_interface,
};

pub const NAV_ACCOUNT_COUNT: usize = 14;
pub const XAU_FEED: &str = "765d2ba906dbc32ca17cc11f5310a89e9ee1f6420508c63861f2f8ba4ee34bb2";
pub const PAXG_FEED: &str = "273717b49430906f4b0c230e99aa1007f83758e3199edbc887c0d06c3e332494";
pub const USDC_FEED: &str = "eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a";
pub fn accounting_error(error: strategy_accounting::AccountingError) -> anchor_lang::error::Error {
    use strategy_accounting::AccountingError as A;
    match error {
        A::Overflow => error!(VaultError::Overflow),
        A::Insolvent => error!(VaultError::VaultInsolvent),
        A::InvalidBootstrap => error!(VaultError::InvalidShareBootstrap),
        A::MinimumOutput => error!(VaultError::MinimumOutputNotMet),
        A::InsufficientLiquidity => error!(VaultError::InsufficientLiquidity),
        A::PartialFinalRedemption => error!(VaultError::StrategyUnwindRequired),
        A::UnprovenExecution | A::Replay => error!(VaultError::UnprovenOrderExecution),
        A::OrderMismatch | A::OutstandingOrder => error!(VaultError::InvalidPendingOrder),
        A::ResidualEscrow => error!(VaultError::InvalidOrderCollateralAccounting),
        _ => error!(VaultError::StrategyAccountingFailed),
    }
}
fn feed(hex: &str) -> Result<[u8; 32]> {
    let mut out = [0; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)
            .map_err(|_| error!(VaultError::InvalidOracleFeed))?;
    }
    Ok(out)
}
pub fn price(info: &AccountInfo, hex: &str, now: i64) -> Result<PriceBounds> {
    let raw = crate::pyth_reader::read_price(info, &feed(hex)?, 30, 100)?;
    let bounds = crate::oracle_math::validated_bounds(&raw, now, 30, 100)?;
    Ok(PriceBounds {
        lower_usd: u64::try_from(bounds.lower).map_err(|_| error!(VaultError::Overflow))?,
        upper_usd: u64::try_from(bounds.upper).map_err(|_| error!(VaultError::Overflow))?,
        published_at: raw.published_at,
    })
}
fn read<T: AccountDeserialize>(info: &AccountInfo, owner: Pubkey) -> Result<T> {
    require_keys_eq!(*info.owner, owner, VaultError::InvalidExternalAccountData);
    require!(!info.executable, VaultError::InvalidExternalAccountData);
    T::try_deserialize(&mut &info.try_borrow_data()?[..])
}
fn token(info: &AccountInfo, mint: Pubkey, owner: Pubkey) -> Result<u64> {
    let a: TokenAccount = read(info, anchor_spl::token::ID)?;
    require!(
        a.mint == mint && a.owner == owner,
        VaultError::InvalidPerpetualCollateral
    );
    Ok(a.amount)
}
fn absent(info: &AccountInfo) -> bool {
    *info.owner == anchor_lang::system_program::ID && info.data_is_empty() && !info.executable
}

/// Remaining accounts, in order:
/// authority, strategy USDC ATA, gold mint, gold custody, pending PDA,
/// pending order (program ID sentinel if absent), escrow (program ID sentinel if absent),
/// market, position, position VI (program ID sentinel if absent), index mint,
/// PAXG update, USDC update, XAU update.
pub fn quote(
    vault_key: Pubkey,
    vault: &VaultV1State,
    strategy: &AccountInfo,
    custody: u64,
    remaining: &[AccountInfo],
) -> Result<NavQuote> {
    let clock = Clock::get()?;
    let (strategy_key, _) =
        Pubkey::find_program_address(&[STRATEGY_SEED, vault_key.as_ref()], &crate::ID);
    require_keys_eq!(
        strategy.key(),
        strategy_key,
        VaultError::InvalidStrategyVault
    );
    if absent(strategy) {
        return Ok(NavQuote {
            net_assets: custody,
            available_usdc: custody,
            gold_value_usdc: 0,
            perpetual_equity_usdc: 0,
            observed_slot: clock.slot,
            noncustody_assets_present: false,
        });
    }
    require!(remaining.len() == 14, VaultError::FullStrategyNavRequired);
    let config: StrategyV1State = read(strategy, crate::ID)?;
    require!(
        config.vault == vault_key
            && config.perpetual_program == gm::address(GMTRADE_PROGRAM_ADDRESS)?
            && config.perpetual_store == gm::address(GMTRADE_STORE_ADDRESS)?
            && config.perpetual_market == gm::address(GMTRADE_XAU_MARKET_ADDRESS)?
            && config.gold_mint == gm::address(PAXG_MINT_ADDRESS)?
            && vault.usdc_mint == gm::address(MAINNET_USDC_MINT)?,
        VaultError::InvalidStrategyVault
    );
    let a = remaining;
    let (authority, _) =
        Pubkey::find_program_address(&[STRATEGY_AUTHORITY_SEED, vault_key.as_ref()], &crate::ID);
    require_keys_eq!(a[0].key(), authority, VaultError::InvalidStrategyAuthority);
    require!(absent(&a[0]), VaultError::InvalidStrategyAuthority);
    let source =
        anchor_spl::associated_token::get_associated_token_address(&authority, &vault.usdc_mint);
    require_keys_eq!(a[1].key(), source, VaultError::InvalidPerpetualCollateral);
    let strategy_usdc = if absent(&a[1]) {
        0
    } else {
        token(&a[1], vault.usdc_mint, authority)?
    };
    require_keys_eq!(a[2].key(), config.gold_mint, VaultError::InvalidGoldMint);
    require_keys_eq!(
        *a[2].owner,
        anchor_spl::token_2022::ID,
        VaultError::InvalidGoldMint
    );
    let gold_mint = token_interface::Mint::try_deserialize(&mut &a[2].try_borrow_data()?[..])?;
    require!(
        gold_mint.decimals == GOLD_DECIMALS
            && config.gold_token_program == anchor_spl::token_2022::ID,
        VaultError::InvalidGoldMint
    );
    let (gold_key, _) =
        Pubkey::find_program_address(&[GOLD_VAULT_SEED, vault_key.as_ref()], &crate::ID);
    require!(
        a[3].key() == gold_key && config.gold_custody == gold_key,
        VaultError::InvalidGoldMint
    );
    let gold: token_interface::TokenAccount = read(&a[3], anchor_spl::token_2022::ID)?;
    require!(
        gold.owner == vault_key && gold.mint == config.gold_mint,
        VaultError::InvalidGoldMint
    );
    // PAXG Token-2022 transfer fees reduce realizable assets; withheld fees are not ours.
    let gold_amount = {
        use anchor_spl::token_2022::spl_token_2022::{
            extension::{
                transfer_fee::TransferFeeConfig, BaseStateWithExtensions, ExtensionType,
                StateWithExtensions,
            },
            state::Mint as SplMint,
        };
        let data = a[2].try_borrow_data()?;
        let mint = StateWithExtensions::<SplMint>::unpack(&data)?;
        let extensions = mint.get_extension_types()?;
        // UI-scaled / interest-bearing balances require a separate normalization policy.
        require!(
            !extensions.contains(&ExtensionType::ScaledUiAmount)
                && !extensions.contains(&ExtensionType::InterestBearingConfig),
            VaultError::StrategyAccountingFailed
        );
        let fee = if extensions.contains(&ExtensionType::TransferFeeConfig) {
            mint.get_extension::<TransferFeeConfig>()?
                .calculate_epoch_fee(clock.epoch, gold.amount)
                .ok_or(VaultError::Overflow)?
        } else {
            0
        };
        gold.amount.checked_sub(fee).ok_or(VaultError::Overflow)?
    };
    let (pending_key, _) =
        Pubkey::find_program_address(&[PENDING_SHORT_ORDER_SEED, vault_key.as_ref()], &crate::ID);
    require_keys_eq!(a[4].key(), pending_key, VaultError::InvalidPendingOrder);
    let present = !absent(&a[4]);
    let mut escrow = 0;
    if present {
        let pending: PendingShortOrderV1 = read(&a[4], crate::ID)?;
        require!(
            pending.vault == vault_key
                && pending.strategy_authority == authority
                && pending.order == a[5].key()
                && pending.position == a[8].key(),
            VaultError::InvalidPendingOrder
        );
        let order = gm::decode::<gmsol_programs::gmsol_store::accounts::Order>(
            &a[5],
            &GMTRADE_ORDER_DISCRIMINATOR,
        )?;
        // Executed/cancelled orders must be reconciled before any share-price-changing operation.
        let (order_key, bump) = crate::adapters::gmtrade::order_address(
            &config.perpetual_program,
            &config.perpetual_store,
            &authority,
            &pending.nonce,
        );
        require!(
            order_key == a[5].key()
                && order.header.version == 0
                && order.header.bump == bump
                && order.header.action_state == 0
                && order.header.owner.to_bytes() == authority.to_bytes()
                && order.header.store.to_bytes() == config.perpetual_store.to_bytes()
                && order.header.market.to_bytes() == config.perpetual_market.to_bytes()
                && order.header.nonce == pending.nonce
                && order.params.position.to_bytes() == pending.position.to_bytes()
                && order.params.collateral_token.to_bytes() == vault.usdc_mint.to_bytes()
                && ((order.params.kind == 3 && pending.collateral_amount > 0)
                    || (order.params.kind == 4 && pending.collateral_amount == 0))
                && order.params.side == 1
                && order.params.size_delta_value == pending.size_delta_value,
            VaultError::UnprovenOrderExecution
        );
        require_keys_eq!(
            a[6].key(),
            anchor_spl::associated_token::get_associated_token_address(
                &a[5].key(),
                &vault.usdc_mint
            ),
            VaultError::InvalidPendingOrder
        );
        escrow = token(&a[6], vault.usdc_mint, a[5].key())?;
    } else {
        require!(
            a[5].key() == config.perpetual_program && a[6].key() == config.perpetual_program,
            VaultError::InvalidPendingOrder
        );
    }
    let (market, position) = gm::market_and_position(&a[7], &a[8], authority, vault.usdc_mint)?;
    require_keys_eq!(
        a[10].key(),
        gm::address(GMTRADE_XAU_INDEX_ADDRESS)?,
        VaultError::InvalidPerpetualIndex
    );
    let index_mint: Mint = read(&a[10], anchor_spl::token::ID)?;
    let vi = if market.virtual_inventory_for_positions == Default::default() {
        require_keys_eq!(
            a[9].key(),
            config.perpetual_program,
            VaultError::InvalidExternalAccountData
        );
        None
    } else {
        require!(
            a[9].key().to_bytes() == market.virtual_inventory_for_positions.to_bytes(),
            VaultError::InvalidExternalAccountData
        );
        use gmsol_programs::anchor_lang::Discriminator;
        Some(gm::decode::<
            gmsol_programs::gmsol_store::accounts::VirtualInventory,
        >(
            &a[9],
            gmsol_programs::gmsol_store::accounts::VirtualInventory::DISCRIMINATOR
                .try_into()
                .map_err(|_| error!(VaultError::InvalidExternalAccountData))?,
        )?)
    };
    let paxg = price(&a[11], PAXG_FEED, clock.unix_timestamp)?;
    let usdc = price(&a[12], USDC_FEED, clock.unix_timestamp)?;
    let index = price(&a[13], XAU_FEED, clock.unix_timestamp)?;
    let active = position.state.size_in_usd != 0 || position.state.size_in_tokens != 0;
    let value = gm::value_position(
        market,
        position,
        index,
        usdc,
        index_mint.decimals,
        clock.unix_timestamp,
        vi,
    )?;
    let mut quote = strategy_accounting::full_nav(
        NavInput {
            custody_usdc: custody,
            strategy_usdc,
            gold_amount,
            pending_escrow_usdc: escrow,
            position: value,
            additional_liabilities_usdc: 0,
            observed_slot: clock.slot,
            position_active: active,
            pending_order_present: present,
        },
        paxg,
        usdc,
        clock.unix_timestamp,
        30,
        clock.slot,
    )
    .map_err(accounting_error)?;
    quote.noncustody_assets_present |= gold.amount != 0;
    Ok(quote)
}
