//! Pinned GMTrade v0 zero-copy layouts. Reject unknown sizes/versions.
//! Account inputs are authenticated before protocol-model valuation.
use crate::{
    constants::*,
    error::VaultError,
    strategy_accounting::{PositionValuation, PriceBounds},
};
use anchor_lang::prelude::*;
use gmsol_model::price::{Price, Prices};
use gmsol_model::{
    action::MarketAction, num::Unsigned, BorrowingFeeMarketMutExt, PerpMarketExt, PerpMarketMutExt,
    Position as ModelPosition, PositionExt, PositionState,
};
use gmsol_programs::gmsol_store::accounts::{Market, Order, Position, VirtualInventory};
use gmsol_programs::model::{MarketModel, PositionModel};
use std::sync::Arc;

fn fail() -> anchor_lang::error::Error {
    error!(VaultError::StrategyAccountingFailed)
}
pub fn address(text: &str) -> Result<Pubkey> {
    text.parse().map_err(|_| fail())
}
/// Heap copy avoids misalignment and multi-kilobyte SBF stack frames.
#[inline(never)]
pub fn decode<T: bytemuck::Pod>(info: &AccountInfo, discriminator: &[u8; 8]) -> Result<Box<T>> {
    require_keys_eq!(
        *info.owner,
        address(GMTRADE_PROGRAM_ADDRESS)?,
        VaultError::InvalidPerpetualProgram
    );
    require!(!info.executable, VaultError::InvalidExternalAccountData);
    let data = info.try_borrow_data()?;
    require!(
        data.len() == 8 + core::mem::size_of::<T>() && data.get(..8) == Some(discriminator),
        VaultError::InvalidExternalAccountData
    );
    let mut value = bytemuck::allocation::zeroed_box::<T>();
    bytemuck::bytes_of_mut(&mut *value).copy_from_slice(&data[8..]);
    Ok(value)
}
#[inline(never)]
pub fn market_and_position(
    market_info: &AccountInfo,
    position_info: &AccountInfo,
    authority: Pubkey,
    usdc: Pubkey,
) -> Result<(Box<Market>, Box<Position>)> {
    require_keys_eq!(
        market_info.key(),
        address(GMTRADE_XAU_MARKET_ADDRESS)?,
        VaultError::InvalidPerpetualMarket
    );
    let market = decode::<Market>(market_info, &GMTRADE_MARKET_DISCRIMINATOR)?;
    require!(
        market.version == 0
            && market.store.to_bytes() == address(GMTRADE_STORE_ADDRESS)?.to_bytes()
            && market.meta.index_token_mint.to_bytes()
                == address(GMTRADE_XAU_INDEX_ADDRESS)?.to_bytes()
            && market.meta.long_token_mint.to_bytes() == usdc.to_bytes()
            && market.meta.short_token_mint.to_bytes() == usdc.to_bytes(),
        VaultError::InvalidExternalAccountData
    );
    let program = address(GMTRADE_PROGRAM_ADDRESS)?;
    let store = address(GMTRADE_STORE_ADDRESS)?;
    let market_token = market.meta.market_token_mint.to_bytes();
    let (expected, bump) = Pubkey::find_program_address(
        &[
            GMTRADE_POSITION_SEED,
            store.as_ref(),
            authority.as_ref(),
            &market_token,
            usdc.as_ref(),
            &[GMTRADE_SHORT_POSITION_KIND],
        ],
        &program,
    );
    require_keys_eq!(
        position_info.key(),
        expected,
        VaultError::InvalidPerpetualPosition
    );
    let position = if *position_info.owner == anchor_lang::system_program::ID
        && position_info.data_is_empty()
        && !position_info.executable
    {
        // The canonical position may not exist yet, or may have been closed after unwind.
        // Empty, system-owned canonical PDA proves no current position account exists.
        let mut value = bytemuck::allocation::zeroed_box::<Position>();
        value.bump = bump;
        value.kind = GMTRADE_SHORT_POSITION_KIND;
        value.store =
            gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(store.to_bytes());
        value.owner =
            gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(authority.to_bytes());
        value.market_token = market.meta.market_token_mint;
        value.collateral_token =
            gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(usdc.to_bytes());
        value
    } else {
        decode::<Position>(position_info, &GMTRADE_POSITION_DISCRIMINATOR)?
    };
    require!(
        position.version == 0
            && position.bump == bump
            && position.kind == GMTRADE_SHORT_POSITION_KIND
            && position.store.to_bytes() == store.to_bytes()
            && position.owner.to_bytes() == authority.to_bytes()
            && position.market_token.to_bytes() == market_token
            && position.collateral_token.to_bytes() == usdc.to_bytes(),
        VaultError::InvalidPerpetualPosition
    );
    Ok((market, position))
}
/// Completed (1) is successful execution; failed execution becomes Cancelled (2).
/// Never infer success from absence/closure or escrow balance alone.
pub fn executed_order(
    info: &AccountInfo,
    vault: &crate::state::OrderBaselineV1,
    authority: Pubkey,
    now_slot: u64,
) -> Result<Box<Order>> {
    let order = decode::<Order>(info, &GMTRADE_ORDER_DISCRIMINATOR)?;
    let store = address(GMTRADE_STORE_ADDRESS)?;
    let (expected, bump) = Pubkey::find_program_address(
        &[
            GMTRADE_ORDER_SEED,
            store.as_ref(),
            authority.as_ref(),
            &vault.nonce,
        ],
        &address(GMTRADE_PROGRAM_ADDRESS)?,
    );
    require_keys_eq!(info.key(), expected, VaultError::InvalidPendingOrder);
    require!(
        order.header.version == 0
            && order.header.bump == bump
            && order.header.action_state == 1
            && order.header.store.to_bytes() == store.to_bytes()
            && order.header.market.to_bytes() == address(GMTRADE_XAU_MARKET_ADDRESS)?.to_bytes()
            && order.header.owner.to_bytes() == authority.to_bytes()
            && order.header.nonce == vault.nonce
            && order.params.position.to_bytes() == vault.position.to_bytes()
            && order.params.kind == 3
            && order.params.side == 1
            && order.params.size_delta_value == vault.expected_size_delta
            && order.header.updated_at_slot >= vault.submitted_slot
            && order.header.updated_at_slot <= now_slot
            && order.builder_fee_amount == 0
            && order.builder_fee_factor == 0,
        VaultError::UnprovenOrderExecution
    );
    Ok(order)
}
fn usd_down(v: u128) -> Result<u64> {
    u64::try_from(v / 100_000_000_000_000).map_err(|_| fail())
}
fn usd_up(v: u128) -> Result<u64> {
    u64::try_from(v / 100_000_000_000_000 + u128::from(v % 100_000_000_000_000 != 0))
        .map_err(|_| fail())
}
fn model_price(bounds: PriceBounds, decimals: u8) -> Result<Price<u128>> {
    let factor = 10u128
        .checked_pow(14u32.checked_sub(decimals as u32).ok_or_else(fail)?)
        .ok_or_else(fail)?;
    Ok(Price {
        min: u128::from(bounds.lower_usd)
            .checked_mul(factor)
            .ok_or_else(fail)?,
        max: u128::from(bounds.upper_usd)
            .checked_mul(factor)
            .ok_or_else(fail)?,
    })
}
#[inline(never)]
pub fn value_position(
    market: Box<Market>,
    position: Box<Position>,
    index: PriceBounds,
    usdc: PriceBounds,
    index_decimals: u8,
    timestamp: i64,
    vi: Option<Box<VirtualInventory>>,
) -> Result<PositionValuation> {
    require!(
        market.state.clocks.funding <= timestamp && market.state.clocks.borrowing <= timestamp,
        VaultError::StrategyAccountingFailed
    );
    if position.state.size_in_usd == 0 {
        require!(
            position.state.size_in_tokens == 0 && position.state.collateral_amount == 0,
            VaultError::StrategyAccountingFailed
        );
        return Ok(PositionValuation::default());
    }
    let collateral = u64::try_from(position.state.collateral_amount).map_err(|_| fail())?;
    let collateral_price = model_price(usdc, 6)?;
    let prices = Prices {
        index_token_price: model_price(index, index_decimals)?,
        long_token_price: collateral_price,
        short_token_price: collateral_price,
    };
    let mut market = MarketModel::from_parts(Arc::from(market), 0, timestamp);
    market.attach_position_vi(
        vi.map(|v| gmsol_programs::model::VirtualInventoryModel::from_parts(Arc::from(v))),
    );
    market
        .update_borrowing(&prices)
        .and_then(|a| a.execute())
        .map_err(|_| fail())?;
    market
        .update_funding(&prices)
        .and_then(|a| a.execute())
        .map_err(|_| fail())?;
    let model = PositionModel::new(market, Arc::from(position)).map_err(|_| fail())?;
    let size = model.size_in_usd();
    let (pnl, _, _) = model.pnl_value(&prices, size).map_err(|_| fail())?;
    let pnl6 = if pnl >= 0 {
        i128::from(usd_down(pnl as u128)?)
    } else {
        -i128::from(usd_up(pnl.unsigned_abs())?)
    };
    let funding = model.pending_funding_fees().map_err(|_| fail())?;
    // Both collateral tokens are USDC; sum each funding credit once.
    let claimable = funding
        .claimable_long_token_amount()
        .checked_add(*funding.claimable_short_token_amount())
        .ok_or_else(fail)?;
    let delta = size.to_opposite_signed().map_err(|_| fail())?;
    let impact = model
        .position_price_impact(&delta, true)
        .map_err(|_| fail())?;
    let mut adverse = impact.value.min(0);
    model
        .market()
        .cap_negative_position_price_impact(&delta, false, &mut adverse)
        .map_err(|_| fail())?;
    let fees = model
        .position_fees(&collateral_price, size, impact.balance_change, false)
        .map_err(|_| fail())?;
    Ok(PositionValuation {
        collateral_usdc: collateral,
        capped_pnl_usd: pnl6,
        claimable_funding_usdc: u64::try_from(claimable).map_err(|_| fail())?,
        borrowing_fee_usd: usd_up(model.pending_borrowing_fee_value().map_err(|_| fail())?)?,
        funding_payable_usdc: u64::try_from(*funding.amount()).map_err(|_| fail())?,
        close_fee_usd: usd_up(*fees.order_fees().fee_value())?,
        adverse_close_impact_usd: usd_up(adverse.unsigned_abs())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn info<'a>(
        key: &'a Pubkey,
        owner: &'a Pubkey,
        lamports: &'a mut u64,
        data: &'a mut [u8],
    ) -> AccountInfo<'a> {
        AccountInfo::new(key, false, false, lamports, data, owner, false)
    }
    fn bytes<T: bytemuck::Pod>(value: &T, disc: &[u8; 8]) -> Vec<u8> {
        let mut b = disc.to_vec();
        b.extend_from_slice(bytemuck::bytes_of(value));
        b
    }
    #[test]
    fn captured_mainnet_market_layout_and_mints_match() {
        let mut d = include_bytes!("../../tests/fixtures/gmtrade/xau_market.bin").to_vec();
        let key = address(GMTRADE_XAU_MARKET_ADDRESS).unwrap();
        let owner = address(GMTRADE_PROGRAM_ADDRESS).unwrap();
        let mut l = 1;
        let m = decode::<Market>(
            &info(&key, &owner, &mut l, &mut d),
            &GMTRADE_MARKET_DISCRIMINATOR,
        )
        .unwrap();
        assert_eq!(m.version, 0);
        assert_eq!(
            m.store.to_bytes(),
            address(GMTRADE_STORE_ADDRESS).unwrap().to_bytes()
        );
        assert_eq!(
            m.meta.index_token_mint.to_bytes(),
            address(GMTRADE_XAU_INDEX_ADDRESS).unwrap().to_bytes()
        );
        assert_eq!(
            m.meta.long_token_mint.to_bytes(),
            address(MAINNET_USDC_MINT).unwrap().to_bytes()
        );
        assert_eq!(m.meta.short_token_mint, m.meta.long_token_mint);
        assert_eq!(core::mem::size_of::<Market>() + 8, d.len());
    }
    #[test]
    fn owner_discriminator_and_size_are_mandatory() {
        let key = Pubkey::new_unique();
        let owner = address(GMTRADE_PROGRAM_ADDRESS).unwrap();
        let wrong = Pubkey::new_unique();
        let mut l = 1;
        let mut d = include_bytes!("../../tests/fixtures/gmtrade/xau_market.bin").to_vec();
        assert!(decode::<Market>(
            &info(&key, &wrong, &mut l, &mut d),
            &GMTRADE_MARKET_DISCRIMINATOR
        )
        .is_err());
        d[0] ^= 1;
        assert!(decode::<Market>(
            &info(&key, &owner, &mut l, &mut d),
            &GMTRADE_MARKET_DISCRIMINATOR
        )
        .is_err());
        d[0] ^= 1;
        d.push(0);
        assert!(decode::<Market>(
            &info(&key, &owner, &mut l, &mut d),
            &GMTRADE_MARKET_DISCRIMINATOR
        )
        .is_err());
    }
    #[test]
    fn canonical_position_identity_and_metadata_required() {
        let mut md = include_bytes!("../../tests/fixtures/gmtrade/xau_market.bin").to_vec();
        let market = address(GMTRADE_XAU_MARKET_ADDRESS).unwrap();
        let owner = address(GMTRADE_PROGRAM_ADDRESS).unwrap();
        let mut ml = 1;
        let mi = info(&market, &owner, &mut ml, &mut md);
        let m = decode::<Market>(&mi, &GMTRADE_MARKET_DISCRIMINATOR).unwrap();
        let authority = Pubkey::new_unique();
        let usdc = address(MAINNET_USDC_MINT).unwrap();
        let store = address(GMTRADE_STORE_ADDRESS).unwrap();
        let (key, bump) = Pubkey::find_program_address(
            &[
                GMTRADE_POSITION_SEED,
                store.as_ref(),
                authority.as_ref(),
                m.meta.market_token_mint.as_ref(),
                usdc.as_ref(),
                &[2],
            ],
            &owner,
        );
        let mut p = bytemuck::allocation::zeroed_box::<Position>();
        p.bump = bump;
        p.kind = 2;
        p.store = gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(store.to_bytes());
        p.owner =
            gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(authority.to_bytes());
        p.market_token = m.meta.market_token_mint;
        p.collateral_token =
            gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(usdc.to_bytes());
        let mut pd = bytes(&*p, &GMTRADE_POSITION_DISCRIMINATOR);
        let mut pl = 1;
        assert!(
            market_and_position(&mi, &info(&key, &owner, &mut pl, &mut pd), authority, usdc)
                .is_ok()
        );
        let wrong = Pubkey::new_unique();
        assert!(market_and_position(
            &mi,
            &info(&wrong, &owner, &mut pl, &mut pd),
            authority,
            usdc
        )
        .is_err());
        p.kind = 1;
        pd = bytes(&*p, &GMTRADE_POSITION_DISCRIMINATOR);
        assert!(
            market_and_position(&mi, &info(&key, &owner, &mut pl, &mut pd), authority, usdc)
                .is_err()
        );
    }
    #[test]
    fn cancelled_pending_missing_and_wrong_nonce_never_prove_execution() {
        let authority = Pubkey::new_unique();
        let owner = address(GMTRADE_PROGRAM_ADDRESS).unwrap();
        let store = address(GMTRADE_STORE_ADDRESS).unwrap();
        let nonce = [9; 32];
        let (key, bump) = Pubkey::find_program_address(
            &[
                GMTRADE_ORDER_SEED,
                store.as_ref(),
                authority.as_ref(),
                &nonce,
            ],
            &owner,
        );
        let b = crate::state::OrderBaselineV1 {
            vault: Pubkey::new_unique(),
            order: key,
            position: Pubkey::new_unique(),
            nonce,
            refund: Pubkey::new_unique(),
            submitted_slot: 10,
            size_before: 0,
            collateral_before: 0,
            trade_id_before: 0,
            expected_size_delta: 100,
            bump: 1,
        };
        let mut o = bytemuck::allocation::zeroed_box::<Order>();
        o.header.bump = bump;
        o.header.action_state = 1;
        o.header.store =
            gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(store.to_bytes());
        o.header.market = gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(
            address(GMTRADE_XAU_MARKET_ADDRESS).unwrap().to_bytes(),
        );
        o.header.owner =
            gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(authority.to_bytes());
        o.header.nonce = nonce;
        o.header.updated_at_slot = 11;
        o.params.position =
            gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(b.position.to_bytes());
        o.params.kind = 3;
        o.params.side = 1;
        o.params.size_delta_value = 100;
        let mut l = 1;
        let mut data = bytes(&*o, &GMTRADE_ORDER_DISCRIMINATOR);
        assert!(executed_order(&info(&key, &owner, &mut l, &mut data), &b, authority, 12).is_ok());
        for state in [0, 2, 3, 255] {
            o.header.action_state = state;
            data = bytes(&*o, &GMTRADE_ORDER_DISCRIMINATOR);
            assert!(
                executed_order(&info(&key, &owner, &mut l, &mut data), &b, authority, 12).is_err()
            );
        }
        o.header.action_state = 1;
        o.header.nonce[0] ^= 1;
        data = bytes(&*o, &GMTRADE_ORDER_DISCRIMINATOR);
        assert!(executed_order(&info(&key, &owner, &mut l, &mut data), &b, authority, 12).is_err());
        data.clear();
        assert!(executed_order(&info(&key, &owner, &mut l, &mut data), &b, authority, 12).is_err());
    }
    #[test]
    fn protocol_usd_normalization_rounds_against_the_vault() {
        assert_eq!(usd_down(100_000_000_000_001).unwrap(), 1);
        assert_eq!(usd_up(100_000_000_000_001).unwrap(), 2);
        let p = PriceBounds {
            lower_usd: 1_000_000,
            upper_usd: 1_000_001,
            published_at: 0,
        };
        let result = model_price(p, 6).unwrap();
        assert_eq!(result.min, 100_000_000_000_000);
        assert_eq!(result.max, 100_000_100_000_000);
    }
}

