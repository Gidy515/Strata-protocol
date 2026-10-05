use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::system_program,
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::{
        associated_token::get_associated_token_address,
        token::spl_token::{
            self,
            state::{
                Account as SplTokenAccount,
                AccountState,
                Mint as SplMint,
            },
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
        let program_id = xau_carry_vault::id();
        let admin = Keypair::new();
        let user = Keypair::new();
        let usdc_mint = Keypair::new().pubkey();

        let mut svm = LiteSVM::new();

        let program_path = std::path::Path::new(
            env!("CARGO_MANIFEST_DIR"),
        )
        .join("../../target/deploy/xau_carry_vault.so");

        let program_bytes = std::fs::read(&program_path)
            .expect("Run anchor build before running these tests");

        svm.add_program(program_id, &program_bytes)
            .expect("Failed to load Vault 1");

        svm.airdrop(&admin.pubkey(), 10_000_000_000)
            .unwrap();
        svm.airdrop(&user.pubkey(), 10_000_000_000)
            .unwrap();

        // LiteSVM loads this program under the upgradeable loader.
        // Supply the admin authority required by our initializer.
        let loader_id: Pubkey =
            "BPFLoaderUpgradeab1e11111111111111111111111"
                .parse()
                .unwrap();

        let program_data = Pubkey::find_program_address(
            &[program_id.as_ref()],
            &loader_id,
        )
        .0;

        let mut program_data_account = svm
            .get_account(&program_data)
            .expect("Upgradeable ProgramData account is missing");

        // UpgradeableLoaderState::ProgramData metadata:
        // variant: u32, slot: u64, authority option: u8,
        // authority public key: 32 bytes.
        assert!(program_data_account.data.len() >= 45);
        assert_eq!(
            &program_data_account.data[..4],
            &3_u32.to_le_bytes(),
        );

        program_data_account.data[12] = 1;
        program_data_account.data[13..45]
            .copy_from_slice(admin.pubkey().as_ref());

        svm.set_account(program_data, program_data_account)
            .unwrap();

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
                lamports: svm.minimum_balance_for_rent_exemption(
                    mint_data.len(),
                ),
                data: mint_data,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        let vault_state = Pubkey::find_program_address(
            &[b"vault_v1", usdc_mint.as_ref()],
            &program_id,
        )
        .0;

        let vault_usdc = Pubkey::find_program_address(
            &[b"token_vault", vault_state.as_ref()],
            &program_id,
        )
        .0;

        let vxau_mint = Pubkey::find_program_address(
            &[b"vxau_mint", vault_state.as_ref()],
            &program_id,
        )
        .0;

        let user_usdc = get_associated_token_address(
            &user.pubkey(),
            &usdc_mint,
        );

        let user_vxau = get_associated_token_address(
            &user.pubkey(),
            &vxau_mint,
        );

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
        SplTokenAccount::pack(token_state, &mut token_data)
            .unwrap();

        svm.set_account(
            user_usdc,
            Account {
                lamports: svm.minimum_balance_for_rent_exemption(
                    token_data.len(),
                ),
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

    fn initialize(
        &mut self,
        signer: &Keypair,
    ) -> Result<(), String> {
        let instruction =
            anchor_lang::solana_program::instruction::Instruction {
                program_id: self.program_id,
                accounts:
                    xau_carry_vault::accounts::InitializeVaultV1 {
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
                data:
                    xau_carry_vault::instruction::InitializeVaultV1 {}
                        .data(),
            };

        let message = Message::new_with_blockhash(
            &[instruction],
            Some(&signer.pubkey()),
            &self.svm.latest_blockhash(),
        );

        let transaction = VersionedTransaction::try_new(
            VersionedMessage::Legacy(message),
            &[signer],
        )
        .unwrap();

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
        let admin =
            Keypair::try_from(self.admin.to_bytes().as_slice())
                .unwrap();

        self.initialize(&admin)
            .expect("Vault initialization failed");
    }

    fn deposit(&mut self, amount: u64) -> Result<(), String> {
        let instruction =
            anchor_lang::solana_program::instruction::Instruction {
                program_id: self.program_id,
                accounts:
                    xau_carry_vault::accounts::DepositUsdcV1 {
                        user: self.user.pubkey(),
                        vault_v1_state: self.vault_state,
                        usdc_mint: self.usdc_mint,
                        vault_v1_usdc_account: self.vault_usdc,
                        vxau_mint: self.vxau_mint,
                        user_usdc_ata: self.user_usdc,
                        user_vxau_ata: self.user_vxau,
                        token_program: spl_token::ID,
                        associated_token_program:
                            anchor_spl::associated_token::ID,
                        system_program: system_program::ID,
                    }
                    .to_account_metas(None),
                data:
                    xau_carry_vault::instruction::DepositUsdcV1 {
                        amount,
                    }
                    .data(),
            };

        let message = Message::new_with_blockhash(
            &[instruction],
            Some(&self.user.pubkey()),
            &self.svm.latest_blockhash(),
        );

        let transaction = VersionedTransaction::try_new(
            VersionedMessage::Legacy(message),
            &[&self.user],
        )
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
        let account =
            self.svm.get_account(&self.vault_state).unwrap();

        let mut data: &[u8] = &account.data;

        xau_carry_vault::state::VaultV1State::try_deserialize(
            &mut data,
        )
        .unwrap()
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
        assert_eq!(
            self.token(self.user_usdc).amount,
            INITIAL_USDC,
        );
        assert_eq!(self.token(self.vault_usdc).amount, 0);
        assert_eq!(self.state().total_deposited, 0);

        // A failed deposit must also roll back ATA creation.
        assert!(
            self.svm
                .get_account(&self.user_vxau)
                .map_or(true, |account| account.lamports == 0)
        );
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

    let state_account = fixture
        .svm
        .get_account(&fixture.vault_state)
        .unwrap();

    assert_eq!(state_account.owner, fixture.program_id);

    let custody = fixture.token(fixture.vault_usdc);
    assert_eq!(custody.mint, fixture.usdc_mint);
    assert_eq!(custody.owner, fixture.vault_state);
    assert_eq!(custody.amount, 0);

    let mint = fixture.mint(fixture.vxau_mint);
    assert_eq!(mint.decimals, 6);
    assert_eq!(mint.supply, 0);
    assert_eq!(
        mint.mint_authority,
        COption::Some(fixture.vault_state),
    );
    assert_eq!(mint.freeze_authority, COption::None);

    // SPL Token owns custody accounts and mints;
    // the vault PDA controls their token authority.
    assert_eq!(
        fixture
            .svm
            .get_account(&fixture.vault_usdc)
            .unwrap()
            .owner,
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

    assert!(
        fixture.svm.get_account(&fixture.vault_state).is_none()
    );
}

#[test]
fn initialize_rejects_wrong_usdc_decimals() {
    let mut fixture = Fixture::new(9);

    let admin =
        Keypair::try_from(fixture.admin.to_bytes().as_slice())
            .unwrap();

    let error = fixture.initialize(&admin).unwrap_err();

    assert!(
        error.contains("Custom(6001)"),
        "Expected InvalidDecimals; received: {error}",
    );

    assert!(
        fixture.svm.get_account(&fixture.vault_state).is_none()
    );
}

#[test]
fn initialize_cannot_reset_existing_vault() {
    let mut fixture = Fixture::new(6);
    fixture.initialize_as_admin();
    fixture.deposit(DEPOSIT_AMOUNT).unwrap();

    let admin =
        Keypair::try_from(fixture.admin.to_bytes().as_slice())
            .unwrap();

    assert!(fixture.initialize(&admin).is_err());

    assert_eq!(
        fixture.state().total_deposited,
        DEPOSIT_AMOUNT,
    );
    assert_eq!(
        fixture.token(fixture.vault_usdc).amount,
        DEPOSIT_AMOUNT,
    );
    assert_eq!(
        fixture.mint(fixture.vxau_mint).supply,
        DEPOSIT_AMOUNT,
    );
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
    assert_eq!(
        fixture.token(fixture.vault_usdc).amount,
        DEPOSIT_AMOUNT,
    );
    assert_eq!(
        fixture.token(fixture.user_vxau).amount,
        DEPOSIT_AMOUNT,
    );
    assert_eq!(
        fixture.mint(fixture.vxau_mint).supply,
        DEPOSIT_AMOUNT,
    );
    assert_eq!(
        fixture.state().total_deposited,
        DEPOSIT_AMOUNT,
    );

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

    // Inject an extreme mint supply so mint_to fails.
    // The USDC transfer runs first, so this tests atomic rollback.
    let mut account = fixture
        .svm
        .get_account(&fixture.vxau_mint)
        .unwrap();

    let mut mint = SplMint::unpack(&account.data).unwrap();
    mint.supply = u64::MAX;

    SplMint::pack(mint, &mut account.data).unwrap();

    fixture
        .svm
        .set_account(fixture.vxau_mint, account)
        .unwrap();

    assert!(fixture.deposit(DEPOSIT_AMOUNT).is_err());

    fixture.assert_no_deposit();
    assert_eq!(
        fixture.mint(fixture.vxau_mint).supply,
        u64::MAX,
    );
}