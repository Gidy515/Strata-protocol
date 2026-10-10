//! Pure accounting kernel. NOT an authenticated Solana account adapter.
//! Every input must be derived from validated accounts in the same transaction.
//! Never expose these inputs as instruction arguments supplied by a user/keeper.
//! USD values use six decimals; assets/shares use their existing six decimals.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountingError {
    Overflow, InvalidPrice, StalePrice, InvalidObservation, Insolvent,
    InvalidBootstrap, InvalidAmount, MinimumOutput, InsufficientLiquidity,
    OutstandingOrder, OrderMismatch, UnprovenExecution, ResidualEscrow,
    Replay, PartialFinalRedemption,
}
type Result<T> = core::result::Result<T, AccountingError>;
const UNIT: u128 = 1_000_000;

#[derive(Debug, Clone, Copy)]
pub struct PriceBounds {
    pub lower_usd: u64,
    pub upper_usd: u64,
    pub published_at: i64,
}
impl PriceBounds {
    fn validate(self, now: i64, max_age: u64) -> Result<()> {
        if self.lower_usd == 0 || self.upper_usd < self.lower_usd {
            return Err(AccountingError::InvalidPrice);
        }
        if max_age == 0 || self.published_at < 0 || now < self.published_at {
            return Err(AccountingError::StalePrice);
        }
        if (now - self.published_at) as u64 > max_age {
            return Err(AccountingError::StalePrice);
        }
        Ok(())
    }
}
fn floor_ratio(value: u128, numerator: u128, denominator: u128) -> Result<u128> {
    if denominator == 0 { return Err(AccountingError::InvalidPrice); }
    value.checked_mul(numerator).map(|v| v / denominator)
        .ok_or(AccountingError::Overflow)
}
fn ceil_ratio(value: u128, numerator: u128, denominator: u128) -> Result<u128> {
    if denominator == 0 { return Err(AccountingError::InvalidPrice); }
    let product = value.checked_mul(numerator).ok_or(AccountingError::Overflow)?;
    (product / denominator).checked_add(u128::from(product % denominator != 0))
        .ok_or(AccountingError::Overflow)
}
fn signed(value: u128) -> Result<i128> {
    i128::try_from(value).map_err(|_| AccountingError::Overflow)
}
fn narrow(value: u128) -> Result<u64> {
    u64::try_from(value).map_err(|_| AccountingError::Overflow)
}

