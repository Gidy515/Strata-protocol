use {
    anchor_lang::{
        prelude::Pubkey, solana_program::system_program, AccountDeserialize, AccountSerialize,
        InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::get_associated_token_address,
        token::spl_token::{
            self,
            state::{Account as SplTokenAccount, AccountState, Mint as SplMint},
        },
    },
    litesvm::LiteSVM,
    solana_account::Account,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_program_option::COption,
    solana_program_pack::Pack,
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const INITIAL_USDC: u64 = 100_000_000; // 100 USDC
const DEPOSIT_AMOUNT: u64 = 10_000_000; // 10 USDC

struct Fixture {
    svm: LiteSVM,
    admin: Keypair,
    user: Keypair,
    program_id: Pubkey,
    program_data: Pubkey,
    usdc_mint: Pubkey,
    vault_state: Pubkey,
    vault_usdc: Pubkey,
    vxau_mint: Pubkey,
    user_usdc: Pubkey,
    user_vxau: Pubkey,
}

impl Fixture {
    fn new(usdc_decimals: u8) -> Self {
        Self::new_with_mint(usdc_decimals, Keypair::new().pubkey())
    }
    fn new_with_mint(usdc_decimals: u8, usdc_mint: Pubkey) -> Self {
        let program_id = xau_carry_vault::id();
        let admin = Keypair::new();
        let user = Keypair::new();

        let mut svm = LiteSVM::new();

        let program_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/deploy/xau_carry_vault.so");

        let program_bytes =
            std::fs::read(&program_path).expect("Run anchor build before running these tests");

        svm.add_program(program_id, &program_bytes)
            .expect("Failed to load Vault 1");

        svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
        svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();

        // LiteSVM loads this program under the upgradeable loader.
        // Supply the admin authority required by our initializer.
        let loader_id: Pubkey = "BPFLoaderUpgradeab1e11111111111111111111111"
            .parse()
            .unwrap();

        let program_data = Pubkey::find_program_address(&[program_id.as_ref()], &loader_id).0;

        let mut program_data_account = svm
            .get_account(&program_data)
            .expect("Upgradeable ProgramData account is missing");

        // UpgradeableLoaderState::ProgramData metadata:
        // variant: u32, slot: u64, authority option: u8,
        // authority public key: 32 bytes.
        assert!(program_data_account.data.len() >= 45);
        assert_eq!(&program_data_account.data[..4], &3_u32.to_le_bytes(),);

        program_data_account.data[12] = 1;
        program_data_account.data[13..45].copy_from_slice(admin.pubkey().as_ref());

        svm.set_account(program_data, program_data_account).unwrap();

        let mint_state = SplMint {
            mint_authority: COption::Some(admin.pubkey()),
            supply: INITIAL_USDC,
            decimals: usdc_decimals,
            is_initialized: true,
            freeze_authority: COption::None,
        };

        let mut mint_data = vec![0; SplMint::LEN];
        SplMint::pack(mint_state, &mut mint_data).unwrap();

        svm.set_account(
            usdc_mint,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(mint_data.len()),
                data: mint_data,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        let vault_state =
            Pubkey::find_program_address(&[b"vault_v1", usdc_mint.as_ref()], &program_id).0;

        let vault_usdc =
            Pubkey::find_program_address(&[b"token_vault", vault_state.as_ref()], &program_id).0;

        let vxau_mint =
            Pubkey::find_program_address(&[b"vxau_mint", vault_state.as_ref()], &program_id).0;

        let user_usdc = get_associated_token_address(&user.pubkey(), &usdc_mint);

        let user_vxau = get_associated_token_address(&user.pubkey(), &vxau_mint);

        let token_state = SplTokenAccount {
            mint: usdc_mint,
            owner: user.pubkey(),
            amount: INITIAL_USDC,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        };

        let mut token_data = vec![0; SplTokenAccount::LEN];
        SplTokenAccount::pack(token_state, &mut token_data).unwrap();

        svm.set_account(
            user_usdc,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(token_data.len()),
                data: token_data,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        Self {
            svm,
            admin,
            user,
            program_id,
            program_data,
            usdc_mint,
            vault_state,
            vault_usdc,
            vxau_mint,
            user_usdc,
            user_vxau,
        }
    }

    fn initialize(&mut self, signer: &Keypair) -> Result<(), String> {
        let instruction = anchor_lang::solana_program::instruction::Instruction {
            program_id: self.program_id,
            accounts: xau_carry_vault::accounts::InitializeVaultV1 {
                admin: signer.pubkey(),
                program: self.program_id,
                program_data: self.program_data,
                usdc_mint: self.usdc_mint,
                vault_v1_state: self.vault_state,
                vault_v1_usdc_account: self.vault_usdc,
                vxau_mint: self.vxau_mint,
                token_program: spl_token::ID,
                system_program: system_program::ID,
                rent: anchor_lang::prelude::rent::ID,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::InitializeVaultV1 {}.data(),
        };

        let message = Message::new_with_blockhash(
            &[instruction],
            Some(&signer.pubkey()),
            &self.svm.latest_blockhash(),
        );

        let transaction =
            VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[signer]).unwrap();

        let result = self
            .svm
            .send_transaction(transaction)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"));

        self.svm.expire_blockhash();
        result
    }

    fn initialize_as_admin(&mut self) {
        // Separate keypair value avoids borrowing self.admin
        // while mutably borrowing the fixture.
        let admin = Keypair::try_from(self.admin.to_bytes().as_slice()).unwrap();

        self.initialize(&admin)
            .expect("Vault initialization failed");
    }

    fn deposit_with_min(&mut self, amount: u64, min_shares_out: u64) -> Result<(), String> {
        let instruction = anchor_lang::solana_program::instruction::Instruction {
            program_id: self.program_id,
            accounts: xau_carry_vault::accounts::DepositUsdcV1 {
                user: self.user.pubkey(),
                vault_v1_state: self.vault_state,
                usdc_mint: self.usdc_mint,
                vault_v1_usdc_account: self.vault_usdc,
                vxau_mint: self.vxau_mint,
                user_usdc_ata: self.user_usdc,
                user_vxau_ata: self.user_vxau,
                strategy_account: self.strategy_address(),
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::DepositUsdcV1 {
                amount,
                min_shares_out,
            }
            .data(),
        };

        let message = Message::new_with_blockhash(
            &[instruction],
            Some(&self.user.pubkey()),
            &self.svm.latest_blockhash(),
        );

        let transaction =
            VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[&self.user])
                .unwrap();

        let result = self
            .svm
            .send_transaction(transaction)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"));

        self.svm.expire_blockhash();
        result
    }

    fn state(&self) -> xau_carry_vault::state::VaultV1State {
        let account = self.svm.get_account(&self.vault_state).unwrap();

        let mut data: &[u8] = &account.data;

        xau_carry_vault::state::VaultV1State::try_deserialize(&mut data).unwrap()
    }

    fn token(&self, address: Pubkey) -> SplTokenAccount {
        let account = self.svm.get_account(&address).unwrap();
        SplTokenAccount::unpack(&account.data).unwrap()
    }

    fn mint(&self, address: Pubkey) -> SplMint {
        let account = self.svm.get_account(&address).unwrap();
        SplMint::unpack(&account.data).unwrap()
    }

    fn assert_no_deposit(&self) {
        assert_eq!(self.token(self.user_usdc).amount, INITIAL_USDC,);
        assert_eq!(self.token(self.vault_usdc).amount, 0);
        assert_eq!(self.state().total_deposited, 0);

        // A failed deposit must also roll back ATA creation.
        assert!(self
            .svm
            .get_account(&self.user_vxau)
            .map_or(true, |account| account.lamports == 0));
    }
}

#[test]
fn initialize_sets_correct_state_and_authorities() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    let state = fixture.state();

    assert_eq!(state.admin, fixture.admin.pubkey());
    assert_eq!(state.usdc_mint, fixture.usdc_mint);
    assert_eq!(state.vxau_mint, fixture.vxau_mint);
    assert_eq!(state.total_deposited, 0);

    let state_account = fixture.svm.get_account(&fixture.vault_state).unwrap();

    assert_eq!(state_account.owner, fixture.program_id);

    let custody = fixture.token(fixture.vault_usdc);
    assert_eq!(custody.mint, fixture.usdc_mint);
    assert_eq!(custody.owner, fixture.vault_state);
    assert_eq!(custody.amount, 0);

    let mint = fixture.mint(fixture.vxau_mint);
    assert_eq!(mint.decimals, 6);
    assert_eq!(mint.supply, 0);
    assert_eq!(mint.mint_authority, COption::Some(fixture.vault_state),);
    assert_eq!(mint.freeze_authority, COption::None);

    // SPL Token owns custody accounts and mints;
    // the vault PDA controls their token authority.
    assert_eq!(
        fixture.svm.get_account(&fixture.vault_usdc).unwrap().owner,
        spl_token::ID,
    );
}

#[test]
fn initialize_rejects_wrong_upgrade_authority() {
    let mut fixture = Fixture::new(6);
    let stranger = Keypair::new();

    fixture
        .svm
        .airdrop(&stranger.pubkey(), 1_000_000_000)
        .unwrap();

    let error = fixture.initialize(&stranger).unwrap_err();

    assert!(
        error.contains("Custom(6000)"),
        "Expected Unauthorized; received: {error}",
    );

    assert!(fixture.svm.get_account(&fixture.vault_state).is_none());
}

#[test]
fn initialize_rejects_wrong_usdc_decimals() {
    let mut fixture = Fixture::new(9);

    let admin = Keypair::try_from(fixture.admin.to_bytes().as_slice()).unwrap();

    let error = fixture.initialize(&admin).unwrap_err();

    assert!(
        error.contains("Custom(6001)"),
        "Expected InvalidDecimals; received: {error}",
    );

    assert!(fixture.svm.get_account(&fixture.vault_state).is_none());
}

#[test]
fn initialize_cannot_reset_existing_vault() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let admin = Keypair::try_from(fixture.admin.to_bytes().as_slice()).unwrap();

    assert!(fixture.initialize(&admin).is_err());

    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.token(fixture.vault_usdc).amount, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, DEPOSIT_AMOUNT,);
}

