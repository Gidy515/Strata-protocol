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
    let message = Message::new_with_blockhash(
        &[instruction],
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
    let message = Message::new_with_blockhash(
        &[instruction],
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

    let mut second_instruction = instruction;

    for account in &mut second_instruction.accounts {
        if account.pubkey == order {
            account.pubkey = second_order;
        } else if account.pubkey == escrow {
            account.pubkey = second_escrow;
        }
    }

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

    let error = expect_failure(svm, admin, second_instruction);

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

    println!("Order cancellation returned collateral and cleared pending state");
}
