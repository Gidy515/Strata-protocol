use anchor_spl::token::spl_token;
use solana_last_restart_slot::LastRestartSlot;
use solana_program_option::COption;
use solana_program_pack::Pack;
use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, AccountSerialize, InstructionData, ToAccountMetas,
    },
    base64::{engine::general_purpose::STANDARD, Engine},
    litesvm::LiteSVM,
    serde_json::Value,
    solana_account::Account,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    std::{fs, path::PathBuf, str::FromStr},
    xau_carry_vault::{
        constants::*,
        state::{StrategyV1State, VaultV1State},
    },
};

fn key(address: &str) -> Pubkey {
    Pubkey::from_str(address).unwrap()
}

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gmtrade")
}

fn load_snapshot(svm: &mut LiteSVM, filename: &str, expected: Pubkey) {
    let path = fixture_directory().join(filename);

    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("Cannot read {}: {error}", path.display()));

    let json: Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("Invalid {}: {error}", path.display()));

    let address = key(json["pubkey"]
        .as_str()
        .expect("Snapshot must contain pubkey"));

    assert_eq!(address, expected, "Wrong snapshot address");

    let account = &json["account"];

    assert_eq!(
        account["data"][1].as_str(),
        Some("base64"),
        "Expected base64 account data",
    );

    let data: Vec<u8> = STANDARD
        .decode(account["data"][0].as_str().expect("Missing account data"))
        .unwrap();

    let owner = key(account["owner"].as_str().expect("Missing account owner"));

    assert_eq!(owner, key(GMTRADE_PROGRAM_ADDRESS));

    svm.set_account(
        address,
        Account {
            lamports: account["lamports"].as_u64().unwrap(),
            data,
            owner,
            executable: account["executable"].as_bool().unwrap(),
            rent_epoch: account["rentEpoch"].as_u64().unwrap(),
        },
    )
    .unwrap();
}