#[cfg(test)]
mod valuation_tests {
    use super::*;
    use gmsol_model::BorrowingFeeMarketExt;
    fn market() -> Box<Market> {
        let mut m = bytemuck::allocation::zeroed_box::<Market>();
        bytemuck::bytes_of_mut(&mut *m)
            .copy_from_slice(&include_bytes!("../../tests/fixtures/gmtrade/xau_market.bin")[8..]);
        m.virtual_inventory_for_positions = Default::default();
        m.virtual_inventory_for_swaps = Default::default();
        m
    }
    fn setup() -> (Box<Market>, Box<Position>, i64) {
        let m = market();
        let now = m.state.clocks.borrowing.max(m.state.clocks.funding);
        let model = MarketModel::from_parts(Arc::from(market()), 0, now);
        let mut p = bytemuck::allocation::zeroed_box::<Position>();
        p.kind = 2;
        p.collateral_token = m.meta.short_token_mint;
        p.market_token = m.meta.market_token_mint;
        p.state.collateral_amount = 50_000_000;
        p.state.size_in_usd = 100 * 10u128.pow(20);
        p.state.size_in_tokens = 3_333_333;
        p.state.borrowing_factor = model.cumulative_borrowing_factor(false).unwrap();
        p.state.funding_fee_amount_per_size =
            model.funding_fee_amount_per_size(false, true).unwrap();
        p.state.long_token_claimable_funding_amount_per_size = model
            .claimable_funding_fee_amount_per_size(false, true)
            .unwrap();
        p.state.short_token_claimable_funding_amount_per_size = model
            .claimable_funding_fee_amount_per_size(false, false)
            .unwrap();
        (m, p, now)
    }
    fn price(value: u64) -> PriceBounds {
        PriceBounds {
            lower_usd: value,
            upper_usd: value,
            published_at: 0,
        }
    }
    #[test]
    fn short_profit_loss_and_close_costs_come_from_protocol_model() {
        let (m, p, now) = setup();
        let gain =
            value_position(m, p, price(2_000_000_000), price(1_000_000), 8, now, None).unwrap();
        assert!(gain.capped_pnl_usd > 0);
        assert_eq!(gain.borrowing_fee_usd, 0);
        assert_eq!(gain.funding_payable_usdc, 0);
        assert_eq!(gain.claimable_funding_usdc, 0);
        assert!(gain.close_fee_usd > 0);
        let (m, p, now) = setup();
        let loss =
            value_position(m, p, price(5_000_000_000), price(1_000_000), 8, now, None).unwrap();
        assert!(loss.capped_pnl_usd < 0);
        assert!(loss.equity(price(1_000_000)).unwrap() < 0);
    }
    #[test]
    fn protocol_fee_projection_uses_transaction_time_and_rejects_future_clocks() {
        let (m, p, now) = setup();
        assert!(value_position(
            m,
            p,
            price(3_000_000_000),
            price(1_000_000),
            8,
            now - 100,
            None
        )
        .is_err());
        let (m, p, now) = setup();
        let current =
            value_position(m, p, price(3_000_000_000), price(1_000_000), 8, now, None).unwrap();
        let (m, p, now) = setup();
        let later = value_position(
            m,
            p,
            price(3_000_000_000),
            price(1_000_000),
            8,
            now + 60,
            None,
        )
        .unwrap();
        assert!(later.borrowing_fee_usd >= current.borrowing_fee_usd);
    }
}