#[test]
fn deposit_transfers_usdc_and_mints_receipts() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    assert_eq!(
        fixture.token(fixture.user_usdc).amount,
        INITIAL_USDC - DEPOSIT_AMOUNT,
    );
    assert_eq!(fixture.token(fixture.vault_usdc).amount, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.token(fixture.user_vxau).amount, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT,);

    let receipts = fixture.token(fixture.user_vxau);
    assert_eq!(receipts.owner, fixture.user.pubkey());
    assert_eq!(receipts.mint, fixture.vxau_mint);
}

#[test]
fn second_deposit_reuses_receipt_ata() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    fixture.deposit(DEPOSIT_AMOUNT).unwrap();
    fixture.deposit(DEPOSIT_AMOUNT * 2).unwrap();

    let total = DEPOSIT_AMOUNT * 3;

    assert_eq!(
        fixture.token(fixture.user_usdc).amount,
        INITIAL_USDC - total,
    );
    assert_eq!(fixture.token(fixture.vault_usdc).amount, total);
    assert_eq!(fixture.token(fixture.user_vxau).amount, total);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, total);
    assert_eq!(fixture.state().total_deposited, total);
}

#[test]
fn deposit_rejects_zero_amount() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    let error = fixture.deposit(0).unwrap_err();

    assert!(
        error.contains("Custom(6002)"),
        "Expected ZeroAmount; received: {error}",
    );

    fixture.assert_no_deposit();
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, 0);
}

#[test]
fn deposit_rejects_insufficient_usdc() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    assert!(fixture.deposit(INITIAL_USDC + 1).is_err());

    fixture.assert_no_deposit();
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, 0);
}

#[test]
fn failed_receipt_mint_rolls_back_usdc_transfer() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let mut account = fixture.svm.get_account(&fixture.user_vxau).unwrap();
    let mut receipts = SplTokenAccount::unpack(&account.data).unwrap();

    receipts.state = AccountState::Frozen;
    SplTokenAccount::pack(receipts, &mut account.data).unwrap();

    fixture.svm.set_account(fixture.user_vxau, account).unwrap();

    let error = fixture.deposit(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("Instruction: MintTo") && error.contains("frozen"),
        "Expected receipt mint failure after transfer: {error}",
    );

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);
    assert_eq!(fixture.token(fixture.user_vxau).state, AccountState::Frozen,);
}

#[test]
fn deposit_rejects_substituted_custody_account() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    let legitimate_custody = fixture.vault_usdc;
    let substituted_custody = Keypair::new().pubkey();

    // Valid SPL account with the correct mint and authority,
    // but the wrong address. This isolates the PDA seed check.
    let custody_account = fixture.svm.get_account(&legitimate_custody).unwrap();

    fixture
        .svm
        .set_account(substituted_custody, custody_account)
        .unwrap();

    fixture.vault_usdc = substituted_custody;

    let error = fixture.deposit(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("ConstraintSeeds"),
        "Expected custody PDA rejection; received: {error}",
    );

    fixture.vault_usdc = legitimate_custody;

    fixture.assert_no_deposit();
    assert_eq!(fixture.token(substituted_custody).amount, 0,);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, 0,);
}