fn insert_state<T: AccountSerialize>(svm: &mut LiteSVM, address: Pubkey, state: &T, space: usize) {
    let mut serialized: Vec<u8> = Vec::new();
    state.try_serialize(&mut serialized).unwrap();

    assert!(serialized.len() <= space);
    serialized.resize(space, 0);

    svm.set_account(
        address,
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(space),
            data: serialized,
            owner: xau_carry_vault::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn send(svm: &mut LiteSVM, admin: &Keypair, instruction: Instruction) {
    let mut budget_data = vec![2];
    budget_data.extend_from_slice(&600_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: key("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data: budget_data,
    };
    let message = Message::new_with_blockhash(
        &[budget, instruction],
        Some(&admin.pubkey()),
        &svm.latest_blockhash(),
    );

    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[admin]).unwrap();

    let result = svm.send_transaction(transaction);

    if let Err(error) = result {
        panic!("Preparation CPI failed:\n{error:#?}");
    }

    // Allows another transaction with otherwise identical instruction data.
    svm.expire_blockhash();
}

fn read_key(data: &[u8], offset: usize) -> Pubkey {
    Pubkey::new_from_array(data[offset..offset + 32].try_into().unwrap())
}

#[test]
#[ignore = "Requires a captured GMTrade program, store, and market"]
fn snapshot_prepare_cpis_create_and_reuse_accounts() {
    let mut svm = LiteSVM::new();
    // Match the restart slot recorded by this captured GMTrade store.
    svm.set_sysvar(&LastRestartSlot {
        last_restart_slot: 246_464_040,
    });
    let admin = Keypair::new();

    let our_program = xau_carry_vault::id();
    let external_program = key(GMTRADE_PROGRAM_ADDRESS);
    let store = key(GMTRADE_STORE_ADDRESS);
    let market = key(GMTRADE_XAU_MARKET_ADDRESS);
    let usdc = key(MAINNET_USDC_MINT);

    let our_binary =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy/xau_carry_vault.so");

    svm.add_program(
        our_program,
        &fs::read(&our_binary).expect("Run anchor build first"),
    )
    .unwrap();

    svm.add_program(
        external_program,
        &fs::read(fixture_directory().join("gmtrade.so"))
            .expect("Capture the GMTrade binary first"),
    )
    .unwrap();

    load_snapshot(&mut svm, "store.json", store);
    load_snapshot(&mut svm, "market.json", market);

    svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();

    let (vault, vault_bump) =
        Pubkey::find_program_address(&[VAULT_SEED, usdc.as_ref()], &our_program);

    let (receipt_mint, _) =
        Pubkey::find_program_address(&[VXAU_MINT_SEED, vault.as_ref()], &our_program);

    let (strategy, strategy_bump) =
        Pubkey::find_program_address(&[STRATEGY_SEED, vault.as_ref()], &our_program);

    let (authority, _) =
        Pubkey::find_program_address(&[STRATEGY_AUTHORITY_SEED, vault.as_ref()], &our_program);

    let (gold_custody, _) =
        Pubkey::find_program_address(&[GOLD_VAULT_SEED, vault.as_ref()], &our_program);

    // Inject our state to isolate preparation CPI compatibility.
    // Initialization/configuration are outside this test's scope.
    insert_state(
        &mut svm,
        vault,
        &VaultV1State {
            admin: admin.pubkey(),
            usdc_mint: usdc,
            vxau_mint: receipt_mint,
            total_deposited: 0,
            bump: vault_bump,
            pending_admin: Pubkey::default(),
            deposits_paused: false,
            withdrawals_paused: false,
            strategy_paused: false,
            max_order_collateral: u64::MAX,
            max_order_size: u128::MAX,
        },
        8 + <VaultV1State as anchor_lang::Space>::INIT_SPACE,
    );

    insert_state(
        &mut svm,
        strategy,
        &StrategyV1State {
            vault,
            gold_mint: key(PAXG_MINT_ADDRESS),
            gold_custody,
            gold_token_program: anchor_spl::token_2022::ID,
            perpetual_program: external_program,
            perpetual_store: store,
            perpetual_market: market,
            configured_at_slot: 0,
            execution_enabled: false,
            bump: strategy_bump,
        },
        8 + 32 * 7 + 8 + 1 + 1,
    );

    // Rent funding only. No USDC or gold is deposited in this test.
    svm.airdrop(&authority, 100_000_000).unwrap();

    let (external_user, user_bump) = Pubkey::find_program_address(
        &[GMTRADE_USER_SEED, store.as_ref(), authority.as_ref()],
        &external_program,
    );

    let market_account = svm.get_account(&market).unwrap();
    assert_eq!(
        &market_account.data[..8],
        GMTRADE_MARKET_DISCRIMINATOR.as_slice(),
    );
    let market_token = read_key(&market_account.data, 88);

    let (position, position_bump) = Pubkey::find_program_address(
        &[
            GMTRADE_POSITION_SEED,
            store.as_ref(),
            authority.as_ref(),
            market_token.as_ref(),
            usdc.as_ref(),
            &[GMTRADE_SHORT_POSITION_KIND],
        ],
        &external_program,
    );

    let prepare_user = Instruction {
        program_id: our_program,
        accounts: xau_carry_vault::accounts::PreparePerpetualUserV1 {
            admin: admin.pubkey(),
            vault_v1_state: vault,
            strategy_v1_state: strategy,
            strategy_authority: authority,
            perpetual_program: external_program,
            perpetual_store: store,
            perpetual_user: external_user,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::PreparePerpetualUserV1 {
            funding_lamports: 0,
        }
        .data(),
    };

    let prepare_position = Instruction {
        program_id: our_program,
        accounts: xau_carry_vault::accounts::PrepareShortPositionV1 {
            admin: admin.pubkey(),
            vault_v1_state: vault,
            strategy_v1_state: strategy,
            strategy_authority: authority,
            perpetual_program: external_program,
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

    send(&mut svm, &admin, prepare_user.clone());

    let user_account = svm.get_account(&external_user).unwrap();
    assert_eq!(user_account.owner, external_program);
    assert_eq!(
        &user_account.data[..8],
        GMTRADE_USER_DISCRIMINATOR.as_slice(),
    );
    assert_eq!(user_account.data[9], user_bump);
    assert_eq!(read_key(&user_account.data, 24), authority);
    assert_eq!(read_key(&user_account.data, 56), store);

    send(&mut svm, &admin, prepare_position.clone());

    let position_account = svm.get_account(&position).unwrap();
    assert_eq!(position_account.owner, external_program);
    assert_eq!(
        &position_account.data[..8],
        GMTRADE_POSITION_DISCRIMINATOR.as_slice(),
    );
    assert_eq!(position_account.data[9], position_bump);
    assert_eq!(position_account.data[42], GMTRADE_SHORT_POSITION_KIND);
    assert_eq!(read_key(&position_account.data, 10), store);
    assert_eq!(read_key(&position_account.data, 56), authority);
    assert_eq!(read_key(&position_account.data, 88), market_token);
    assert_eq!(read_key(&position_account.data, 120), usdc);

    let remaining_sol = svm.get_account(&authority).unwrap().lamports;
    assert!(remaining_sol < 100_000_000, "Creation should spend rent");

    // Existing accounts should validate without being reset or charged rent.
    send(&mut svm, &admin, prepare_user);
    send(&mut svm, &admin, prepare_position);

    assert_eq!(
        svm.get_account(&external_user).unwrap().data,
        user_account.data,
    );
    assert_eq!(
        svm.get_account(&position).unwrap().data,
        position_account.data,
    );
    assert_eq!(svm.get_account(&authority).unwrap().lamports, remaining_sol,);

    let authority_account = svm.get_account(&authority).unwrap();
    assert_eq!(authority_account.owner, system_program::ID);
    assert!(authority_account.data.is_empty());

    exercise_order_creation(
        &mut svm,
        &admin,
        vault,
        strategy,
        authority,
        store,
        market,
        external_user,
        position,
    );
}

fn install_mock_usdc(
    svm: &mut LiteSVM,
    mint: Pubkey,
    custody: Pubkey,
    vault: Pubkey,
    source: Pubkey,
    authority: Pubkey,
) {
    let mut mint_data: Vec<u8> = vec![0u8; spl_token::state::Mint::LEN];

    let mint_state = spl_token::state::Mint {
        mint_authority: COption::None,
        supply: 1_007_000_000,
        decimals: 6,
        is_initialized: true,
        freeze_authority: COption::None,
    };

    spl_token::state::Mint::pack(mint_state, mint_data.as_mut_slice()).unwrap();

    let mint_lamports = svm.minimum_balance_for_rent_exemption(mint_data.len());

    svm.set_account(
        mint,
        Account {
            lamports: mint_lamports,
            data: mint_data,
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    for (address, token_authority, amount) in [
        (custody, vault, 1_000_000_000u64),
        (source, authority, 7_000_000u64),
    ] {
        let mut data: Vec<u8> = vec![0u8; spl_token::state::Account::LEN];

        let token_state = spl_token::state::Account {
            mint,
            owner: token_authority,
            amount,
            delegate: COption::None,
            state: spl_token::state::AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        spl_token::state::Account::pack(token_state, data.as_mut_slice()).unwrap();

        let token_lamports = svm.minimum_balance_for_rent_exemption(data.len());

        svm.set_account(
            address,
            Account {
                lamports: token_lamports,
                data,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }
}

fn token_amount(svm: &LiteSVM, address: Pubkey) -> u64 {
    let account = svm.get_account(&address).unwrap();

    assert_eq!(account.owner, spl_token::ID);

    let token_state = spl_token::state::Account::unpack(account.data.as_slice()).unwrap();

    token_state.amount
}

fn expect_failure(svm: &mut LiteSVM, admin: &Keypair, instruction: Instruction) -> String {
    let mut budget_data = vec![2];
    budget_data.extend_from_slice(&600_000u32.to_le_bytes());
    let budget = Instruction {
        program_id: key("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data: budget_data,
    };
    let message = Message::new_with_blockhash(
        &[budget, instruction],
        Some(&admin.pubkey()),
        &svm.latest_blockhash(),
    );

    let transaction =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[admin]).unwrap();

    let error = svm
        .send_transaction(transaction)
        .expect_err("Transaction unexpectedly succeeded");

    svm.expire_blockhash();

    format!("{error:#?}")
}

fn assert_uncreated(svm: &LiteSVM, address: Pubkey) {
    if let Some(account) = svm.get_account(&address) {
        assert_eq!(account.lamports, 0);
        assert!(account.data.is_empty());
    }
}

fn exercise_order_creation(
    svm: &mut LiteSVM,
    admin: &Keypair,
    vault: Pubkey,
    strategy: Pubkey,
    authority: Pubkey,
    store: Pubkey,
    market: Pubkey,
    external_user: Pubkey,
    position: Pubkey,
) {
    use xau_carry_vault::{
        adapters::gmtrade::order_address, instructions::CreateShortOrderParamsV1,
        state::PendingShortOrderV1,
    };

    let our_program = xau_carry_vault::id();
    let external_program = key(GMTRADE_PROGRAM_ADDRESS);
    let usdc = key(MAINNET_USDC_MINT);

    let (custody, _) =
        Pubkey::find_program_address(&[TOKEN_VAULT_SEED, vault.as_ref()], &our_program);

    let source = anchor_spl::associated_token::get_associated_token_address(&authority, &usdc);

    install_mock_usdc(svm, usdc, custody, vault, source, authority);

    let nonce = [41u8; 32];

    let (order, _) = order_address(&external_program, &store, &authority, &nonce);

    let escrow = anchor_spl::associated_token::get_associated_token_address(&order, &usdc);

    let (pending, _) =
        Pubkey::find_program_address(&[PENDING_SHORT_ORDER_SEED, vault.as_ref()], &our_program);

    let (event_authority, _) =
        Pubkey::find_program_address(&[GMTRADE_EVENT_AUTHORITY_SEED], &external_program);

    // Synthetic order-creation inputs only.
    // No oracle, position execution, or realistic pricing is tested.
    let collateral_amount = 100_000_000u64;
    let size_delta_value = 100_000_000_000_000_000_000_000u128;
    let acceptable_price = 1u128;
    let execution_lamports = 50_000_000u64;
    let funding_lamports = 100_000_000u64;

    let instruction = Instruction {
        program_id: our_program,
        accounts: xau_carry_vault::accounts::CreateShortOrderV1 {
            admin: admin.pubkey(),
            vault_v1_state: vault,
            strategy_v1_state: strategy,
            usdc_mint: usdc,
            vault_v1_usdc_account: custody,
            strategy_authority: authority,
            strategy_usdc_source: source,
            perpetual_program: external_program,
            perpetual_store: store,
            perpetual_market: market,
            perpetual_user: external_user,
            perpetual_position: position,
            perpetual_order: order,
            order_usdc_escrow: escrow,
            pending_short_order: pending,
            order_baseline: Pubkey::find_program_address(
                &[ORDER_BASELINE_SEED, order.as_ref()],
                &our_program,
            )
            .0,
            perpetual_event_authority: event_authority,
            token_program: spl_token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::CreateShortOrderV1 {
            nonce,
            params: CreateShortOrderParamsV1 {
                collateral_amount,
                size_delta_value,
                acceptable_price,
                execution_lamports,
                funding_lamports,
            },
        }
        .data(),
    };

    // 1. Real configuration remains disabled.
    let error = expect_failure(svm, admin, instruction.clone());

    assert!(error.contains("StrategyExecutionDisabled"), "{error}",);

    assert_eq!(token_amount(svm, custody), 1_000_000_000);
    assert_eq!(token_amount(svm, source), 7_000_000);
    assert_uncreated(svm, order);
    assert_uncreated(svm, escrow);
    assert_uncreated(svm, pending);

    // Enable only the injected local test account.
    let strategy_account = svm.get_account(&strategy).unwrap();
    let mut strategy_data = strategy_account.data.as_slice();

    let mut strategy_state = StrategyV1State::try_deserialize(&mut strategy_data).unwrap();

    strategy_state.execution_enabled = true;

    insert_state(svm, strategy, &strategy_state, strategy_account.data.len());

    // 2. Force GMTrade to reject its user account.
    // Our handler does not inspect this initialized flag, so rejection
    // should happen inside GMTrade after our collateral transfer.
    let valid_user = svm.get_account(&external_user).unwrap();
    let mut invalid_user = valid_user.clone();

    invalid_user.data[10] = 0;

    svm.set_account(external_user, invalid_user).unwrap();

    let authority_sol_before = svm.get_account(&authority).unwrap().lamports;

    let position_before = svm.get_account(&position).unwrap().data;

    let market_before = svm.get_account(&market).unwrap().data;

    let error = expect_failure(svm, admin, instruction.clone());

    assert!(
        error.contains("InvalidUserAccount"),
        "Expected GMTrade user validation failure:\n{error}",
    );

    assert!(
        error.contains("TransferChecked"),
        "Expected collateral transfer before CPI rejection:\n{error}",
    );

    assert_eq!(token_amount(svm, custody), 1_000_000_000);
    assert_eq!(token_amount(svm, source), 7_000_000);

    assert_eq!(
        svm.get_account(&authority).unwrap().lamports,
        authority_sol_before,
    );

    assert_eq!(svm.get_account(&position).unwrap().data, position_before,);

    assert_eq!(svm.get_account(&market).unwrap().data, market_before,);

    assert_uncreated(svm, order);
    assert_uncreated(svm, escrow);
    assert_uncreated(svm, pending);

    svm.set_account(external_user, valid_user).unwrap();

    // 3. Create the real external order in the local VM.
    send(svm, admin, instruction.clone());

    assert_eq!(
        token_amount(svm, custody),
        1_000_000_000 - collateral_amount,
    );

    // Existing source tokens must remain untouched.
    assert_eq!(token_amount(svm, source), 7_000_000);

    assert_eq!(token_amount(svm, escrow), collateral_amount,);

    let order_account = svm.get_account(&order).unwrap();

    assert_eq!(order_account.owner, external_program);
    assert_eq!(
        &order_account.data[..8],
        GMTRADE_ORDER_DISCRIMINATOR.as_slice(),
    );

    let pending_account = svm.get_account(&pending).unwrap();
    assert_eq!(pending_account.owner, our_program);

    let mut pending_data = pending_account.data.as_slice();

    let record = PendingShortOrderV1::try_deserialize(&mut pending_data).unwrap();

    assert_eq!(record.vault, vault);
    assert_eq!(record.order, order);
    assert_eq!(record.position, position);
    assert_eq!(record.strategy_authority, authority);
    assert_eq!(record.nonce, nonce);
    assert_eq!(record.collateral_amount, collateral_amount);
    assert_eq!(record.size_delta_value, size_delta_value);
    assert_eq!(record.acceptable_price, acceptable_price);
    assert_eq!(record.execution_lamports, execution_lamports);

    // Order creation should not change the prepared position itself.
    assert_eq!(svm.get_account(&position).unwrap().data, position_before,);

    // 4. A second order with a fresh nonce must be blocked by
    // the singleton pending-order account.
    let second_nonce = [42u8; 32];

    let (second_order, _) = order_address(&external_program, &store, &authority, &second_nonce);

    let second_escrow =
        anchor_spl::associated_token::get_associated_token_address(&second_order, &usdc);

    let mut second_instruction = instruction.clone();

    for account in &mut second_instruction.accounts {
        if account.pubkey == order {
            account.pubkey = second_order;
        } else if account.pubkey == escrow {
            account.pubkey = second_escrow;
        }
    }

    second_instruction.accounts[15].pubkey =
        Pubkey::find_program_address(&[ORDER_BASELINE_SEED, second_order.as_ref()], &our_program).0;
    second_instruction.data = xau_carry_vault::instruction::CreateShortOrderV1 {
        nonce: second_nonce,
        params: CreateShortOrderParamsV1 {
            collateral_amount,
            size_delta_value,
            acceptable_price,
            execution_lamports,
            funding_lamports,
        },
    }
    .data();

    let custody_before_second = token_amount(svm, custody);
    let authority_before_second = svm.get_account(&authority).unwrap().lamports;

    let error = expect_failure(svm, admin, second_instruction.clone());

    assert!(
        error.contains("already in use"),
        "Expected pending-account initialization rejection:\n{error}",
    );

    assert_eq!(token_amount(svm, custody), custody_before_second,);

    assert_eq!(
        svm.get_account(&authority).unwrap().lamports,
        authority_before_second,
    );

    assert_eq!(
        svm.get_account(&pending).unwrap().data,
        pending_account.data,
    );

    assert_eq!(svm.get_account(&order).unwrap().data, order_account.data,);

    assert_eq!(token_amount(svm, escrow), collateral_amount);
    assert_uncreated(svm, second_order);
    assert_uncreated(svm, second_escrow);

    // Cancellation must remain available while strategy execution is disabled.
    let strategy_account = svm.get_account(&strategy).unwrap();
    let mut strategy_data = strategy_account.data.as_slice();

    let mut strategy_state = StrategyV1State::try_deserialize(&mut strategy_data).unwrap();

    strategy_state.execution_enabled = false;

    insert_state(svm, strategy, &strategy_state, strategy_account.data.len());

    let (store_wallet, _) =
        xau_carry_vault::adapters::gmtrade::store_wallet_address(&external_program, &store);

    // Synthetic system-owned store wallet for this local CPI test.
    // The external program and store data remain the captured versions.
    svm.airdrop(&store_wallet, 1_000_000_000).unwrap();

    let cancel_instruction = Instruction {
        program_id: our_program,
        accounts: xau_carry_vault::accounts::CancelShortOrderV1 {
            admin: admin.pubkey(),
            vault_v1_state: vault,
            strategy_v1_state: strategy,
            pending_short_order: pending,
            order_baseline: Some(
                Pubkey::find_program_address(&[ORDER_BASELINE_SEED, order.as_ref()], &our_program)
                    .0,
            ),
            usdc_mint: usdc,
            vault_v1_usdc_account: custody,
            strategy_authority: authority,
            strategy_usdc_source: source,
            perpetual_program: external_program,
            perpetual_store: store,
            perpetual_store_wallet: store_wallet,
            perpetual_user: external_user,
            perpetual_order: order,
            order_usdc_escrow: escrow,
            perpetual_event_authority: event_authority,
            token_program: spl_token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::CancelShortOrderV1 {}.data(),
    };

    // A completed order must not be treated as cancelled collateral.
    let valid_order = svm.get_account(&order).unwrap();
    let mut completed_order = valid_order.clone();
    completed_order.data[9] = 1;

    svm.set_account(order, completed_order).unwrap();

    let error = expect_failure(svm, admin, cancel_instruction.clone());
    assert!(error.contains("OrderNotPending"), "{error}");

    assert_eq!(token_amount(svm, custody), 900_000_000);
    assert_eq!(token_amount(svm, escrow), collateral_amount);
    assert_eq!(
        svm.get_account(&pending).unwrap().data,
        pending_account.data,
    );

    svm.set_account(order, valid_order).unwrap();

    // Cancel the actual pending external order and recover its collateral.
    send(svm, admin, cancel_instruction);

    assert_eq!(token_amount(svm, custody), 1_000_000_000);
    assert_eq!(token_amount(svm, source), 7_000_000);

    assert_uncreated(svm, order);
    assert_uncreated(svm, escrow);
    assert_uncreated(svm, pending);

    assert_eq!(svm.get_account(&position).unwrap().data, position_before,);

    let baseline_address =
        Pubkey::find_program_address(&[ORDER_BASELINE_SEED, order.as_ref()], &our_program).0;
    assert_uncreated(svm, baseline_address);
    println!("Order cancellation returned collateral and cleared pending state");

    // Submit again through the real captured GMTrade program, now starting from a
    // nonzero position baseline. Execution outcome below is injected, not a live oracle trade.
    let strategy_account = svm.get_account(&strategy).unwrap();
    let mut state =
        StrategyV1State::try_deserialize(&mut strategy_account.data.as_slice()).unwrap();
    state.execution_enabled = true;
    insert_state(svm, strategy, &state, strategy_account.data.len());
    let mut before = svm.get_account(&position).unwrap();
    let mut typed =
        bytemuck::allocation::zeroed_box::<gmsol_programs::gmsol_store::accounts::Position>();
    bytemuck::bytes_of_mut(&mut *typed).copy_from_slice(&before.data[8..]);
    typed.state.size_in_usd = 50 * 10u128.pow(20);
    typed.state.size_in_tokens = 1_000_000;
    typed.state.collateral_amount = 10_000_000;
    typed.state.trade_id = 8;
    before.data[8..].copy_from_slice(bytemuck::bytes_of(&*typed));
    svm.set_account(position, before).unwrap();
    let second_baseline = second_instruction.accounts[15].pubkey;
    // A donated lamport cannot block creation of the canonical baseline extension.
    svm.airdrop(&second_baseline, 1).unwrap();
    send(svm, admin, second_instruction.clone());
    let saved = svm.get_account(&second_baseline).unwrap();
    let baseline =
        xau_carry_vault::state::OrderBaselineV1::try_deserialize(&mut saved.data.as_slice())
            .unwrap();
    assert_eq!(baseline.size_before, 50 * 10u128.pow(20));
    assert_eq!(baseline.trade_id_before, 8);
    assert_eq!(baseline.collateral_before, 10_000_000);
    assert_eq!(baseline.order, second_order);
    assert_eq!(baseline.nonce, second_nonce);
    let reconciliation = Instruction {
        program_id: our_program,
        accounts: xau_carry_vault::accounts::ReconcileShortOrderV1 {
            keeper: admin.pubkey(),
            refund: admin.pubkey(),
            vault_v1_state: vault,
            strategy_v1_state: strategy,
            pending_short_order: pending,
            order_baseline: second_baseline,
            usdc_mint: usdc,
            vault_v1_usdc_account: custody,
            strategy_authority: authority,
            strategy_usdc_source: source,
            perpetual_program: external_program,
            perpetual_store: store,
            perpetual_store_wallet: store_wallet,
            perpetual_user: external_user,
            perpetual_order: second_order,
            order_usdc_escrow: second_escrow,
            perpetual_event_authority: event_authority,
            perpetual_market: market,
            perpetual_position: position,
            token_program: spl_token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::ReconcileShortOrderV1 {}.data(),
    };
    // Pending order / empty escrow alone cannot establish execution.
    let error = expect_failure(svm, admin, reconciliation.clone());
    assert!(error.contains("UnprovenOrderExecution"), "{error}");
    let mut completed = svm.get_account(&second_order).unwrap();
    let mut decoded =
        bytemuck::allocation::zeroed_box::<gmsol_programs::gmsol_store::accounts::Order>();
    bytemuck::bytes_of_mut(&mut *decoded).copy_from_slice(&completed.data[8..]);
    let mut clock = svm.get_sysvar::<anchor_lang::prelude::Clock>();
    clock.slot = 100;
    svm.set_sysvar(&clock);
    decoded.header.action_state = 1;
    decoded.header.updated_at_slot = 1;
    completed.data[8..].copy_from_slice(bytemuck::bytes_of(&*decoded));
    svm.set_account(second_order, completed).unwrap();
    // Authentic completed order with unchanged trade state must still fail.
    let error = expect_failure(svm, admin, reconciliation.clone());
    assert!(error.contains("UnprovenOrderExecution"), "{error}");
    let mut executed = svm.get_account(&position).unwrap();
    typed.state.trade_id = 9;
    typed.state.updated_at_slot = 1;
    typed.state.size_in_usd = baseline.size_before + baseline.expected_size_delta;
    typed.state.collateral_amount = 100_000_000;
    executed.data[8..].copy_from_slice(bytemuck::bytes_of(&*typed));
    svm.set_account(position, executed).unwrap();
    // Model an execution that consumed collateral but left one USDC funding credit.
    let mut escrow_account = svm.get_account(&second_escrow).unwrap();
    let mut escrow_tokens = spl_token::state::Account::unpack(&escrow_account.data).unwrap();
    escrow_tokens.amount = 1_000_000;
    spl_token::state::Account::pack(escrow_tokens, &mut escrow_account.data).unwrap();
    svm.set_account(second_escrow, escrow_account).unwrap();
    let source_before = token_amount(svm, source);
    let custody_before = token_amount(svm, custody);
    let keeper = Keypair::new();
    svm.airdrop(&keeper.pubkey(), 1_000_000_000).unwrap();
    let mut permissionless_reconciliation = reconciliation.clone();
    permissionless_reconciliation.accounts[0].pubkey = keeper.pubkey();
    send(svm, &keeper, permissionless_reconciliation.clone());
    assert_eq!(token_amount(svm, source), source_before + 1_000_000);
    assert_eq!(token_amount(svm, custody), custody_before);
    assert_uncreated(svm, pending);
    assert_uncreated(svm, second_baseline);
    assert_uncreated(svm, second_order);
    assert_uncreated(svm, second_escrow);
    expect_failure(svm, &keeper, permissionless_reconciliation); // Closed baseline/order cannot be replayed.
    println!("Reconciliation checked persisted baseline and swept execution funding credit through real GMTrade close CPI");
    // Decrease orders use the actual captured GMTrade create/set-keep/close CPIs.
    // Execution results are still controlled fixtures, not oracle-backed fills.
    let index_price = Pubkey::new_unique();
    let usdc_price = Pubkey::new_unique();
    let current_clock = svm.get_sysvar::<anchor_lang::prelude::Clock>();
    for (oracle_key, feed, price) in [
        (
            index_price,
            xau_carry_vault::strategy_nav::XAU_FEED,
            3_000_000_000i64,
        ),
        (
            usdc_price,
            xau_carry_vault::strategy_nav::USDC_FEED,
            1_000_000,
        ),
    ] {
        let mut d = vec![0u8; 134];
        d[..8].copy_from_slice(&[34, 241, 35, 99, 157, 126, 244, 205]);
        d[40] = 1;
        for i in 0..32 {
            d[41 + i] = u8::from_str_radix(&feed[i * 2..i * 2 + 2], 16).unwrap();
        }
        d[73..81].copy_from_slice(&price.to_le_bytes());
        d[89..93].copy_from_slice(&(-6i32).to_le_bytes());
        d[93..101].copy_from_slice(&current_clock.unix_timestamp.to_le_bytes());
        d[125..133].copy_from_slice(&current_clock.slot.to_le_bytes());
        svm.set_account(
            oracle_key,
            Account {
                lamports: 10_000_000,
                data: d,
                owner: key("rec2HHDDnjLfj4kE7VyEtFA1HPGQLK33259532cRyHp"),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    }
    for (iteration, full, cancel) in [
        (0u8, false, true),
        (1, false, false),
        (2, true, false),
        (3, false, false),
    ] {
        let nonce = [60 + iteration; 32];
        let (order, _) = order_address(&external_program, &store, &authority, &nonce);
        let escrow = anchor_spl::associated_token::get_associated_token_address(&order, &usdc);
        let baseline_address =
            Pubkey::find_program_address(&[DECREASE_BASELINE_SEED, order.as_ref()], &our_program).0;
        let position_before = svm.get_account(&position).unwrap();
        let mut state =
            bytemuck::allocation::zeroed_box::<gmsol_programs::gmsol_store::accounts::Position>();
        bytemuck::bytes_of_mut(&mut *state).copy_from_slice(&position_before.data[8..]);
        let forced = iteration == 3;
        if forced {
            state.state.size_in_usd = 100 * 10u128.pow(20);
            state.state.size_in_tokens = 100_000;
            state.state.collateral_amount = 10_000_000;
            let mut reset = position_before.clone();
            reset.data[8..].copy_from_slice(bytemuck::bytes_of(&*state));
            svm.set_account(position, reset).unwrap();
        }
        let delta = if full {
            state.state.size_in_usd
        } else {
            50 * 10u128.pow(20)
        };
        let mut create = Instruction {
            program_id: our_program,
            accounts: xau_carry_vault::accounts::CreateShortDecreaseV1 {
                admin: admin.pubkey(),
                vault_v1_state: vault,
                strategy_v1_state: strategy,
                usdc_mint: usdc,
                vault_v1_usdc_account: custody,
                strategy_authority: authority,
                strategy_usdc_source: source,
                perpetual_program: external_program,
                perpetual_store: store,
                perpetual_market: market,
                perpetual_user: external_user,
                perpetual_position: position,
                perpetual_order: order,
                order_usdc_escrow: escrow,
                pending_short_order: pending,
                order_baseline: baseline_address,
                perpetual_event_authority: event_authority,
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::CreateShortDecreaseV1 {
                nonce,
                params: xau_carry_vault::ShortDecreaseParamsV1 {
                    collateral_withdrawal: if full { 0 } else { 10_000_000 },
                    size_delta_value: delta,
                    acceptable_price: 3_000_000_000_000_000,
                    min_usdc_out: if full { 0 } else { 8_000_000 },
                    allow_full_close: false,
                    execution_lamports: 50_000_000,
                    funding_lamports: 100_000_000,
                },
            }
            .data(),
        };
        create.accounts.extend([
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(index_price, false),
            anchor_lang::solana_program::instruction::AccountMeta::new_readonly(usdc_price, false),
        ]);
        send(svm, admin, create);
        let saved = svm.get_account(&baseline_address).unwrap();
        let baseline =
            xau_carry_vault::DecreaseBaselineV1::try_deserialize(&mut &saved.data[..]).unwrap();
        assert_eq!(baseline.size_before, state.state.size_in_usd);
        assert_eq!(baseline.trade_id_before, state.state.trade_id);
        let custody_before = token_amount(svm, custody);
        let source_before = token_amount(svm, source);
        assert_eq!(token_amount(svm, escrow), 0);
        let recover = Instruction {
            program_id: our_program,
            accounts: xau_carry_vault::accounts::RecoverShortDecreaseV1 {
                keeper: keeper.pubkey(),
                vault_v1_state: vault,
                strategy_v1_state: strategy,
                pending_short_order: pending,
                order_baseline: baseline_address,
                refund: admin.pubkey(),
                usdc_mint: usdc,
                vault_v1_usdc_account: custody,
                strategy_authority: authority,
                strategy_usdc_source: source,
                perpetual_program: external_program,
                perpetual_store: store,
                perpetual_store_wallet: store_wallet,
                perpetual_user: external_user,
                perpetual_order: order,
                order_usdc_escrow: escrow,
                perpetual_event_authority: event_authority,
                perpetual_market: market,
                perpetual_position: position,
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::ReconcileShortDecreaseV1 {}.data(),
        };
        if cancel {
            let mut cancellation = recover.clone();
            cancellation.data = xau_carry_vault::instruction::CancelShortDecreaseV1 {}.data();
            assert!(expect_failure(svm, &keeper, cancellation.clone())
                .contains("UnauthorizedStrategyAdmin"));
            cancellation.accounts[0].pubkey = admin.pubkey();
            send(svm, admin, cancellation);
            assert_eq!(token_amount(svm, custody), custody_before + source_before);
            assert_eq!(token_amount(svm, source), 0);
            continue;
        }
        assert!(expect_failure(svm, &keeper, recover.clone()).contains("UnprovenOrderExecution"));
        let mut completed = svm.get_account(&order).unwrap();
        let mut order_state =
            bytemuck::allocation::zeroed_box::<gmsol_programs::gmsol_store::accounts::Order>();
        bytemuck::bytes_of_mut(&mut *order_state).copy_from_slice(&completed.data[8..]);
        order_state.header.action_state = 1;
        order_state.header.updated_at_slot = current_clock.slot;
        completed.data[8..].copy_from_slice(bytemuck::bytes_of(&*order_state));
        svm.set_account(order, completed).unwrap();
        assert!(expect_failure(svm, &keeper, recover.clone()).contains("UnprovenOrderExecution"));
        let mut updated = svm.get_account(&position).unwrap();
        state.state.trade_id += 1;
        state.state.updated_at_slot = current_clock.slot;
        state.state.size_in_usd -= delta;
        if full || forced {
            state.state.size_in_usd = 0;
            state.state.size_in_tokens = 0;
            state.state.collateral_amount = 0;
        } else {
            state.state.collateral_amount -= 10_000_000;
        }
        updated.data[8..].copy_from_slice(bytemuck::bytes_of(&*state));
        svm.set_account(position, updated).unwrap();
        let payout = if full { 20_000_000 } else { 9_000_000 };
        let mut escrow_account = svm.get_account(&escrow).unwrap();
        let mut t = spl_token::state::Account::unpack(&escrow_account.data).unwrap();
        t.amount = payout;
        spl_token::state::Account::pack(t, &mut escrow_account.data).unwrap();
        svm.set_account(escrow, escrow_account).unwrap();
        if forced {
            assert!(
                expect_failure(svm, &keeper, recover.clone()).contains("UnprovenOrderExecution")
            );
            let mut accept = recover.clone();
            accept.data = xau_carry_vault::instruction::AcceptFullCloseDecreaseV1 {}.data();
            assert!(
                expect_failure(svm, &keeper, accept.clone()).contains("UnauthorizedStrategyAdmin")
            );
            accept.accounts[0].pubkey = admin.pubkey();
            send(svm, admin, accept);
        } else {
            send(svm, &keeper, recover.clone());
        }
        assert_eq!(
            token_amount(svm, custody),
            custody_before + source_before + payout
        );
        assert_eq!(token_amount(svm, source), 0);
        assert_uncreated(svm, pending);
        assert_uncreated(svm, baseline_address);
        assert_uncreated(svm, order);
        assert_uncreated(svm, escrow);
        expect_failure(svm, &keeper, recover);
    }
    println!("Partial/full decrease reconciliation returned proceeds to custody; pending cancellation requires admin");
}
