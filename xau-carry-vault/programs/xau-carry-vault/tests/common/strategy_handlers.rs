//! LiteSVM tests execute Vault handlers with authenticated synthetic oracle updates.
//! GMTrade market layout uses a captured public account; no real trades are sent.
use super::*;
use anchor_lang::prelude::Clock;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::Space;
use xau_carry_vault::{
    constants::*,
    state::{StrategyV1State, VaultV1State},
};

fn address(text: &str) -> Pubkey {
    text.parse().unwrap()
}
fn set_account(f: &mut Fixture, key: Pubkey, owner: Pubkey, data: Vec<u8>) {
    f.svm
        .set_account(
            key,
            Account {
                lamports: f.svm.minimum_balance_for_rent_exemption(data.len()).max(1),
                data,
                owner,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
}
fn set_token(
    f: &mut Fixture,
    key: Pubkey,
    owner: Pubkey,
    mint: Pubkey,
    amount: u64,
    program: Pubkey,
) {
    let mut data = vec![0; SplTokenAccount::LEN];
    SplTokenAccount::pack(
        SplTokenAccount {
            mint,
            owner,
            amount,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        },
        &mut data,
    )
    .unwrap();
    set_account(f, key, program, data);
}
fn set_price(f: &mut Fixture, key: Pubkey, feed: &str, value: i64, time: i64) {
    let mut d = vec![0; 134];
    d[..8].copy_from_slice(&[34, 241, 35, 99, 157, 126, 244, 205]);
    d[40] = 1;
    for i in 0..32 {
        d[41 + i] = u8::from_str_radix(&feed[2 * i..2 * i + 2], 16).unwrap();
    }
    d[73..81].copy_from_slice(&value.to_le_bytes());
    d[89..93].copy_from_slice(&(-6i32).to_le_bytes());
    d[93..101].copy_from_slice(&time.to_le_bytes());
    d[101..109].copy_from_slice(&time.to_le_bytes());
    d[125..133].copy_from_slice(&100u64.to_le_bytes());
    set_account(
        f,
        key,
        address("rec2HHDDnjLfj4kE7VyEtFA1HPGQLK33259532cRyHp"),
        d,
    );
}
fn setup(gold_amount: u64, strategy_usdc: u64) -> (Fixture, Vec<AccountMeta>) {
    let mut f = Fixture::new_with_mint(6, address(MAINNET_USDC_MINT));
    f.initialize_as_admin();
    f.deposit(10_000_000).unwrap();
    let authority = Pubkey::find_program_address(
        &[STRATEGY_AUTHORITY_SEED, f.vault_state.as_ref()],
        &f.program_id,
    )
    .0;
    f.svm.airdrop(&authority, 1_000_000).unwrap();
    let source = get_associated_token_address(&authority, &f.usdc_mint);
    let usdc = f.usdc_mint;
    set_token(
        &mut f,
        source,
        authority,
        usdc,
        strategy_usdc,
        spl_token::ID,
    );
    let gold_mint = address(PAXG_MINT_ADDRESS);
    let mut mint = vec![0; SplMint::LEN];
    SplMint::pack(
        SplMint {
            mint_authority: COption::None,
            supply: gold_amount,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut mint,
    )
    .unwrap();
    set_account(&mut f, gold_mint, anchor_spl::token_2022::ID, mint);
    let gold =
        Pubkey::find_program_address(&[GOLD_VAULT_SEED, f.vault_state.as_ref()], &f.program_id).0;
    let vault = f.vault_state;
    set_token(
        &mut f,
        gold,
        vault,
        gold_mint,
        gold_amount,
        anchor_spl::token_2022::ID,
    );
    let gm = address(GMTRADE_PROGRAM_ADDRESS);
    let market = address(GMTRADE_XAU_MARKET_ADDRESS);
    let md = include_bytes!("../fixtures/gmtrade/xau_market.bin").to_vec();
    let m: Box<gmsol_programs::gmsol_store::accounts::Market> = {
        let mut m = bytemuck::allocation::zeroed_box();
        bytemuck::bytes_of_mut(&mut *m).copy_from_slice(&md[8..]);
        m
    };
    let timestamp = m
        .state
        .clocks
        .funding
        .max(m.state.clocks.borrowing)
        .max(m.state.clocks.price_impact_distribution)
        + 10;
    let clock = Clock {
        slot: 455_040_000,
        unix_timestamp: timestamp,
        ..Clock::default()
    };
    f.svm.set_sysvar(&clock);
    let index = address(GMTRADE_XAU_INDEX_ADDRESS);
    let store = address(GMTRADE_STORE_ADDRESS);
    let position = Pubkey::find_program_address(
        &[
            GMTRADE_POSITION_SEED,
            store.as_ref(),
            authority.as_ref(),
            m.meta.market_token_mint.as_ref(),
            usdc.as_ref(),
            &[2],
        ],
        &gm,
    )
    .0;
    // Missing canonical position is supported before preparation / after complete unwind.
    let vi = if m.virtual_inventory_for_positions == Default::default() {
        gm
    } else {
        use gmsol_programs::anchor_lang::Discriminator;
        let v = bytemuck::allocation::zeroed_box::<
            gmsol_programs::gmsol_store::accounts::VirtualInventory,
        >();
        let mut d = gmsol_programs::gmsol_store::accounts::VirtualInventory::DISCRIMINATOR.to_vec();
        d.extend_from_slice(bytemuck::bytes_of(&*v));
        let key = Pubkey::new_from_array(m.virtual_inventory_for_positions.to_bytes());
        set_account(&mut f, key, gm, d);
        key
    };
    set_account(&mut f, market, gm, md);
    set_account(
        &mut f,
        index,
        spl_token::ID,
        include_bytes!("../fixtures/gmtrade/xau_index_mint.bin").to_vec(),
    );
    let config = StrategyV1State {
        vault,
        gold_mint,
        gold_custody: gold,
        gold_token_program: anchor_spl::token_2022::ID,
        perpetual_program: gm,
        perpetual_store: store,
        perpetual_market: market,
        configured_at_slot: 1,
        execution_enabled: false,
        bump: Pubkey::find_program_address(&[STRATEGY_SEED, vault.as_ref()], &f.program_id).1,
    };
    let mut data = vec![];
    config.try_serialize(&mut data).unwrap();
    data.resize(8 + StrategyV1State::INIT_SPACE, 0);
    let strategy = f.strategy_address();
    let prog = f.program_id;
    set_account(&mut f, strategy, prog, data);
    let paxg = Pubkey::new_unique();
    let usdc_price = Pubkey::new_unique();
    let xau = Pubkey::new_unique();
    set_price(
        &mut f,
        paxg,
        "273717b49430906f4b0c230e99aa1007f83758e3199edbc887c0d06c3e332494",
        3_000_000_000,
        timestamp,
    );
    set_price(
        &mut f,
        usdc_price,
        "eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a",
        1_000_000,
        timestamp,
    );
    set_price(
        &mut f,
        xau,
        xau_carry_vault::strategy_nav::XAU_FEED,
        3_000_000_000,
        timestamp,
    );
    let pending =
        Pubkey::find_program_address(&[PENDING_SHORT_ORDER_SEED, vault.as_ref()], &prog).0;
    let extra = [
        authority, source, gold_mint, gold, pending, gm, gm, market, position, vi, index, paxg,
        usdc_price, xau,
    ]
    .into_iter()
    .map(|key| AccountMeta::new_readonly(key, false))
    .collect();
    (f, extra)
}
fn deposit(
    f: &mut Fixture,
    extras: &[AccountMeta],
    amount: u64,
    minimum: u64,
) -> Result<(), String> {
    let mut accounts = xau_carry_vault::accounts::DepositUsdcV1 {
        user: f.user.pubkey(),
        vault_v1_state: f.vault_state,
        usdc_mint: f.usdc_mint,
        vault_v1_usdc_account: f.vault_usdc,
        vxau_mint: f.vxau_mint,
        user_usdc_ata: f.user_usdc,
        user_vxau_ata: f.user_vxau,
        strategy_account: f.strategy_address(),
        token_program: spl_token::ID,
        associated_token_program: anchor_spl::associated_token::ID,
        system_program: system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend_from_slice(extras);
    f.run_lifecycle_instruction(
        Instruction {
            program_id: f.program_id,
            accounts,
            data: xau_carry_vault::instruction::DepositUsdcV1 {
                amount,
                min_shares_out: minimum,
            }
            .data(),
        },
        false,
    )
}
fn withdraw(
    f: &mut Fixture,
    extras: &[AccountMeta],
    shares: u64,
    minimum: u64,
) -> Result<(), String> {
    let mut accounts = xau_carry_vault::accounts::WithdrawUsdcV1 {
        user: f.user.pubkey(),
        vault_v1_state: f.vault_state,
        usdc_mint: f.usdc_mint,
        vault_v1_usdc_account: f.vault_usdc,
        vxau_mint: f.vxau_mint,
        user_usdc_ata: f.user_usdc,
        user_vxau_ata: f.user_vxau,
        strategy_account: f.strategy_address(),
        token_program: spl_token::ID,
        associated_token_program: anchor_spl::associated_token::ID,
        system_program: system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend_from_slice(extras);
    f.run_lifecycle_instruction(
        Instruction {
            program_id: f.program_id,
            accounts,
            data: xau_carry_vault::instruction::WithdrawUsdcV1 {
                amount: shares,
                min_assets_out: minimum,
            }
            .data(),
        },
        false,
    )
}
fn settle(f: &mut Fixture, extras: &[AccountMeta], nonce: u64) -> Result<(), String> {
    let (request, escrow) = f.queue_addresses(nonce);
    let mut accounts = xau_carry_vault::accounts::CompleteWithdrawalV1 {
        payer: f.admin.pubkey(),
        owner: f.user.pubkey(),
        vault_v1_state: f.vault_state,
        usdc_mint: f.usdc_mint,
        vxau_mint: f.vxau_mint,
        vault_v1_usdc_account: f.vault_usdc,
        withdrawal_request: request,
        withdrawal_escrow: escrow,
        owner_usdc_ata: f.user_usdc,
        owner_vxau_ata: f.user_vxau,
        strategy_account: f.strategy_address(),
        token_program: spl_token::ID,
        associated_token_program: anchor_spl::associated_token::ID,
        system_program: system_program::ID,
    }
    .to_account_metas(None);
    accounts.extend_from_slice(extras);
    f.run_lifecycle_instruction(
        Instruction {
            program_id: f.program_id,
            accounts,
            data: xau_carry_vault::instruction::SettleWithdrawalV1 {}.data(),
        },
        true,
    )
}
#[test]
fn authenticated_gold_and_strategy_cash_change_deposit_share_price() {
    let (mut f, extra) = setup(1_000, 2_000_000); // 10 custody + 3 gold + 2 strategy = 15 NAV
    deposit(&mut f, &extra, 3_000_000, 2_000_000).unwrap();
    assert_eq!(f.token(f.user_vxau).amount, 12_000_000);
    assert_eq!(f.token(f.vault_usdc).amount, 13_000_000);
}
#[test]
fn strategy_redemption_and_queue_use_full_nav() {
    let (mut f, extra) = setup(1_000, 2_000_000);
    withdraw(&mut f, &extra, 2_000_000, 3_000_000).unwrap();
    assert_eq!(f.token(f.vault_usdc).amount, 7_000_000);
    f.queue_withdrawal(500, 2_000_000, 3_000_000).unwrap();
    settle(&mut f, &extra, 500).unwrap();
    assert_eq!(f.token(f.vault_usdc).amount, 4_000_000);
    assert_eq!(f.mint(f.vxau_mint).supply, 6_000_000);
}
#[test]
fn full_nav_does_not_turn_invested_assets_into_withdrawal_liquidity() {
    let (mut f, extra) = setup(100_000, 0);
    let shares = f.mint(f.vxau_mint).supply;
    assert!(withdraw(&mut f, &extra, shares / 2, 1).is_err());
    assert_eq!(f.mint(f.vxau_mint).supply, shares);
    assert_eq!(f.token(f.vault_usdc).amount, 10_000_000);
}
#[test]
fn final_share_burn_requires_all_strategy_assets_unwound() {
    let (mut f, extra) = setup(0, 0);
    let source = extra[1].pubkey;
    // An offsetting position is covered by unit tests; here one residual USDC still blocks last exit.
    let authority = extra[0].pubkey;
    let mint = f.usdc_mint;
    set_token(&mut f, source, authority, mint, 1, spl_token::ID);
    let shares = f.mint(f.vxau_mint).supply;
    assert!(withdraw(&mut f, &extra, shares, 1).is_err());
    set_token(&mut f, source, authority, mint, 0, spl_token::ID);
    withdraw(&mut f, &extra, shares, 10_000_000).unwrap();
    assert_eq!(f.mint(f.vxau_mint).supply, 0);
}
#[test]
fn stale_wrong_feed_or_wrong_gold_custody_roll_back_deposit() {
    for mode in 0..3 {
        let (mut f, mut extra) = setup(1_000, 0);
        let shares = f.mint(f.vxau_mint).supply;
        let user = f.token(f.user_usdc).amount;
        if mode == 0 {
            let clock = f.svm.get_sysvar::<Clock>();
            set_price(
                &mut f,
                extra[11].pubkey,
                "273717b49430906f4b0c230e99aa1007f83758e3199edbc887c0d06c3e332494",
                3_000_000_000,
                clock.unix_timestamp - 31,
            );
        } else if mode == 1 {
            let clock = f.svm.get_sysvar::<Clock>();
            set_price(
                &mut f,
                extra[11].pubkey,
                xau_carry_vault::strategy_nav::XAU_FEED,
                3_000_000_000,
                clock.unix_timestamp,
            );
        } else {
            extra[3].pubkey = extra[1].pubkey;
        }
        assert!(deposit(&mut f, &extra, 1_000_000, 1).is_err());
        assert_eq!(f.mint(f.vxau_mint).supply, shares);
        assert_eq!(f.token(f.user_usdc).amount, user);
    }
}

#[test]
fn live_position_pnl_changes_handler_pricing_without_becoming_cash() {
    use gmsol_model::{BorrowingFeeMarketExt, PerpMarketExt};
    use gmsol_programs::{
        gmsol_store::accounts::{Market, Position},
        model::MarketModel,
    };
    use std::sync::Arc;
    let (mut f, extra) = setup(0, 0);
    let authority = extra[0].pubkey;
    let market_data = f.svm.get_account(&extra[7].pubkey).unwrap().data;
    let mut m = bytemuck::allocation::zeroed_box::<Market>();
    bytemuck::bytes_of_mut(&mut *m).copy_from_slice(&market_data[8..]);
    let model =
        MarketModel::from_parts(Arc::from(m), 0, f.svm.get_sysvar::<Clock>().unix_timestamp);
    let mut p = bytemuck::allocation::zeroed_box::<Position>();
    let store = address(GMTRADE_STORE_ADDRESS);
    let gm = address(GMTRADE_PROGRAM_ADDRESS);
    p.kind = 2;
    p.bump = Pubkey::find_program_address(
        &[
            GMTRADE_POSITION_SEED,
            store.as_ref(),
            authority.as_ref(),
            model.meta.market_token_mint.as_ref(),
            f.usdc_mint.as_ref(),
            &[2],
        ],
        &gm,
    )
    .1;
    p.store = gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(store.to_bytes());
    p.owner = gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(authority.to_bytes());
    p.market_token = model.meta.market_token_mint;
    p.collateral_token =
        gmsol_programs::anchor_lang::prelude::Pubkey::new_from_array(f.usdc_mint.to_bytes());
    p.state.size_in_usd = 100 * 10u128.pow(20);
    p.state.size_in_tokens = 3_333_333;
    p.state.collateral_amount = 50_000_000;
    p.state.borrowing_factor = model.cumulative_borrowing_factor(false).unwrap();
    p.state.funding_fee_amount_per_size = model.funding_fee_amount_per_size(false, true).unwrap();
    p.state.long_token_claimable_funding_amount_per_size = model
        .claimable_funding_fee_amount_per_size(false, true)
        .unwrap();
    p.state.short_token_claimable_funding_amount_per_size = model
        .claimable_funding_fee_amount_per_size(false, false)
        .unwrap();
    let mut d = GMTRADE_POSITION_DISCRIMINATOR.to_vec();
    d.extend_from_slice(bytemuck::bytes_of(&*p));
    set_account(&mut f, extra[8].pubkey, gm, d);
    let before = f.token(f.user_vxau).amount;
    deposit(&mut f, &extra, 6_000_000, 1).unwrap();
    let minted = f.token(f.user_vxau).amount - before;
    // Idle-only pricing would mint six million shares. ~50 USDC short equity reduces it to ~one million.
    assert!(minted > 900_000 && minted < 1_100_000, "minted {minted}");
    let clock = f.svm.get_sysvar::<Clock>();
    set_price(
        &mut f,
        extra[13].pubkey,
        xau_carry_vault::strategy_nav::XAU_FEED,
        5_000_000_000,
        clock.unix_timestamp,
    );
    // Price increase produces a loss that exceeds short collateral; total NAV is negative.
    let supply = f.mint(f.vxau_mint).supply;
    assert!(deposit(&mut f, &extra, 1_000_000, 1).is_err());
    assert_eq!(f.mint(f.vxau_mint).supply, supply);
}
#[test]
fn usdc_depeg_is_applied_to_gold_in_handler_nav() {
    let (mut f, extra) = setup(1_000, 0);
    let clock = f.svm.get_sysvar::<Clock>();
    set_price(
        &mut f,
        extra[12].pubkey,
        "eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a",
        500_000,
        clock.unix_timestamp,
    ); // 3 USD gold = 6 USDC; total 16 USDC NAV.
    deposit(&mut f, &extra, 1_600_000, 1_000_000).unwrap();
    assert_eq!(f.mint(f.vxau_mint).supply, 11_000_000);
}

#[test]
fn execution_enable_needs_authenticated_nav_but_disable_needs_no_oracle() {
    let (mut f, extra) = setup(0, 0);
    let build = |f: &Fixture, enabled: bool, remaining: &[AccountMeta]| {
        let mut accounts = xau_carry_vault::accounts::SetStrategyExecutionV1 {
            admin: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
            strategy_v1_state: f.strategy_address(),
            usdc_mint: f.usdc_mint,
            vault_v1_usdc_account: f.vault_usdc,
        }
        .to_account_metas(None);
        accounts.extend_from_slice(remaining);
        Instruction {
            program_id: f.program_id,
            accounts,
            data: xau_carry_vault::instruction::SetStrategyExecutionV1 { enabled }.data(),
        }
    };
    let ix = build(&f, true, &[]);
    assert!(f.run_lifecycle_instruction(ix, true).is_err());
    let ix = build(&f, true, &extra);
    f.run_lifecycle_instruction(ix, true).unwrap();
    let state = f.svm.get_account(&f.strategy_address()).unwrap();
    assert!(
        StrategyV1State::try_deserialize(&mut state.data.as_slice())
            .unwrap()
            .execution_enabled
    );
    let ix = build(&f, false, &[]);
    f.run_lifecycle_instruction(ix, true).unwrap();
    let state = f.svm.get_account(&f.strategy_address()).unwrap();
    assert!(
        !StrategyV1State::try_deserialize(&mut state.data.as_slice())
            .unwrap()
            .execution_enabled
    );
}
#[test]
fn funded_vault_can_configure_without_changing_receipt_backing() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let mut f = Fixture::new_with_mint(6, address(MAINNET_USDC_MINT));
    f.initialize_as_admin();
    f.deposit(10_000_000).unwrap();
    let gm = address(GMTRADE_PROGRAM_ADDRESS);
    let market = address(GMTRADE_XAU_MARKET_ADDRESS);
    let store = address(GMTRADE_STORE_ADDRESS);
    // Configure checks executable ID and public store header, without invoking GMTrade.
    f.svm
        .add_program(
            gm,
            &std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures/gmtrade/gmtrade.so"),
            )
            .unwrap(),
        )
        .unwrap();
    let snapshot: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/gmtrade/store.json")).unwrap();
    set_account(
        &mut f,
        store,
        gm,
        STANDARD
            .decode(snapshot["account"]["data"][0].as_str().unwrap())
            .unwrap(),
    );
    set_account(
        &mut f,
        market,
        gm,
        include_bytes!("../fixtures/gmtrade/xau_market.bin").to_vec(),
    );
    let gold = address(PAXG_MINT_ADDRESS);
    set_account(
        &mut f,
        gold,
        anchor_spl::token_2022::ID,
        include_bytes!("../fixtures/gmtrade/paxg_mint.bin").to_vec(),
    );
    let gold_custody =
        Pubkey::find_program_address(&[GOLD_VAULT_SEED, f.vault_state.as_ref()], &f.program_id).0;
    let ix = Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::ConfigureStrategyV1 {
            admin: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
            gold_mint: gold,
            perpetual_program: gm,
            perpetual_store: store,
            perpetual_market: market,
            strategy_v1_state: f.strategy_address(),
            vault_v1_gold_account: gold_custody,
            gold_token_program: anchor_spl::token_2022::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::ConfigureStrategyV1 {}.data(),
    };
    f.run_lifecycle_instruction(ix, true).unwrap();
    assert_eq!(f.token(f.vault_usdc).amount, 10_000_000);
    assert_eq!(f.mint(f.vxau_mint).supply, 10_000_000);
    let a = f.svm.get_account(&f.strategy_address()).unwrap();
    assert!(
        !StrategyV1State::try_deserialize(&mut a.data.as_slice())
            .unwrap()
            .execution_enabled
    );
}

#[test]
fn gold_transfer_fee_reduces_handler_nav() {
    use anchor_spl::token_2022::spl_token_2022::{
        extension::{
            transfer_fee::{TransferFee, TransferFeeConfig},
            BaseStateWithExtensionsMut, ExtensionType, StateWithExtensionsMut,
        },
        state::Mint as Token2022Mint,
    };
    let (mut f, extra) = setup(1_000, 0);
    let size = ExtensionType::try_calculate_account_len::<Token2022Mint>(&[
        ExtensionType::TransferFeeConfig,
    ])
    .unwrap();
    let mut data = vec![0; size];
    let mut state =
        StateWithExtensionsMut::<Token2022Mint>::unpack_uninitialized(&mut data).unwrap();
    let fee = state.init_extension::<TransferFeeConfig>(true).unwrap();
    fee.older_transfer_fee = TransferFee {
        epoch: 0.into(),
        maximum_fee: u64::MAX.into(),
        transfer_fee_basis_points: 1000u16.into(),
    };
    fee.newer_transfer_fee = fee.older_transfer_fee;
    state.base = Token2022Mint {
        mint_authority: COption::None,
        supply: 1000,
        decimals: 6,
        is_initialized: true,
        freeze_authority: COption::None,
    };
    state.pack_base();
    state.init_account_type().unwrap();
    set_account(&mut f, extra[2].pubkey, anchor_spl::token_2022::ID, data);
    // 10 custody + 2.7 net gold = 12.7 NAV. A 1.27 USDC deposit gets one share.
    deposit(&mut f, &extra, 1_270_000, 1_000_000).unwrap();
    assert_eq!(f.mint(f.vxau_mint).supply, 11_000_000);
}

fn trading_setup() -> (Fixture, Vec<AccountMeta>, Pubkey, Pubkey, Pubkey) {
    let (mut f, extras) = setup(0, 0);
    let mut strategy = f.svm.get_account(&f.strategy_address()).unwrap();
    let mut state = StrategyV1State::try_deserialize(&mut &strategy.data[..]).unwrap();
    state.execution_enabled = true;
    state.try_serialize(&mut &mut strategy.data[..]).unwrap();
    f.svm.set_account(f.strategy_address(), strategy).unwrap();
    let mut vault = f.svm.get_account(&f.vault_state).unwrap();
    let mut v = VaultV1State::try_deserialize(&mut &vault.data[..]).unwrap();
    v.max_order_collateral = 10_000_000;
    v.try_serialize(&mut &mut vault.data[..]).unwrap();
    f.svm.set_account(f.vault_state, vault).unwrap();
    let router = address(JUPITER_PROGRAM_ADDRESS);
    f.svm
        .add_program(router, include_bytes!("../fixtures/test_router.so"))
        .unwrap();
    let pool = Pubkey::find_program_address(&[b"mock_pool"], &router).0;
    f.svm.airdrop(&pool, 1_000_000).unwrap();
    let pool_usdc = Pubkey::new_unique();
    let pool_gold = Pubkey::new_unique();
    let usdc = f.usdc_mint;
    let gold = extras[2].pubkey;
    set_token(&mut f, pool_usdc, pool, usdc, 100_000_000, spl_token::ID);
    set_token(
        &mut f,
        pool_gold,
        pool,
        gold,
        1_000_000,
        anchor_spl::token_2022::ID,
    );
    (f, extras, pool, pool_usdc, pool_gold)
}
fn trade_ix(
    f: &Fixture,
    extra: &[AccountMeta],
    pool: Pubkey,
    pool_usdc: Pubkey,
    pool_gold: Pubkey,
    buy: bool,
    input: u64,
    quoted: u64,
    minimum: u64,
) -> Instruction {
    let router = address(JUPITER_PROGRAM_ADDRESS);
    let event = Pubkey::find_program_address(&[b"__event_authority"], &router).0;
    let gold = extra[3].pubkey;
    let mut a = xau_carry_vault::accounts::GoldTradeV1 {
        admin: f.admin.pubkey(),
        vault_v1_state: f.vault_state,
        strategy_v1_state: f.strategy_address(),
        usdc_mint: f.usdc_mint,
        gold_mint: extra[2].pubkey,
        vault_v1_usdc_account: f.vault_usdc,
        vault_v1_gold_account: gold,
        jupiter_program: router,
        gold_price: extra[11].pubkey,
        usdc_price: extra[12].pubkey,
        token_program: spl_token::ID,
        gold_token_program: anchor_spl::token_2022::ID,
    }
    .to_account_metas(None);
    let (source, dest, inmint, outmint, poolin, poolout) = if buy {
        (
            f.vault_usdc,
            gold,
            f.usdc_mint,
            extra[2].pubkey,
            pool_usdc,
            pool_gold,
        )
    } else {
        (
            gold,
            f.vault_usdc,
            extra[2].pubkey,
            f.usdc_mint,
            pool_gold,
            pool_usdc,
        )
    };
    a.extend([
        AccountMeta::new_readonly(spl_token::ID, false),
        AccountMeta::new_readonly(pool, false),
        AccountMeta::new_readonly(f.vault_state, false),
        AccountMeta::new(source, false),
        AccountMeta::new(poolin, false),
        AccountMeta::new(poolout, false),
        AccountMeta::new(dest, false),
        AccountMeta::new_readonly(inmint, false),
        AccountMeta::new_readonly(outmint, false),
        AccountMeta::new_readonly(router, false),
        AccountMeta::new_readonly(anchor_spl::token_2022::ID, false),
        AccountMeta::new_readonly(event, false),
        AccountMeta::new_readonly(router, false),
        AccountMeta::new(poolin, false),
        AccountMeta::new(poolout, false),
        AccountMeta::new_readonly(pool, false),
        AccountMeta::new_readonly(router, false),
    ]);
    let mut data = vec![193, 32, 155, 51, 65, 214, 156, 129, 0]; // shared_accounts_route, id0
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&[30, 100, 0, 1]); // TokenSwapV2,100%,input0,output1
    data.extend_from_slice(&input.to_le_bytes());
    data.extend_from_slice(&quoted.to_le_bytes());
    data.extend_from_slice(&100u16.to_le_bytes());
    data.push(0);
    let params = xau_carry_vault::GoldTradeParamsV1 {
        amount_in: input,
        min_amount_out: minimum,
        route_data: data,
    };
    Instruction {
        program_id: f.program_id,
        accounts: a,
        data: if buy {
            xau_carry_vault::instruction::BuyGoldV1 { params }.data()
        } else {
            xau_carry_vault::instruction::SellGoldV1 { params }.data()
        },
    }
}
#[test]
fn gold_buy_sell_cycle_restores_cash_and_permits_final_redemption() {
    let (mut f, e, p, u, g) = trading_setup();
    let ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 990);
    f.run_lifecycle_instruction(ix, true).unwrap();
    assert_eq!(f.token(f.vault_usdc).amount, 7_000_000);
    assert!(withdraw(&mut f, &e, 10_000_000, 1)
        .unwrap_err()
        .contains("InsufficientLiquidity"));
    // Selling and sweeping are recovery operations, even while new strategy deployment is disabled.
    let mut account = f.svm.get_account(&f.strategy_address()).unwrap();
    let mut config = StrategyV1State::try_deserialize(&mut &account.data[..]).unwrap();
    config.execution_enabled = false;
    config.try_serialize(&mut &mut account.data[..]).unwrap();
    f.svm.set_account(f.strategy_address(), account).unwrap();
    let ix = trade_ix(&f, &e, p, u, g, false, 1_000, 3_000_000, 2_970_000);
    f.run_lifecycle_instruction(ix, true).unwrap();
    assert_eq!(f.token(f.vault_usdc).amount, 10_000_000);
    withdraw(&mut f, &e, 10_000_000, 10_000_000).unwrap();
    assert_eq!(f.token(f.user_vxau).amount, 0);
}
#[test]
fn gold_route_guards_and_failed_output_roll_back_both_transfers() {
    let (mut f, e, p, u, g) = trading_setup();
    let before = f.token(f.vault_usdc).amount;
    let ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 989, 990);
    assert!(f
        .run_lifecycle_instruction(ix, true)
        .unwrap_err()
        .contains("MinimumOutputNotMet"));
    assert_eq!(f.token(f.vault_usdc).amount, before);
    let ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 989);
    assert!(f
        .run_lifecycle_instruction(ix, true)
        .unwrap_err()
        .contains("StrategyPriceBound"));
    let mut ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 990);
    ix.accounts[12 + 6].pubkey = f.user_usdc;
    assert!(f.run_lifecycle_instruction(ix, true).is_err());
    let mut ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 990);
    ix.accounts.push(AccountMeta::new(f.vxau_mint, false));
    assert!(f
        .run_lifecycle_instruction(ix, true)
        .unwrap_err()
        .contains("InvalidGoldRoute"));
    let ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 990);
    let time = f.svm.get_sysvar::<Clock>().unix_timestamp - 31;
    set_price(
        &mut f,
        e[11].pubkey,
        xau_carry_vault::strategy_nav::PAXG_FEED,
        3_000_000_000,
        time,
    );
    assert!(f.run_lifecycle_instruction(ix, true).is_err());
    assert_eq!(f.token(f.vault_usdc).amount, before);
}
#[test]
fn permissionless_sweep_returns_strategy_cash_and_unblocks_queued_settlement() {
    let (mut f, e) = setup(0, 2_000_000);
    f.queue_withdrawal(930, 10_000_000, 12_000_000).unwrap();
    let ix = Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::SweepStrategyUsdcV1 {
            vault_v1_state: f.vault_state,
            usdc_mint: f.usdc_mint,
            strategy_authority: e[0].pubkey,
            strategy_usdc_source: e[1].pubkey,
            vault_v1_usdc_account: f.vault_usdc,
            token_program: spl_token::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::SweepStrategyUsdcV1 {}.data(),
    };
    f.run_lifecycle_instruction(ix, false).unwrap();
    assert_eq!(f.token(f.vault_usdc).amount, 12_000_000);
    settle(&mut f, &e, 930).unwrap();
    assert_eq!(f.token(f.user_vxau).amount, 0);
}
#[test]
fn gold_token2022_transfer_fees_are_measured_as_net_received_assets() {
    use anchor_spl::token_2022::spl_token_2022::{
        extension::{
            transfer_fee::{TransferFee, TransferFeeAmount, TransferFeeConfig},
            BaseStateWithExtensionsMut, ExtensionType, StateWithExtensionsMut,
        },
        state::{Account as A, Mint as M},
    };
    let (mut f, e, p, u, g) = trading_setup();
    let len =
        ExtensionType::try_calculate_account_len::<M>(&[ExtensionType::TransferFeeConfig]).unwrap();
    let mut data = vec![0; len];
    let mut state = StateWithExtensionsMut::<M>::unpack_uninitialized(&mut data).unwrap();
    let config = state.init_extension::<TransferFeeConfig>(true).unwrap();
    config.older_transfer_fee = TransferFee {
        epoch: 0.into(),
        maximum_fee: u64::MAX.into(),
        transfer_fee_basis_points: 100u16.into(),
    };
    config.newer_transfer_fee = config.older_transfer_fee;
    state.base = M {
        mint_authority: COption::None,
        supply: 1_000_000,
        decimals: 6,
        is_initialized: true,
        freeze_authority: COption::None,
    };
    state.pack_base();
    state.init_account_type().unwrap();
    set_account(&mut f, e[2].pubkey, anchor_spl::token_2022::ID, data);
    for token in [e[3].pubkey, g] {
        let old = f.svm.get_account(&token).unwrap();
        let base = A::unpack(&old.data).unwrap();
        let len =
            ExtensionType::try_calculate_account_len::<A>(&[ExtensionType::TransferFeeAmount])
                .unwrap();
        let mut data = vec![0; len];
        let mut state = StateWithExtensionsMut::<A>::unpack_uninitialized(&mut data).unwrap();
        state.init_extension::<TransferFeeAmount>(true).unwrap();
        state.base = base;
        state.pack_base();
        state.init_account_type().unwrap();
        set_account(&mut f, token, anchor_spl::token_2022::ID, data);
    }
    let ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 990);
    f.run_lifecycle_instruction(ix, true).unwrap();
    let account = f.svm.get_account(&e[3].pubkey).unwrap();
    let gold =
        anchor_spl::token_interface::TokenAccount::try_deserialize(&mut &account.data[..]).unwrap();
    assert_eq!(gold.amount, 990);
    let ix = trade_ix(&f, &e, p, u, g, false, 990, 2_940_000, 2_910_600);
    f.run_lifecycle_instruction(ix, true).unwrap();
    withdraw(&mut f, &e, 10_000_000, 9_940_000).unwrap();
    assert_eq!(f.token(f.user_vxau).amount, 0);
}
#[test]
fn gold_buy_rejects_wrong_admin_and_deployment_disabled_but_sell_is_recovery() {
    let (mut f, e, p, u, g) = trading_setup();
    let mut ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 990);
    ix.accounts[0].pubkey = f.user.pubkey();
    assert!(f
        .run_lifecycle_instruction(ix, false)
        .unwrap_err()
        .contains("UnauthorizedStrategyAdmin"));
    let mut account = f.svm.get_account(&f.strategy_address()).unwrap();
    let mut config = StrategyV1State::try_deserialize(&mut &account.data[..]).unwrap();
    config.execution_enabled = false;
    config.try_serialize(&mut &mut account.data[..]).unwrap();
    f.svm.set_account(f.strategy_address(), account).unwrap();
    let ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 990);
    assert!(f
        .run_lifecycle_instruction(ix, true)
        .unwrap_err()
        .contains("StrategyExecutionDisabled"));
    assert_eq!(f.token(f.vault_usdc).amount, 10_000_000);
}
/// Full handler cycle with real GMTrade submission/close CPIs and controlled execution receipts.
/// The router and price updates are test fixtures; no live market execution is claimed.
#[test]
fn complete_carry_handler_cycle_deploys_then_unwinds_and_settles_queue() {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let (mut f, e, p, u, g) = trading_setup();
    let gm = address(GMTRADE_PROGRAM_ADDRESS);
    let store = address(GMTRADE_STORE_ADDRESS);
    let market = e[7].pubkey;
    f.svm
        .add_program(gm, include_bytes!("../fixtures/gmtrade/gmtrade.so"))
        .unwrap();
    f.svm
        .set_sysvar(&solana_last_restart_slot::LastRestartSlot {
            last_restart_slot: 246_464_040,
        });
    let snapshot: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/gmtrade/store.json")).unwrap();
    set_account(
        &mut f,
        store,
        gm,
        STANDARD
            .decode(snapshot["account"]["data"][0].as_str().unwrap())
            .unwrap(),
    );
    let authority = e[0].pubkey;
    let source = e[1].pubkey;
    let position = e[8].pubkey;
    let user = Pubkey::find_program_address(
        &[GMTRADE_USER_SEED, store.as_ref(), authority.as_ref()],
        &gm,
    )
    .0;
    let event = Pubkey::find_program_address(&[GMTRADE_EVENT_AUTHORITY_SEED], &gm).0;
    let wallet = Pubkey::find_program_address(&[b"store_wallet", store.as_ref()], &gm).0;
    let ix = Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::PreparePerpetualUserV1 {
            admin: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
            strategy_v1_state: f.strategy_address(),
            strategy_authority: authority,
            perpetual_program: gm,
            perpetual_store: store,
            perpetual_user: user,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::PreparePerpetualUserV1 {
            funding_lamports: 100_000_000,
        }
        .data(),
    };
    f.run_lifecycle_instruction(ix, true).unwrap();
    let ix = Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::PrepareShortPositionV1 {
            admin: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
            strategy_v1_state: f.strategy_address(),
            strategy_authority: authority,
            perpetual_program: gm,
            perpetual_store: store,
            perpetual_market: market,
            perpetual_position: position,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::PrepareShortPositionV1 {
            funding_lamports: 0,
        }
        .data(),
    };
    f.run_lifecycle_instruction(ix, true).unwrap();
    let ix = trade_ix(&f, &e, p, u, g, true, 3_000_000, 1_000, 990);
    f.run_lifecycle_instruction(ix, true).unwrap();
    let nonce = [150u8; 32];
    let order = crate_order(gm, store, authority, nonce);
    let escrow = get_associated_token_address(&order, &f.usdc_mint);
    let baseline =
        Pubkey::find_program_address(&[ORDER_BASELINE_SEED, order.as_ref()], &f.program_id).0;
    let size = 3 * 10u128.pow(20);
    let create = Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::CreateShortOrderV1 {
            admin: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
            strategy_v1_state: f.strategy_address(),
            usdc_mint: f.usdc_mint,
            vault_v1_usdc_account: f.vault_usdc,
            strategy_authority: authority,
            strategy_usdc_source: source,
            perpetual_program: gm,
            perpetual_store: store,
            perpetual_market: market,
            perpetual_user: user,
            perpetual_position: position,
            perpetual_order: order,
            order_usdc_escrow: escrow,
            pending_short_order: e[4].pubkey,
            order_baseline: baseline,
            perpetual_event_authority: event,
            token_program: spl_token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::CreateShortOrderV1 {
            nonce,
            params: xau_carry_vault::CreateShortOrderParamsV1 {
                collateral_amount: 2_000_000,
                size_delta_value: size,
                acceptable_price: 3_000_000_000_000_000,
                execution_lamports: 50_000_000,
                funding_lamports: 100_000_000,
            },
        }
        .data(),
    };
    run_budgeted(&mut f, create).unwrap();
    assert_eq!(f.token(f.vault_usdc).amount, 5_000_000);
    inject_fill(&mut f, order, position, escrow, size, 100_000, 2_000_000, 0);
    let reconcile = Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::ReconcileShortOrderV1 {
            keeper: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
            strategy_v1_state: f.strategy_address(),
            pending_short_order: e[4].pubkey,
            order_baseline: baseline,
            refund: f.admin.pubkey(),
            usdc_mint: f.usdc_mint,
            vault_v1_usdc_account: f.vault_usdc,
            strategy_authority: authority,
            strategy_usdc_source: source,
            perpetual_program: gm,
            perpetual_store: store,
            perpetual_store_wallet: wallet,
            perpetual_user: user,
            perpetual_order: order,
            order_usdc_escrow: escrow,
            perpetual_event_authority: event,
            perpetual_market: market,
            perpetual_position: position,
            token_program: spl_token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::ReconcileShortOrderV1 {}.data(),
    };
    run_budgeted(&mut f, reconcile).unwrap();
    f.queue_withdrawal(941, 10_000_000, 10_000_000).unwrap();
    assert!(settle(&mut f, &e, 941).is_err()); // cash is still deployed
    let nonce = [151u8; 32];
    let order = crate_order(gm, store, authority, nonce);
    let escrow = get_associated_token_address(&order, &f.usdc_mint);
    let baseline =
        Pubkey::find_program_address(&[DECREASE_BASELINE_SEED, order.as_ref()], &f.program_id).0;
    let mut create = Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::CreateShortDecreaseV1 {
            admin: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
            strategy_v1_state: f.strategy_address(),
            usdc_mint: f.usdc_mint,
            vault_v1_usdc_account: f.vault_usdc,
            strategy_authority: authority,
            strategy_usdc_source: source,
            perpetual_program: gm,
            perpetual_store: store,
            perpetual_market: market,
            perpetual_user: user,
            perpetual_position: position,
            perpetual_order: order,
            order_usdc_escrow: escrow,
            pending_short_order: e[4].pubkey,
            order_baseline: baseline,
            perpetual_event_authority: event,
            token_program: spl_token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::CreateShortDecreaseV1 {
            nonce,
            params: xau_carry_vault::ShortDecreaseParamsV1 {
                collateral_withdrawal: 0,
                size_delta_value: size,
                acceptable_price: 3_000_000_000_000_000,
                min_usdc_out: 2_000_000,
                allow_full_close: false,
                execution_lamports: 50_000_000,
                funding_lamports: 100_000_000,
            },
        }
        .data(),
    };
    create
        .accounts
        .extend_from_slice(&[e[13].clone(), e[12].clone()]);
    run_budgeted(&mut f, create).unwrap();
    inject_fill(&mut f, order, position, escrow, 0, 0, 0, 2_000_000);
    let recover = Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::RecoverShortDecreaseV1 {
            keeper: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
            strategy_v1_state: f.strategy_address(),
            pending_short_order: e[4].pubkey,
            order_baseline: baseline,
            refund: f.admin.pubkey(),
            usdc_mint: f.usdc_mint,
            vault_v1_usdc_account: f.vault_usdc,
            strategy_authority: authority,
            strategy_usdc_source: source,
            perpetual_program: gm,
            perpetual_store: store,
            perpetual_store_wallet: wallet,
            perpetual_user: user,
            perpetual_order: order,
            order_usdc_escrow: escrow,
            perpetual_event_authority: event,
            perpetual_market: market,
            perpetual_position: position,
            token_program: spl_token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::ReconcileShortDecreaseV1 {}.data(),
    };
    run_budgeted(&mut f, recover).unwrap();
    assert_eq!(f.token(f.vault_usdc).amount, 7_000_000);
    let ix = trade_ix(&f, &e, p, u, g, false, 1_000, 3_000_000, 2_970_000);
    f.run_lifecycle_instruction(ix, true).unwrap();
    settle(&mut f, &e, 941).unwrap();
    assert_eq!(f.mint(f.vxau_mint).supply, 0);
    f.assert_queue_closed(941);
}
fn crate_order(program: Pubkey, store: Pubkey, authority: Pubkey, nonce: [u8; 32]) -> Pubkey {
    xau_carry_vault::adapters::gmtrade::order_address(&program, &store, &authority, &nonce).0
}
fn run_budgeted(f: &mut Fixture, ix: Instruction) -> Result<(), String> {
    let mut data = vec![2];
    data.extend_from_slice(&600_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: address("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data,
    };
    let message = Message::new_with_blockhash(
        &[budget, ix],
        Some(&f.admin.pubkey()),
        &f.svm.latest_blockhash(),
    );
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[&f.admin]).unwrap();
    let r = f
        .svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"));
    f.svm.expire_blockhash();
    r
}
fn inject_fill(
    f: &mut Fixture,
    order: Pubkey,
    position: Pubkey,
    escrow: Pubkey,
    size: u128,
    tokens: u128,
    collateral: u128,
    payout: u64,
) {
    let clock = f.svm.get_sysvar::<Clock>();
    let mut a = f.svm.get_account(&order).unwrap();
    let mut o = bytemuck::allocation::zeroed_box::<gmsol_programs::gmsol_store::accounts::Order>();
    bytemuck::bytes_of_mut(&mut *o).copy_from_slice(&a.data[8..]);
    o.header.action_state = 1;
    o.header.updated_at_slot = clock.slot;
    a.data[8..].copy_from_slice(bytemuck::bytes_of(&*o));
    f.svm.set_account(order, a).unwrap();
    let mut a = f.svm.get_account(&position).unwrap();
    let mut p =
        bytemuck::allocation::zeroed_box::<gmsol_programs::gmsol_store::accounts::Position>();
    bytemuck::bytes_of_mut(&mut *p).copy_from_slice(&a.data[8..]);
    p.state.trade_id += 1;
    p.state.updated_at_slot = clock.slot;
    p.state.size_in_usd = size;
    p.state.size_in_tokens = tokens;
    p.state.collateral_amount = collateral;
    a.data[8..].copy_from_slice(bytemuck::bytes_of(&*p));
    f.svm.set_account(position, a).unwrap();
    let mut a = f.svm.get_account(&escrow).unwrap();
    let mut t = SplTokenAccount::unpack(&a.data).unwrap();
    t.amount = payout;
    SplTokenAccount::pack(t, &mut a.data).unwrap();
    f.svm.set_account(escrow, a).unwrap();
}