#[test]
fn deposit_rejects_unrelated_receipt_mint() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    let legitimate_mint = fixture.vxau_mint;
    let legitimate_receipt_ata = fixture.user_vxau;
    let substituted_mint = Keypair::new().pubkey();

    // Correct decimals and authority, but not the mint
    // recorded in VaultV1State.
    let mint_account = fixture.svm.get_account(&legitimate_mint).unwrap();

    fixture
        .svm
        .set_account(substituted_mint, mint_account)
        .unwrap();

    let substituted_receipt_ata =
        get_associated_token_address(&fixture.user.pubkey(), &substituted_mint);

    fixture.vxau_mint = substituted_mint;
    fixture.user_vxau = substituted_receipt_ata;

    let error = fixture.deposit(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("ConstraintHasOne") || error.contains("ConstraintSeeds"),
        "Expected receipt mint relationship rejection; received: {error}",
    );

    fixture.vxau_mint = legitimate_mint;
    fixture.user_vxau = legitimate_receipt_ata;

    fixture.assert_no_deposit();

    assert_eq!(fixture.mint(legitimate_mint).supply, 0);
    assert_eq!(fixture.mint(substituted_mint).supply, 0);

    assert!(fixture
        .svm
        .get_account(&substituted_receipt_ata)
        .map_or(true, |account| account.lamports == 0));
}

#[test]
fn deposit_rejects_another_users_source_account() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    let legitimate_source = fixture.user_usdc;
    let other_user = Keypair::new();

    let other_source = get_associated_token_address(&other_user.pubkey(), &fixture.usdc_mint);

    let mut source_account = fixture.svm.get_account(&legitimate_source).unwrap();

    let mut source = SplTokenAccount::unpack(&source_account.data).unwrap();

    source.owner = other_user.pubkey();

    SplTokenAccount::pack(source, &mut source_account.data).unwrap();

    fixture
        .svm
        .set_account(other_source, source_account)
        .unwrap();

    fixture.user_usdc = other_source;

    let error = fixture.deposit(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("ConstraintTokenOwner") || error.contains("ConstraintAssociated"),
        "Expected source ownership/ATA rejection; received: {error}",
    );

    fixture.user_usdc = legitimate_source;

    fixture.assert_no_deposit();

    assert_eq!(fixture.token(other_source).amount, INITIAL_USDC,);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, 0,);
}

#[test]
fn deposit_rejects_cumulative_accounting_overflow() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    // Inject an extreme accounting value to exercise checked_add.
    let mut state = fixture.state();
    state.total_deposited = u64::MAX;

    let mut state_account = fixture.svm.get_account(&fixture.vault_state).unwrap();

    {
        let mut destination: &mut [u8] = state_account.data.as_mut_slice();

        state.try_serialize(&mut destination).unwrap();
    }

    fixture
        .svm
        .set_account(fixture.vault_state, state_account)
        .unwrap();

    let error = fixture.deposit(1).unwrap_err();

    assert!(
        error.contains("Custom(6003)"),
        "Expected Overflow; received: {error}",
    );

    assert_eq!(fixture.state().total_deposited, u64::MAX,);
    assert_eq!(fixture.token(fixture.user_usdc).amount, INITIAL_USDC,);
    assert_eq!(fixture.token(fixture.vault_usdc).amount, 0);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, 0);

    assert!(fixture
        .svm
        .get_account(&fixture.user_vxau)
        .map_or(true, |account| account.lamports == 0));
}

#[test]
fn multiple_users_receive_correct_receipts() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    let first_source = fixture.user_usdc;
    let first_receipt = fixture.user_vxau;

    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let second_user = Keypair::new();

    fixture
        .svm
        .airdrop(&second_user.pubkey(), 10_000_000_000)
        .unwrap();

    let second_source = get_associated_token_address(&second_user.pubkey(), &fixture.usdc_mint);

    let second_receipt = get_associated_token_address(&second_user.pubkey(), &fixture.vxau_mint);

    // Fund the second user's mock USDC account.
    let mut source_account = fixture.svm.get_account(&first_source).unwrap();

    let mut source = SplTokenAccount::unpack(&source_account.data).unwrap();

    source.owner = second_user.pubkey();
    source.amount = INITIAL_USDC;

    SplTokenAccount::pack(source, &mut source_account.data).unwrap();

    fixture
        .svm
        .set_account(second_source, source_account)
        .unwrap();

    // Keep the mock USDC mint supply consistent with funding.
    let mut mint_account = fixture.svm.get_account(&fixture.usdc_mint).unwrap();

    let mut mint = SplMint::unpack(&mint_account.data).unwrap();

    mint.supply = mint.supply.checked_add(INITIAL_USDC).unwrap();

    SplMint::pack(mint, &mut mint_account.data).unwrap();

    fixture
        .svm
        .set_account(fixture.usdc_mint, mint_account)
        .unwrap();

    fixture.user = second_user;
    fixture.user_usdc = second_source;
    fixture.user_vxau = second_receipt;

    let second_amount = DEPOSIT_AMOUNT * 2;

    fixture.deposit(second_amount).unwrap();

    let total = DEPOSIT_AMOUNT + second_amount;

    assert_eq!(
        fixture.token(first_source).amount,
        INITIAL_USDC - DEPOSIT_AMOUNT,
    );
    assert_eq!(
        fixture.token(second_source).amount,
        INITIAL_USDC - second_amount,
    );

    assert_eq!(fixture.token(first_receipt).amount, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.token(second_receipt).amount, second_amount,);

    assert_eq!(fixture.token(second_receipt).owner, fixture.user.pubkey(),);

    assert_eq!(fixture.token(fixture.vault_usdc).amount, total);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, total);
    assert_eq!(fixture.state().total_deposited, total);

    assert_eq!(
        fixture.token(first_receipt).amount + fixture.token(second_receipt).amount,
        fixture.mint(fixture.vxau_mint).supply,
    );
}

impl Fixture {
    fn withdraw_with_min(&mut self, amount: u64, min_assets_out: u64) -> Result<(), String> {
        let instruction = anchor_lang::solana_program::instruction::Instruction {
            program_id: self.program_id,
            accounts: xau_carry_vault::accounts::WithdrawUsdcV1 {
                user: self.user.pubkey(),
                vault_v1_state: self.vault_state,
                usdc_mint: self.usdc_mint,
                vault_v1_usdc_account: self.vault_usdc,
                vxau_mint: self.vxau_mint,
                user_vxau_ata: self.user_vxau,
                user_usdc_ata: self.user_usdc,
                strategy_account: self.strategy_address(),
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::WithdrawUsdcV1 {
                amount,
                min_assets_out,
            }
            .data(),
        };

        let message = Message::new_with_blockhash(
            &[instruction],
            Some(&self.user.pubkey()),
            &self.svm.latest_blockhash(),
        );

        let transaction =
            VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[&self.user])
                .unwrap();

        let result = self
            .svm
            .send_transaction(transaction)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"));

        self.svm.expire_blockhash();
        result
    }