/// Protocol adapter output, not raw position fields. Positive PnL must already
/// include protocol profit caps; losses must remain signed. Funding receivable
/// includes only amounts attributable to this vault and excludes amounts already
/// present in either custody account. Fees/impact are not subtracted a second time.
#[derive(Debug, Clone, Copy, Default)]
pub struct PositionValuation {
    pub collateral_usdc: u64,
    pub capped_pnl_usd: i128,
    pub claimable_funding_usdc: u64,
    pub borrowing_fee_usd: u64,
    pub funding_payable_usdc: u64,
    pub close_fee_usd: u64,
    pub adverse_close_impact_usd: u64,
}
impl PositionValuation {
    pub fn equity(self, usdc: PriceBounds) -> Result<i128> {
        if usdc.lower_usd == 0 || usdc.upper_usd < usdc.lower_usd {
            return Err(AccountingError::InvalidPrice);
        }
        let mut value = i128::from(self.collateral_usdc)
            .checked_add(i128::from(self.claimable_funding_usdc))
            .ok_or(AccountingError::Overflow)?;
        let pnl = if self.capped_pnl_usd >= 0 {
            signed(floor_ratio(self.capped_pnl_usd as u128, UNIT, usdc.upper_usd as u128)?)?
        } else {
            let loss = signed(ceil_ratio(self.capped_pnl_usd.unsigned_abs(), UNIT, usdc.lower_usd as u128)?)?;
            loss.checked_neg().ok_or(AccountingError::Overflow)?
        };
        value = value.checked_add(pnl).ok_or(AccountingError::Overflow)?;
        for fee in [self.borrowing_fee_usd, self.close_fee_usd, self.adverse_close_impact_usd] {
            let cost = signed(ceil_ratio(fee as u128, UNIT, usdc.lower_usd as u128)?)?;
            value = value.checked_sub(cost).ok_or(AccountingError::Overflow)?;
        }
        value.checked_sub(i128::from(self.funding_payable_usdc))
            .ok_or(AccountingError::Overflow)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NavInput {
    pub custody_usdc: u64,
    pub strategy_usdc: u64,
    pub gold_amount: u64,
    pub pending_escrow_usdc: u64,
    pub position: PositionValuation,
    pub additional_liabilities_usdc: u64,
    pub observed_slot: u64,
    pub position_active: bool,
    pub pending_order_present: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavQuote {
    pub net_assets: u64,
    pub available_usdc: u64,
    pub gold_value_usdc: u64,
    pub perpetual_equity_usdc: i128,
    pub observed_slot: u64,
    pub noncustody_assets_present: bool,
}
pub fn full_nav(input: NavInput, gold: PriceBounds, usdc: PriceBounds,
    now: i64, max_age: u64, current_slot: u64) -> Result<NavQuote> {
    // No stale cached total: every quote must use current account state.
    if input.observed_slot != current_slot { return Err(AccountingError::InvalidObservation); }
    gold.validate(now, max_age)?;
    usdc.validate(now, max_age)?;
    let gold_value = narrow(floor_ratio(input.gold_amount as u128,
        gold.lower_usd as u128, usdc.upper_usd as u128)?)?;
    let equity = input.position.equity(usdc)?;
    let mut net = i128::from(input.custody_usdc);
    for balance in [input.strategy_usdc, gold_value, input.pending_escrow_usdc] {
        net = net.checked_add(i128::from(balance)).ok_or(AccountingError::Overflow)?;
    }
    net = net.checked_add(equity).and_then(|v|
        v.checked_sub(i128::from(input.additional_liabilities_usdc)))
        .ok_or(AccountingError::Overflow)?;
    if net < 0 { return Err(AccountingError::Insolvent); }
    Ok(NavQuote {
        net_assets: narrow(net as u128)?, available_usdc: input.custody_usdc,
        gold_value_usdc: gold_value, perpetual_equity_usdc: equity,
        observed_slot: current_slot,
        noncustody_assets_present: input.strategy_usdc != 0 || input.gold_amount != 0
            || input.pending_escrow_usdc != 0 || equity != 0
            || input.additional_liabilities_usdc != 0 || input.position_active
            || input.pending_order_present || input.position.collateral_usdc != 0
            || input.position.capped_pnl_usd != 0 || input.position.claimable_funding_usdc != 0
            || input.position.borrowing_fee_usd != 0 || input.position.funding_payable_usdc != 0
            || input.position.close_fee_usd != 0 || input.position.adverse_close_impact_usd != 0,
    })
}

pub fn deposit_quote(amount: u64, supply: u64, nav: NavQuote, minimum: u64) -> Result<u64> {
    if amount == 0 || minimum == 0 { return Err(AccountingError::InvalidAmount); }
    let shares = if supply == 0 {
        // Zero net assets can conceal mutually offsetting positions/liabilities.
        if nav.net_assets != 0 || nav.noncustody_assets_present {
            return Err(AccountingError::InvalidBootstrap);
        }
        amount
    } else {
        if nav.net_assets == 0 { return Err(AccountingError::Insolvent); }
        narrow(floor_ratio(amount as u128, supply as u128, nav.net_assets as u128)?)?
    };
    if shares == 0 || shares < minimum { return Err(AccountingError::MinimumOutput); }
    Ok(shares)
}
pub fn redemption_quote(shares: u64, supply: u64, nav: NavQuote, minimum: u64) -> Result<u64> {
    if shares == 0 || minimum == 0 || shares > supply { return Err(AccountingError::InvalidAmount); }
    if nav.net_assets == 0 { return Err(AccountingError::Insolvent); }
    let assets = narrow(floor_ratio(shares as u128, nav.net_assets as u128, supply as u128)?)?;
    if assets == 0 || assets < minimum { return Err(AccountingError::MinimumOutput); }
    if assets > nav.available_usdc { return Err(AccountingError::InsufficientLiquidity); }
    // Burning every share while residual assets/liabilities remain creates an
    // ownerless portfolio. The last redemption must wait for strategy unwind.
    if shares == supply && nav.noncustody_assets_present {
        return Err(AccountingError::PartialFinalRedemption);
    }
    Ok(assets)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderStatus { Pending, Frozen, Executed, Cancelled, Missing }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingOrder {
    pub order: [u8; 32], pub position: [u8; 32], pub nonce: [u8; 32],
    pub submitted_slot: u64, pub size_before: u128, pub expected_size_delta: u128,
}
#[derive(Debug, Clone, Copy)]
pub struct OrderEvidence {
    pub order: [u8; 32], pub position: [u8; 32], pub nonce: [u8; 32],
    pub status: OrderStatus, pub execution_slot: u64, pub observed_slot: u64,
    pub size_after: u128, pub remaining_escrow: u64,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct OrderTracker { pub pending: Option<PendingOrder> }
impl OrderTracker {
    pub fn submit(&mut self, order: PendingOrder) -> Result<()> {
        if self.pending.is_some() { return Err(AccountingError::OutstandingOrder); }
        if order.expected_size_delta == 0 { return Err(AccountingError::InvalidAmount); }
        self.pending = Some(order);
        Ok(())
    }
    /// Called only after the adapter proves owner, discriminator, canonical PDA,
    /// store, strategy authority, market, collateral mint and successful status.
    /// Missing order data or empty escrow is NEVER evidence of execution.
    pub fn reconcile(&mut self, evidence: OrderEvidence, current_slot: u64) -> Result<()> {
        let pending = self.pending.ok_or(AccountingError::Replay)?;
        if pending.order != evidence.order || pending.position != evidence.position
            || pending.nonce != evidence.nonce { return Err(AccountingError::OrderMismatch); }
        if evidence.observed_slot != current_slot || evidence.execution_slot < pending.submitted_slot
            || evidence.execution_slot > current_slot { return Err(AccountingError::InvalidObservation); }
        if evidence.status != OrderStatus::Executed { return Err(AccountingError::UnprovenExecution); }
        if evidence.remaining_escrow != 0 { return Err(AccountingError::ResidualEscrow); }
        let expected = pending.size_before.checked_add(pending.expected_size_delta)
            .ok_or(AccountingError::Overflow)?;
        if evidence.size_after != expected { return Err(AccountingError::UnprovenExecution); }
        self.pending = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const U: u64 = 1_000_000;
    fn price(n: u64) -> PriceBounds { PriceBounds { lower_usd: n, upper_usd: n, published_at: 100 } }
    fn input() -> NavInput { NavInput { custody_usdc: 100*U, strategy_usdc: 0,
        gold_amount: 0, pending_escrow_usdc: 0, position: Default::default(),
        additional_liabilities_usdc: 0, observed_slot: 10, position_active:false, pending_order_present:false } }
    fn quote(i: NavInput) -> NavQuote { full_nav(i, price(3000*U), price(U), 100, 30, 10).unwrap() }
    fn pending() -> PendingOrder { PendingOrder { order: [1;32], position: [2;32], nonce:[3;32],
        submitted_slot: 5, size_before: 100, expected_size_delta: 50 } }
    fn evidence() -> OrderEvidence { OrderEvidence { order:[1;32], position:[2;32], nonce:[3;32],
        status:OrderStatus::Executed, execution_slot:8, observed_slot:10,
        size_after:150, remaining_escrow:0 } }
    #[test] fn collateral_move_does_not_create_profit() {
        let baseline = quote(input()).net_assets;
        let mut i=input(); i.custody_usdc-=40*U; i.pending_escrow_usdc=40*U;
        assert_eq!(quote(i).net_assets,baseline);
        i.pending_escrow_usdc=0; i.position.collateral_usdc=40*U;
        assert_eq!(quote(i).net_assets,baseline);
    }
    #[test] fn funding_and_fees_count_once() {
        let mut i=input(); i.position.collateral_usdc=20*U;
        i.position.claimable_funding_usdc=3*U; i.position.capped_pnl_usd=-5*i128::from(U);
        i.position.borrowing_fee_usd=U; i.position.funding_payable_usdc=2*U;
        i.position.close_fee_usd=U; i.position.adverse_close_impact_usd=U;
        i.additional_liabilities_usdc=U;
        assert_eq!(quote(i).net_assets,112*U);
    }
    #[test] fn negative_equity_not_clamped() {
        let mut i=input(); i.position.capped_pnl_usd=-20*i128::from(U);
        assert_eq!(quote(i).net_assets,80*U);
        i.position.capped_pnl_usd=-101*i128::from(U);
        assert_eq!(full_nav(i,price(U),price(U),100,30,10),Err(AccountingError::Insolvent));
    }
    #[test] fn signed_conversion_rounds_outward() {
        let usd=PriceBounds { lower_usd:999_000, upper_usd:1_001_000, published_at:100 };
        let mut p=PositionValuation { capped_pnl_usd:1, ..Default::default() };
        assert_eq!(p.equity(usd).unwrap(),0);
        p.capped_pnl_usd=-1; assert_eq!(p.equity(usd).unwrap(),-2);
    }
    #[test] fn stale_future_and_missing_prices_fail_even_without_gold() {
        for now in [99,131] { assert!(full_nav(input(),price(U),price(U),now,30,10).is_err()); }
        assert!(full_nav(input(),price(0),price(U),100,30,10).is_err());
        assert!(full_nav(input(),price(U),price(U),100,30,11).is_err());
        assert!(full_nav(input(),price(U),price(U),100,0,10).is_err());
    }
    #[test] fn gold_and_usdc_confidence_use_opposite_bounds() {
        let mut i=input(); i.gold_amount=U;
        let gold=PriceBounds{lower_usd:2990*U,upper_usd:3010*U,published_at:100};
        let usdc=PriceBounds{lower_usd:990_000,upper_usd:1_010_000,published_at:100};
        let q=full_nav(i,gold,usdc,100,30,10).unwrap();
        assert_eq!(q.gold_value_usdc,2_960_396_039);
    }
    #[test] fn invested_assets_affect_share_price_not_cash_availability() {
        let mut i=input(); i.gold_amount=10_000;
        let q=quote(i); assert_eq!(q.net_assets,130*U);
        assert_eq!(deposit_quote(13*U,100*U,q,10*U).unwrap(),10*U);
        assert_eq!(redemption_quote(10*U,100*U,q,13*U).unwrap(),13*U);
        assert_eq!(redemption_quote(90*U,100*U,q,1),Err(AccountingError::InsufficientLiquidity));
    }
    #[test] fn zero_supply_cannot_capture_offsetting_positions() {
        let mut i=input(); i.custody_usdc=0; i.strategy_usdc=U;
        i.additional_liabilities_usdc=U;
        assert_eq!(deposit_quote(U,0,quote(i),1),Err(AccountingError::InvalidBootstrap));
        i.strategy_usdc=0; i.additional_liabilities_usdc=0;
        assert_eq!(deposit_quote(U,0,quote(i),1).unwrap(),U);
    }
    #[test] fn final_redemption_requires_unwind_even_if_cash_is_sufficient() {
        let mut i=input(); i.strategy_usdc=U; i.additional_liabilities_usdc=U;
        assert_eq!(redemption_quote(100*U,100*U,quote(i),1),Err(AccountingError::PartialFinalRedemption));
        assert_eq!(redemption_quote(100*U,100*U,quote(input()),1).unwrap(),100*U);
    }
    #[test] fn rounding_cannot_make_deposit_redemption_profit() {
        for supply in 1..20 { for assets in 1..20 { for amount in 1..20 {
            let q=NavQuote { net_assets:assets, available_usdc:assets, gold_value_usdc:0,
                perpetual_equity_usdc:0,observed_slot:10,noncustody_assets_present:false };
            if let Ok(shares)=deposit_quote(amount,supply,q,1) {
                let after=NavQuote {net_assets:assets+amount,available_usdc:assets+amount,..q};
                if let Ok(returned)=redemption_quote(shares,supply+shares,after,1) {
                    assert!(returned<=amount);
                }
            }
        }}}
    }
    #[test] fn all_nonexecuted_states_preserve_pending() {
        for status in [OrderStatus::Pending,OrderStatus::Frozen,OrderStatus::Cancelled,OrderStatus::Missing] {
            let mut t=OrderTracker::default(); t.submit(pending()).unwrap();
            let e=OrderEvidence {status,..evidence()};
            assert_eq!(t.reconcile(e,10),Err(AccountingError::UnprovenExecution));
            assert_eq!(t.pending,Some(pending()));
        }
    }
    #[test] fn execution_reconciliation_is_exact_and_not_replayable() {
        let mut t=OrderTracker::default();t.submit(pending()).unwrap();
        assert_eq!(t.submit(pending()),Err(AccountingError::OutstandingOrder));
        t.reconcile(evidence(),10).unwrap(); assert!(t.pending.is_none());
        assert_eq!(t.reconcile(evidence(),10),Err(AccountingError::Replay));
    }
    #[test] fn wrong_identity_size_slot_and_residual_escrow_rejected() {
        let mut variants=vec![];
        variants.push(OrderEvidence {order:[9;32],..evidence()});
        variants.push(OrderEvidence {position:[9;32],..evidence()});
        variants.push(OrderEvidence {nonce:[9;32],..evidence()});
        variants.push(OrderEvidence {size_after:149,..evidence()});
        variants.push(OrderEvidence {remaining_escrow:1,..evidence()});
        variants.push(OrderEvidence {execution_slot:4,..evidence()});
        variants.push(OrderEvidence {execution_slot:11,..evidence()});
        variants.push(OrderEvidence {observed_slot:9,..evidence()});
        for e in variants { let mut t=OrderTracker::default();t.submit(pending()).unwrap();
            assert!(t.reconcile(e,10).is_err());assert_eq!(t.pending,Some(pending())); }
    }
    #[test] fn zero_equity_active_position_and_empty_pending_order_block_final_exit() {
        for (active,pending) in [(true,false),(false,true)] {
            let mut i=input();i.position_active=active;i.pending_order_present=pending;
            assert_eq!(redemption_quote(100*U,100*U,quote(i),1),Err(AccountingError::PartialFinalRedemption));
            i.custody_usdc=0;
            assert_eq!(deposit_quote(U,0,quote(i),1),Err(AccountingError::InvalidBootstrap));
        }
    }
    #[test] fn overflow_rejected() {
        let mut i=input();i.custody_usdc=u64::MAX;i.strategy_usdc=1;
        assert!(full_nav(i,price(U),price(U),100,30,10).is_err());
        let p=PositionValuation {capped_pnl_usd:i128::MIN,..Default::default()};
        assert!(p.equity(price(1)).is_err());
    }
}