    fn assert_deposit_intact(&self, amount: u64) {
        assert_eq!(self.token(self.user_usdc).amount, INITIAL_USDC - amount,);
        assert_eq!(self.token(self.vault_usdc).amount, amount);
        assert_eq!(self.token(self.user_vxau).amount, amount);
        assert_eq!(self.mint(self.vxau_mint).supply, amount);
        assert_eq!(self.state().total_deposited, amount);
    }
}

#[test]
fn withdraw_partial_burns_receipts_and_returns_usdc() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let redeemed = DEPOSIT_AMOUNT / 2;
    fixture.withdraw(redeemed).unwrap();

    let remaining = DEPOSIT_AMOUNT - redeemed;

    assert_eq!(
        fixture.token(fixture.user_usdc).amount,
        INITIAL_USDC - remaining,
    );
    assert_eq!(fixture.token(fixture.vault_usdc).amount, remaining);
    assert_eq!(fixture.token(fixture.user_vxau).amount, remaining);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, remaining);

    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT,);
}

#[test]
fn withdraw_full_then_deposit_again() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    fixture.withdraw(DEPOSIT_AMOUNT).unwrap();

    assert_eq!(fixture.token(fixture.user_usdc).amount, INITIAL_USDC,);
    assert_eq!(fixture.token(fixture.vault_usdc).amount, 0);
    assert_eq!(fixture.token(fixture.user_vxau).amount, 0);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, 0);
    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT,);

    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    assert_eq!(fixture.token(fixture.vault_usdc).amount, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.token(fixture.user_vxau).amount, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT * 2,);
}

#[test]
fn withdraw_rejects_zero_amount() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let error = fixture.withdraw(0).unwrap_err();

    assert!(
        error.contains("Custom(6002)"),
        "Expected ZeroAmount; received: {error}",
    );

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn withdraw_rejects_insufficient_receipts() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let error = fixture.withdraw(DEPOSIT_AMOUNT + 1).unwrap_err();

    assert!(
        error.contains("Custom(6004)"),
        "Expected InsufficientReceipts; received: {error}",
    );

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn withdraw_prices_custody_loss_proportionally() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let remaining_assets = DEPOSIT_AMOUNT - 1;
    fixture.set_custody_amount(remaining_assets);

    // Redeeming all shares returns all remaining assets.
    fixture
        .withdraw_with_min(DEPOSIT_AMOUNT, remaining_assets)
        .unwrap();

    assert_eq!(fixture.token(fixture.user_usdc).amount, INITIAL_USDC - 1,);
    assert_eq!(fixture.token(fixture.vault_usdc).amount, 0);
    assert_eq!(fixture.token(fixture.user_vxau).amount, 0);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, 0);
    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT);
}

#[test]
fn failed_usdc_transfer_rolls_back_receipt_burn() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    // Simulate a frozen USDC destination. The receipt burn can
    // succeed, but the subsequent transfer must fail.
    let mut account = fixture.svm.get_account(&fixture.user_usdc).unwrap();

    let mut destination = SplTokenAccount::unpack(&account.data).unwrap();

    destination.state = AccountState::Frozen;

    SplTokenAccount::pack(destination, &mut account.data).unwrap();

    fixture.svm.set_account(fixture.user_usdc, account).unwrap();

    assert!(fixture.withdraw(DEPOSIT_AMOUNT).is_err());

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);

    assert_eq!(fixture.token(fixture.user_usdc).state, AccountState::Frozen,);
}

#[test]
fn withdraw_rejects_substituted_custody_account() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let legitimate_custody = fixture.vault_usdc;
    let substituted_custody = Keypair::new().pubkey();

    let account = fixture.svm.get_account(&legitimate_custody).unwrap();

    fixture
        .svm
        .set_account(substituted_custody, account)
        .unwrap();

    fixture.vault_usdc = substituted_custody;

    let error = fixture.withdraw(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("ConstraintSeeds"),
        "Expected custody PDA rejection; received: {error}",
    );

    fixture.vault_usdc = legitimate_custody;
    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);

    assert_eq!(fixture.token(substituted_custody).amount, DEPOSIT_AMOUNT,);
}

#[test]
fn withdraw_rejects_another_users_receipt_account() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let legitimate_receipt = fixture.user_vxau;
    let other_user = Keypair::new();

    let other_receipt = get_associated_token_address(&other_user.pubkey(), &fixture.vxau_mint);

    let mut account = fixture.svm.get_account(&legitimate_receipt).unwrap();

    let mut receipt = SplTokenAccount::unpack(&account.data).unwrap();

    receipt.owner = other_user.pubkey();

    SplTokenAccount::pack(receipt, &mut account.data).unwrap();

    fixture.svm.set_account(other_receipt, account).unwrap();

    fixture.user_vxau = other_receipt;

    let error = fixture.withdraw(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("ConstraintTokenOwner") || error.contains("ConstraintAssociated"),
        "Expected receipt ownership rejection; received: {error}",
    );

    fixture.user_vxau = legitimate_receipt;
    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);

    assert_eq!(fixture.token(other_receipt).amount, DEPOSIT_AMOUNT,);
}

#[test]
fn withdraw_rejects_another_users_usdc_destination() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let legitimate_destination = fixture.user_usdc;
    let other_user = Keypair::new();

    let other_destination = get_associated_token_address(&other_user.pubkey(), &fixture.usdc_mint);

    let mut account = fixture.svm.get_account(&legitimate_destination).unwrap();

    let mut destination = SplTokenAccount::unpack(&account.data).unwrap();

    destination.owner = other_user.pubkey();
    destination.amount = 0;

    SplTokenAccount::pack(destination, &mut account.data).unwrap();

    fixture.svm.set_account(other_destination, account).unwrap();

    fixture.user_usdc = other_destination;

    let error = fixture.withdraw(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("ConstraintTokenOwner") || error.contains("ConstraintAssociated"),
        "Expected destination ownership rejection; received: {error}",
    );

    fixture.user_usdc = legitimate_destination;
    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);

    assert_eq!(fixture.token(other_destination).amount, 0);
}

impl Fixture {
    fn strategy_address(&self) -> Pubkey {
        Pubkey::find_program_address(
            &[
                xau_carry_vault::constants::STRATEGY_SEED,
                self.vault_state.as_ref(),
            ],
            &self.program_id,
        )
        .0
    }

    fn read_idle_nav_at(
        &mut self,
        strategy_account: Pubkey,
    ) -> Result<xau_carry_vault::IdleNavQuoteV1, String> {
        use anchor_lang::AnchorDeserialize;

        let instruction = anchor_lang::solana_program::instruction::Instruction {
            program_id: self.program_id,
            accounts: xau_carry_vault::accounts::ReadIdleNavV1 {
                vault_v1_state: self.vault_state,
                usdc_mint: self.usdc_mint,
                vault_v1_usdc_account: self.vault_usdc,
                vxau_mint: self.vxau_mint,
                strategy_account,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::ReadIdleNavV1 {}.data(),
        };

        // The instruction has no signer requirement.
        // The user signs only as the transaction fee payer.
        let message = Message::new_with_blockhash(
            &[instruction],
            Some(&self.user.pubkey()),
            &self.svm.latest_blockhash(),
        );

        let transaction =
            VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[&self.user])
                .unwrap();

        let result = self.svm.send_transaction(transaction);
        self.svm.expire_blockhash();

        let metadata = result.map_err(|error| format!("{error:?}"))?;

        if metadata.return_data.program_id != self.program_id {
            return Err(format!(
                "Unexpected return-data program: {}",
                metadata.return_data.program_id,
            ));
        }

        xau_carry_vault::IdleNavQuoteV1::try_from_slice(&metadata.return_data.data)
            .map_err(|error| format!("Invalid NAV return data: {error}"))
    }

    fn read_idle_nav(&mut self) -> Result<xau_carry_vault::IdleNavQuoteV1, String> {
        let strategy = self.strategy_address();
        self.read_idle_nav_at(strategy)
    }

    fn inject_disabled_strategy(&mut self) {
        let strategy = xau_carry_vault::StrategyV1State {
            vault: self.vault_state,
            gold_mint: Pubkey::new_unique(),
            gold_custody: Pubkey::new_unique(),
            gold_token_program: spl_token::ID,
            perpetual_program: Pubkey::new_unique(),
            perpetual_store: Pubkey::new_unique(),
            perpetual_market: Pubkey::new_unique(),
            configured_at_slot: 0,
            execution_enabled: false,
            bump: Pubkey::find_program_address(
                &[
                    xau_carry_vault::constants::STRATEGY_SEED,
                    self.vault_state.as_ref(),
                ],
                &self.program_id,
            )
            .1,
        };

        let mut data = Vec::new();
        strategy.try_serialize(&mut data).unwrap();

        let address = self.strategy_address();

        self.svm
            .set_account(
                address,
                Account {
                    lamports: self.svm.minimum_balance_for_rent_exemption(data.len()),
                    data,
                    owner: self.program_id,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    }
}

#[test]
fn idle_nav_reports_empty_vault() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    // Leave the canonical strategy PDA nonexistent.
    // This also exercises reading an absent account.
    assert!(fixture
        .svm
        .get_account(&fixture.strategy_address())
        .is_none());

    let quote = fixture.read_idle_nav().unwrap();

    assert_eq!(quote.net_assets, 0);
    assert_eq!(quote.share_supply, 0);
}

#[test]
fn idle_nav_reads_current_balances_after_redemption() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let redeemed = DEPOSIT_AMOUNT / 2;
    fixture.withdraw(redeemed).unwrap();

    // Historical deposits remain unchanged after redemption.
    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT);

    let state_before = fixture.svm.get_account(&fixture.vault_state).unwrap();
    let custody_before = fixture.svm.get_account(&fixture.vault_usdc).unwrap();
    let mint_before = fixture.svm.get_account(&fixture.vxau_mint).unwrap();

    let quote = fixture.read_idle_nav().unwrap();

    assert_eq!(quote.net_assets, DEPOSIT_AMOUNT - redeemed);
    assert_eq!(quote.share_supply, DEPOSIT_AMOUNT - redeemed);

    // Quoting must not modify vault accounting or token accounts.
    for (address, before) in [
        (fixture.vault_state, state_before),
        (fixture.vault_usdc, custody_before),
        (fixture.vxau_mint, mint_before),
    ] {
        let after = fixture.svm.get_account(&address).unwrap();
        assert_eq!(after.data, before.data);
        assert_eq!(after.lamports, before.lamports);
        assert_eq!(after.owner, before.owner);
    }
}

#[test]
fn idle_nav_includes_unsolicited_custody_tokens() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let donation = DEPOSIT_AMOUNT / 2;

    // Move mock USDC from the user into custody without minting
    // receipts. Total USDC supply remains unchanged.
    let mut source_account = fixture.svm.get_account(&fixture.user_usdc).unwrap();
    let mut source = SplTokenAccount::unpack(&source_account.data).unwrap();

    source.amount = source.amount.checked_sub(donation).unwrap();
    SplTokenAccount::pack(source, &mut source_account.data).unwrap();

    let mut custody_account = fixture.svm.get_account(&fixture.vault_usdc).unwrap();
    let mut custody = SplTokenAccount::unpack(&custody_account.data).unwrap();

    custody.amount = custody.amount.checked_add(donation).unwrap();
    SplTokenAccount::pack(custody, &mut custody_account.data).unwrap();

    fixture
        .svm
        .set_account(fixture.user_usdc, source_account)
        .unwrap();
    fixture
        .svm
        .set_account(fixture.vault_usdc, custody_account)
        .unwrap();

    let quote = fixture.read_idle_nav().unwrap();

    assert_eq!(quote.net_assets, DEPOSIT_AMOUNT + donation);
    assert_eq!(quote.share_supply, DEPOSIT_AMOUNT);
    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT);
}

#[test]
fn idle_nav_rejects_configured_strategy_even_when_disabled() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();
    fixture.inject_disabled_strategy();

    let error = fixture.read_idle_nav().unwrap_err();

    assert!(
        error.contains("FullStrategyNavRequired"),
        "Expected full strategy NAV requirement; received: {error}",
    );

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn idle_nav_rejects_substituted_strategy_address() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();

    // An empty system-owned account satisfies the handler's
    // absence check, but must fail the canonical PDA constraint.
    let substituted = Pubkey::new_unique();

    fixture
        .svm
        .set_account(
            substituted,
            Account {
                lamports: fixture.svm.minimum_balance_for_rent_exemption(0),
                data: Vec::new(),
                owner: system_program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let error = fixture.read_idle_nav_at(substituted).unwrap_err();

    assert!(
        error.contains("ConstraintSeeds"),
        "Expected strategy PDA rejection; received: {error}",
    );
}

impl Fixture {
    fn deposit(&mut self, amount: u64) -> Result<(), String> {
        self.deposit_with_min(amount, 1)
    }

    fn withdraw(&mut self, amount: u64) -> Result<(), String> {
        self.withdraw_with_min(amount, 1)
    }

    // Local test-only balance injection.
    fn set_custody_amount(&mut self, amount: u64) {
        let mut account = self.svm.get_account(&self.vault_usdc).unwrap();
        let mut custody = SplTokenAccount::unpack(&account.data).unwrap();

        custody.amount = amount;
        SplTokenAccount::pack(custody, &mut account.data).unwrap();

        self.svm.set_account(self.vault_usdc, account).unwrap();
    }
}

#[test]
fn deposit_prices_shares_against_increased_nav() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    // Existing shares now represent twice their initial assets.
    fixture.set_custody_amount(DEPOSIT_AMOUNT * 2);

    let expected_shares = DEPOSIT_AMOUNT / 2;
    fixture
        .deposit_with_min(DEPOSIT_AMOUNT, expected_shares)
        .unwrap();

    assert_eq!(fixture.token(fixture.vault_usdc).amount, DEPOSIT_AMOUNT * 3,);
    assert_eq!(
        fixture.mint(fixture.vxau_mint).supply,
        DEPOSIT_AMOUNT + expected_shares,
    );
    assert_eq!(
        fixture.token(fixture.user_vxau).amount,
        DEPOSIT_AMOUNT + expected_shares,
    );
    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT * 2,);
}

#[test]
fn deposit_prices_shares_against_reduced_nav() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    fixture.set_custody_amount(DEPOSIT_AMOUNT / 2);

    fixture
        .deposit_with_min(DEPOSIT_AMOUNT, DEPOSIT_AMOUNT * 2)
        .unwrap();

    assert_eq!(
        fixture.token(fixture.vault_usdc).amount,
        DEPOSIT_AMOUNT + DEPOSIT_AMOUNT / 2,
    );
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, DEPOSIT_AMOUNT * 3,);
    assert_eq!(fixture.token(fixture.user_vxau).amount, DEPOSIT_AMOUNT * 3,);
}

#[test]
fn deposit_rejects_output_below_minimum_without_changes() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let error = fixture
        .deposit_with_min(DEPOSIT_AMOUNT, DEPOSIT_AMOUNT + 1)
        .unwrap_err();

    assert!(
        error.contains("MinimumOutputNotMet"),
        "Unexpected error: {error}",
    );

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn withdraw_returns_increased_share_value() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    fixture.set_custody_amount(DEPOSIT_AMOUNT * 2);

    let shares = DEPOSIT_AMOUNT / 2;
    fixture.withdraw_with_min(shares, DEPOSIT_AMOUNT).unwrap();

    assert_eq!(fixture.token(fixture.user_usdc).amount, INITIAL_USDC);
    assert_eq!(fixture.token(fixture.vault_usdc).amount, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.token(fixture.user_vxau).amount, shares);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, shares);
}

#[test]
fn withdraw_rejects_output_below_minimum_without_burning() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let error = fixture
        .withdraw_with_min(DEPOSIT_AMOUNT, DEPOSIT_AMOUNT + 1)
        .unwrap_err();

    assert!(
        error.contains("MinimumOutputNotMet"),
        "Unexpected error: {error}",
    );

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn pricing_rejects_configured_strategy_for_both_handlers() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();
    fixture.inject_disabled_strategy();

    let deposit_error = fixture.deposit(1).unwrap_err();
    let withdrawal_error = fixture.withdraw(1).unwrap_err();

    assert!(
        deposit_error.contains("FullStrategyNavRequired"),
        "Unexpected deposit error: {deposit_error}",
    );
    assert!(
        withdrawal_error.contains("FullStrategyNavRequired"),
        "Unexpected withdrawal error: {withdrawal_error}",
    );

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn deposit_rejects_zero_nav_with_outstanding_shares() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    fixture.set_custody_amount(0);

    let error = fixture.deposit(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("VaultInsolvent"),
        "Unexpected error: {error}",
    );

    assert_eq!(
        fixture.token(fixture.user_usdc).amount,
        INITIAL_USDC - DEPOSIT_AMOUNT,
    );
    assert_eq!(fixture.token(fixture.vault_usdc).amount, 0);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.state().total_deposited, DEPOSIT_AMOUNT);
}

#[test]
fn pricing_requires_positive_minimum_outputs() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let deposit_error = fixture.deposit_with_min(DEPOSIT_AMOUNT, 0).unwrap_err();
    let withdrawal_error = fixture.withdraw_with_min(DEPOSIT_AMOUNT, 0).unwrap_err();

    assert!(deposit_error.contains("InvalidMinimumOutput"));
    assert!(withdrawal_error.contains("InvalidMinimumOutput"));

    fixture.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn withdraw_rejects_output_rounded_to_zero() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    fixture.set_custody_amount(1);

    let error = fixture.withdraw(1).unwrap_err();

    assert!(
        error.contains("RedemptionTooSmall"),
        "Unexpected error: {error}",
    );

    assert_eq!(fixture.token(fixture.vault_usdc).amount, 1);
    assert_eq!(fixture.token(fixture.user_vxau).amount, DEPOSIT_AMOUNT,);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, DEPOSIT_AMOUNT,);
}

#[test]
fn deposit_rejects_assets_without_outstanding_shares() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.set_custody_amount(1);

    let error = fixture.deposit(DEPOSIT_AMOUNT).unwrap_err();

    assert!(
        error.contains("InvalidShareBootstrap"),
        "Unexpected error: {error}",
    );

    assert_eq!(fixture.token(fixture.user_usdc).amount, INITIAL_USDC);
    assert_eq!(fixture.token(fixture.vault_usdc).amount, 1);
    assert_eq!(fixture.mint(fixture.vxau_mint).supply, 0);
    assert_eq!(fixture.state().total_deposited, 0);

    // Receipt ATA creation must also roll back.
    assert!(fixture
        .svm
        .get_account(&fixture.user_vxau)
        .map_or(true, |account| account.lamports == 0));
}

impl Fixture {
    fn run_lifecycle_instruction(
        &mut self,
        instruction: anchor_lang::solana_program::instruction::Instruction,
        admin_payer: bool,
    ) -> Result<(), String> {
        let signer = if admin_payer {
            Keypair::try_from(self.admin.to_bytes().as_slice()).unwrap()
        } else {
            Keypair::try_from(self.user.to_bytes().as_slice()).unwrap()
        };

        let message = Message::new_with_blockhash(
            &[instruction],
            Some(&signer.pubkey()),
            &self.svm.latest_blockhash(),
        );

        let tx =
            VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[&signer]).unwrap();

        let result = self
            .svm
            .send_transaction(tx)
            .map(|_| ())
            .map_err(|error| format!("{error:?}"));

        self.svm.expire_blockhash();
        result
    }

    fn queue_addresses(&self, nonce: u64) -> (Pubkey, Pubkey) {
        let request = Pubkey::find_program_address(
            &[
                xau_carry_vault::constants::WITHDRAWAL_REQUEST_SEED,
                self.vault_state.as_ref(),
                self.user.pubkey().as_ref(),
                &nonce.to_le_bytes(),
            ],
            &self.program_id,
        )
        .0;

        let escrow = Pubkey::find_program_address(
            &[
                xau_carry_vault::constants::WITHDRAWAL_ESCROW_SEED,
                request.as_ref(),
            ],
            &self.program_id,
        )
        .0;

        (request, escrow)
    }

    fn queue_withdrawal(&mut self, nonce: u64, shares: u64, minimum: u64) -> Result<(), String> {
        let (request, escrow) = self.queue_addresses(nonce);

        let ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: self.program_id,
            accounts: xau_carry_vault::accounts::RequestWithdrawalV1 {
                user: self.user.pubkey(),
                vault_v1_state: self.vault_state,
                vxau_mint: self.vxau_mint,
                user_vxau_ata: self.user_vxau,
                withdrawal_request: request,
                withdrawal_escrow: escrow,
                token_program: spl_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::RequestWithdrawalV1 {
                nonce,
                shares,
                min_assets_out: minimum,
            }
            .data(),
        };

        self.run_lifecycle_instruction(ix, false)
    }

    fn finish_withdrawal(
        &mut self,
        nonce: u64,
        cancel: bool,
        admin_payer: bool,
    ) -> Result<(), String> {
        let (request, escrow) = self.queue_addresses(nonce);
        let payer = if admin_payer {
            self.admin.pubkey()
        } else {
            self.user.pubkey()
        };

        let data = if cancel {
            xau_carry_vault::instruction::CancelWithdrawalV1 {}.data()
        } else {
            xau_carry_vault::instruction::SettleWithdrawalV1 {}.data()
        };

        let ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: self.program_id,
            accounts: xau_carry_vault::accounts::CompleteWithdrawalV1 {
                payer,
                owner: self.user.pubkey(),
                vault_v1_state: self.vault_state,
                usdc_mint: self.usdc_mint,
                vxau_mint: self.vxau_mint,
                vault_v1_usdc_account: self.vault_usdc,
                withdrawal_request: request,
                withdrawal_escrow: escrow,
                owner_usdc_ata: self.user_usdc,
                owner_vxau_ata: self.user_vxau,
                strategy_account: self.strategy_address(),
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data,
        };

        self.run_lifecycle_instruction(ix, admin_payer)
    }

    fn set_pauses(
        &mut self,
        deposits: bool,
        withdrawals: bool,
        strategy: bool,
    ) -> Result<(), String> {
        let ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: self.program_id,
            accounts: xau_carry_vault::accounts::ManageVaultV1 {
                admin: self.admin.pubkey(),
                vault_v1_state: self.vault_state,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::SetVaultControlsV1 {
                controls: xau_carry_vault::VaultControlsV1 {
                    deposits_paused: deposits,
                    withdrawals_paused: withdrawals,
                    strategy_paused: strategy,
                    max_order_collateral: u64::MAX,
                    max_order_size: u128::MAX,
                },
            }
            .data(),
        };

        self.run_lifecycle_instruction(ix, true)
    }

    fn recover_unowned(&mut self) -> Result<(), String> {
        let ix = anchor_lang::solana_program::instruction::Instruction {
            program_id: self.program_id,
            accounts: xau_carry_vault::accounts::RecoverUnownedCustodyV1 {
                admin: self.admin.pubkey(),
                vault_v1_state: self.vault_state,
                usdc_mint: self.usdc_mint,
                vxau_mint: self.vxau_mint,
                vault_v1_usdc_account: self.vault_usdc,
                admin_usdc_ata: get_associated_token_address(&self.admin.pubkey(), &self.usdc_mint),
                strategy_account: self.strategy_address(),
                token_program: spl_token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: xau_carry_vault::instruction::RecoverUnownedCustodyV1 {}.data(),
        };

        self.run_lifecycle_instruction(ix, true)
    }

    fn assert_queue_closed(&self, nonce: u64) {
        let (request, escrow) = self.queue_addresses(nonce);

        for key in [request, escrow] {
            assert!(
                self.svm
                    .get_account(&key)
                    .map_or(true, |account| account.lamports == 0),
                "Queue account remained open: {key}",
            );
        }
    }
}

#[test]
fn queue_escrows_shares_without_burning() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.deposit(DEPOSIT_AMOUNT).unwrap();

    f.queue_withdrawal(1, DEPOSIT_AMOUNT / 2, 1).unwrap();

    let (_, escrow) = f.queue_addresses(1);
    assert_eq!(f.token(escrow).amount, DEPOSIT_AMOUNT / 2);
    assert_eq!(f.token(f.user_vxau).amount, DEPOSIT_AMOUNT / 2);
    assert_eq!(f.mint(f.vxau_mint).supply, DEPOSIT_AMOUNT);
    assert_eq!(f.token(f.vault_usdc).amount, DEPOSIT_AMOUNT);
}

#[test]
fn queue_settlement_is_permissionless_and_uses_current_nav() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.deposit(DEPOSIT_AMOUNT).unwrap();
    f.queue_withdrawal(2, DEPOSIT_AMOUNT / 2, DEPOSIT_AMOUNT)
        .unwrap();

    // Profit arrives after the withdrawal request.
    f.set_custody_amount(DEPOSIT_AMOUNT * 2);

    // Admin pays, but all outputs go to the recorded user.
    f.finish_withdrawal(2, false, true).unwrap();

    assert_eq!(f.token(f.user_usdc).amount, INITIAL_USDC);
    assert_eq!(f.token(f.vault_usdc).amount, DEPOSIT_AMOUNT);
    assert_eq!(f.mint(f.vxau_mint).supply, DEPOSIT_AMOUNT / 2);
    assert_eq!(f.token(f.user_vxau).amount, DEPOSIT_AMOUNT / 2);
    f.assert_queue_closed(2);
}

#[test]
fn queue_minimum_failure_preserves_request_and_shares() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.deposit(DEPOSIT_AMOUNT).unwrap();

    f.queue_withdrawal(3, DEPOSIT_AMOUNT, DEPOSIT_AMOUNT + 1)
        .unwrap();

    let error = f.finish_withdrawal(3, false, true).unwrap_err();
    assert!(error.contains("MinimumOutputNotMet"), "{error}");

    let (request, escrow) = f.queue_addresses(3);
    assert!(f.svm.get_account(&request).unwrap().lamports > 0);
    assert_eq!(f.token(escrow).amount, DEPOSIT_AMOUNT);
    assert_eq!(f.mint(f.vxau_mint).supply, DEPOSIT_AMOUNT);
    assert_eq!(f.token(f.vault_usdc).amount, DEPOSIT_AMOUNT);

    // Owner can cancel an unfillable minimum.
    f.finish_withdrawal(3, true, false).unwrap();
    f.assert_deposit_intact(DEPOSIT_AMOUNT);
    f.assert_queue_closed(3);
}

#[test]
fn queue_cancellation_rejects_non_owner() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.deposit(DEPOSIT_AMOUNT).unwrap();
    f.queue_withdrawal(4, DEPOSIT_AMOUNT, 1).unwrap();

    let error = f.finish_withdrawal(4, true, true).unwrap_err();
    assert!(
        error.contains("UnauthorizedWithdrawalCancellation"),
        "{error}",
    );

    let (_, escrow) = f.queue_addresses(4);
    assert_eq!(f.token(escrow).amount, DEPOSIT_AMOUNT);

    f.finish_withdrawal(4, true, false).unwrap();
    f.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn pause_blocks_deposits_redemptions_and_settlement_but_not_cancel() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.deposit(DEPOSIT_AMOUNT).unwrap();
    f.queue_withdrawal(5, DEPOSIT_AMOUNT / 2, 1).unwrap();

    f.set_pauses(true, true, true).unwrap();

    assert!(f.deposit(1).unwrap_err().contains("DepositsPaused"));
    assert!(f.withdraw(1).unwrap_err().contains("WithdrawalsPaused"));
    assert!(f
        .queue_withdrawal(6, 1, 1)
        .unwrap_err()
        .contains("WithdrawalsPaused"));
    assert!(f
        .finish_withdrawal(5, false, true)
        .unwrap_err()
        .contains("WithdrawalsPaused"));

    f.finish_withdrawal(5, true, false).unwrap();
    f.assert_deposit_intact(DEPOSIT_AMOUNT);

    f.set_pauses(false, false, false).unwrap();
    f.withdraw(DEPOSIT_AMOUNT).unwrap();
}

#[test]
fn configured_strategy_blocks_settlement_but_not_cancellation() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.deposit(DEPOSIT_AMOUNT).unwrap();
    f.queue_withdrawal(7, DEPOSIT_AMOUNT, 1).unwrap();
    f.inject_disabled_strategy();

    let error = f.finish_withdrawal(7, false, true).unwrap_err();
    assert!(error.contains("FullStrategyNavRequired"), "{error}");

    f.finish_withdrawal(7, true, false).unwrap();
    f.assert_deposit_intact(DEPOSIT_AMOUNT);
    f.assert_queue_closed(7);
}

#[test]
fn queue_transfer_failure_rolls_back_settlement_burn() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.deposit(DEPOSIT_AMOUNT).unwrap();
    f.queue_withdrawal(8, DEPOSIT_AMOUNT, 1).unwrap();

    let mut account = f.svm.get_account(&f.user_usdc).unwrap();
    let mut token = SplTokenAccount::unpack(&account.data).unwrap();
    token.state = AccountState::Frozen;
    SplTokenAccount::pack(token, &mut account.data).unwrap();
    f.svm.set_account(f.user_usdc, account).unwrap();

    assert!(f.finish_withdrawal(8, false, true).is_err());

    let (request, escrow) = f.queue_addresses(8);
    assert!(f.svm.get_account(&request).unwrap().lamports > 0);
    assert_eq!(f.token(escrow).amount, DEPOSIT_AMOUNT);
    assert_eq!(f.mint(f.vxau_mint).supply, DEPOSIT_AMOUNT);
    assert_eq!(f.token(f.vault_usdc).amount, DEPOSIT_AMOUNT);

    // Cancel does not transfer USDC, so frozen USDC does not
    // prevent receipt recovery.
    f.finish_withdrawal(8, true, false).unwrap();
    f.assert_queue_closed(8);
}

#[test]
fn recovery_clears_unowned_bootstrap_donation() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.set_custody_amount(1);

    assert!(f
        .deposit(DEPOSIT_AMOUNT)
        .unwrap_err()
        .contains("InvalidShareBootstrap"));

    f.recover_unowned().unwrap();

    let admin_ata = get_associated_token_address(&f.admin.pubkey(), &f.usdc_mint);

    assert_eq!(f.token(admin_ata).amount, 1);
    assert_eq!(f.token(f.vault_usdc).amount, 0);
    assert_eq!(f.state().total_deposited, 0);

    f.deposit(DEPOSIT_AMOUNT).unwrap();
    f.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn recovery_cannot_take_assets_backing_receipts() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();
    f.deposit(DEPOSIT_AMOUNT).unwrap();

    let error = f.recover_unowned().unwrap_err();
    assert!(error.contains("OutstandingShares"), "{error}");
    f.assert_deposit_intact(DEPOSIT_AMOUNT);
}

#[test]
fn vault_admin_transfer_requires_proposal_and_acceptance() {
    let mut f = Fixture::new(6);
    f.initialize_as_admin();

    let accept = || anchor_lang::solana_program::instruction::Instruction {
        program_id: xau_carry_vault::id(),
        accounts: xau_carry_vault::accounts::AcceptVaultAdminV1 {
            new_admin: f.user.pubkey(),
            vault_v1_state: f.vault_state,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::AcceptVaultAdminV1 {}.data(),
    };
    let accept_ix = accept();

    let error = f
        .run_lifecycle_instruction(accept_ix.clone(), false)
        .unwrap_err();
    assert!(error.contains("NotPendingAdmin"), "{error}");

    let proposed = f.user.pubkey();
    let propose_ix = anchor_lang::solana_program::instruction::Instruction {
        program_id: f.program_id,
        accounts: xau_carry_vault::accounts::ManageVaultV1 {
            admin: f.admin.pubkey(),
            vault_v1_state: f.vault_state,
        }
        .to_account_metas(None),
        data: xau_carry_vault::instruction::ProposeVaultAdminV1 {
            new_admin: proposed,
        }
        .data(),
    };

    f.run_lifecycle_instruction(propose_ix, true).unwrap();

    assert_eq!(f.state().admin, f.admin.pubkey());
    assert_eq!(f.state().pending_admin, proposed);

    f.run_lifecycle_instruction(accept_ix, false).unwrap();

    assert_eq!(f.state().admin, proposed);
    assert_eq!(f.state().pending_admin, Pubkey::default());

    // Former admin loses management permission.
    let error = f.set_pauses(true, true, true).unwrap_err();
    assert!(error.contains("UnauthorizedStrategyAdmin"), "{error}");
}

#[path = "common/strategy_handlers.rs"]
mod strategy_handler_tests;
