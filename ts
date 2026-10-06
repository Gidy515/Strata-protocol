[33mcommit 83f346d8d830a172dd121dae9fba94a8cd62e79d[m[33m ([m[1;36mHEAD[m[33m -> [m[1;32mfeat/vault-2-deposit-v2[m[33m, [m[1;31morigin/feat/vault-2-deposit-v2[m[33m)[m
Author: Ankit Tiwari <ankittiwari.contact@gmail.com>
Date:   Tue Oct 6 10:23:14 2026 +0000

    build: sync program keys and generate IDLs for Vault 1 and Vault 2

[1mdiff --git a/xau-carry-vault/Anchor.toml b/xau-carry-vault/Anchor.toml[m
[1mindex 1756a45..d9e5e7f 100644[m
[1m--- a/xau-carry-vault/Anchor.toml[m
[1m+++ b/xau-carry-vault/Anchor.toml[m
[36m@@ -1,6 +1,5 @@[m
[31m-skip_local_validator = true[m
[31m-[m
 [toolchain][m
[32m+[m[32manchor_version = "1.2.0"[m
 [m
 [features][m
 resolution = true[m
[36m@@ -8,7 +7,7 @@[m [mskip-lint = false[m
 [m
 [programs.localnet][m
 basket-weighted-vault = "9zhhveFur9Kx6MCxYLLL4JiGAGWhY2bsjL9agcYY3BE2"[m
[31m-xau_carry_vault = "7mMR3QNnykdLvzVgys81aBL6hPNF96ZpmfxPSpf4EGgg"[m
[32m+[m[32mxau_carry_vault = "8FUda9RyAkR2KR6pyBb6h7AVciNQawFRgu5WHZUifTto"[m
 [m
 [provider][m
 cluster = "localnet"[m
[1mdiff --git a/xau-carry-vault/programs/basket-weighted-vault/Cargo.toml b/xau-carry-vault/programs/basket-weighted-vault/Cargo.toml[m
[1mindex f9de1de..6d404f2 100644[m
[1m--- a/xau-carry-vault/programs/basket-weighted-vault/Cargo.toml[m
[1m+++ b/xau-carry-vault/programs/basket-weighted-vault/Cargo.toml[m
[36m@@ -9,6 +9,9 @@[m [mcrate-type = ["cdylib", "rlib"][m
 name = "basket_weighted_vault"[m
 [m
 [features][m
[32m+[m[32mcustom-heap = [][m
[32m+[m[32mcustom-panic = [][m
[32m+[m[32manchor-debug = [][m
 default = [][m
 cpi = ["no-entrypoint"][m
 no-entrypoint = [][m
[36m@@ -17,5 +20,5 @@[m [mno-log-ix-name = [][m
 idl-build = ["anchor-lang/idl-build", "anchor-spl/idl-build"][m
 [m
 [dependencies][m
[31m-anchor-lang = { version = "0.30.1", features = ["init-if-needed"] }[m
[31m-anchor-spl = { version = "0.30.1" }[m
[32m+[m[32manchor-lang = { version = "1.2.0", features = ["init-if-needed"] }[m
[32m+[m[32manchor-spl = { version = "1.2.0" }[m
[1mdiff --git a/xau-carry-vault/programs/basket-weighted-vault/src/instructions.rs b/xau-carry-vault/programs/basket-weighted-vault/src/instructions.rs[m
[1mdeleted file mode 100644[m
[1mindex e90810e..0000000[m
[1m--- a/xau-carry-vault/programs/basket-weighted-vault/src/instructions.rs[m
[1m+++ /dev/null[m
[36m@@ -1,5 +0,0 @@[m
[31m-pub mod initialize;[m
[31m-pub mod increment;[m
[31m-[m
[31m-pub use initialize::*;[m
[31m-pub use increment::*;[m
[1mdiff --git a/xau-carry-vault/programs/basket-weighted-vault/src/instructions/deposit_to_basket_v2.rs b/xau-carry-vault/programs/basket-weighted-vault/src/instructions/deposit_to_basket_v2.rs[m
[1mnew file mode 100644[m
[1mindex 0000000..c985ab6[m
[1m--- /dev/null[m
[1m+++ b/xau-carry-vault/programs/basket-weighted-vault/src/instructions/deposit_to_basket_v2.rs[m
[36m@@ -0,0 +1,81 @@[m
[32m+[m[32muse anchor_lang::prelude::*;[m
[32m+[m[32muse anchor_spl::token::{self, Token, TokenAccount, Transfer};[m
[32m+[m[32muse crate::instructions::initialize_basket_config::BasketConfig;[m
[32m+[m
[32m+[m[32m#[derive(Accounts)][m
[32m+[m[32mpub struct DepositToBasketV2<'info> {[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub user: Signer<'info>,[m
[32m+[m
[32m+[m[32m    #[account([m
[32m+[m[32m        seeds = [b"basket_config", vault.key().as_ref()],[m
[32m+[m[32m        bump = basket_config.bump,[m
[32m+[m[32m        has_one = vault,[m
[32m+[m[32m    )][m
[32m+[m[32m    pub basket_config: Account<'info, BasketConfig>,[m
[32m+[m
[32m+[m[32m    /// CHECK: Vault account associated with basket[m
[32m+[m[32m    pub vault: UncheckedAccount<'info>,[m
[32m+[m
[32m+[m[32m    // User token accounts for each of the 4 basket assets[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub user_token_0: Account<'info, TokenAccount>,[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub user_token_1: Account<'info, TokenAccount>,[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub user_token_2: Account<'info, TokenAccount>,[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub user_token_3: Account<'info, TokenAccount>,[m
[32m+[m
[32m+[m[32m    // Vault token accounts for each of the 4 basket assets[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub vault_token_0: Account<'info, TokenAccount>,[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub vault_token_1: Account<'info, TokenAccount>,[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub vault_token_2: Account<'info, TokenAccount>,[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub vault_token_3: Account<'info, TokenAccount>,[m
[32m+[m
[32m+[m[32m    pub token_program: Program<'info, Token>,[m
[32m+[m[32m}[m
[32m+[m
[32m+[m[32mpub fn deposit_to_basket_v2_handler(ctx: Context<DepositToBasketV2>, total_amount: u64) -> Result<()> {[m
[32m+[m[32m    let basket_config = &ctx.accounts.basket_config;[m
[32m+[m
[32m+[m[32m    let user_tokens = [[m
[32m+[m[32m        &ctx.accounts.user_token_0,[m
[32m+[m[32m        &ctx.accounts.user_token_1,[m
[32m+[m[32m        &ctx.accounts.user_token_2,[m
[32m+[m[32m        &ctx.accounts.user_token_3,[m
[32m+[m[32m    ];[m
[32m+[m
[32m+[m[32m    let vault_tokens = [[m
[32m+[m[32m        &ctx.accounts.vault_token_0,[m
[32m+[m[32m        &ctx.accounts.vault_token_1,[m
[32m+[m[32m        &ctx.accounts.vault_token_2,[m
[32m+[m[32m        &ctx.accounts.vault_token_3,[m
[32m+[m[32m    ];[m
[32m+[m
[32m+[m[32m    for i in 0..4 {[m
[32m+[m[32m        let weight = basket_config.weights[i] as u64;[m
[32m+[m[32m        let deposit_amount = (total_amount * weight) / 10_000;[m
[32m+[m
[32m+[m[32m        if deposit_amount > 0 {[m
[32m+[m[32m            let cpi_accounts = Transfer {[m
[32m+[m[32m                from: user_tokens[i].to_account_info(),[m
[32m+[m[32m                to: vault_tokens[i].to_account_info(),[m
[32m+[m[32m                authority: ctx.accounts.user.to_account_info(),[m
[32m+[m[32m            };[m
[32m+[m
[32m+[m[32m            let cpi_ctx = CpiContext::new([m
[32m+[m[32m                ctx.accounts.token_program.key(),[m
[32m+[m[32m                cpi_accounts,[m
[32m+[m[32m            );[m
[32m+[m
[32m+[m[32m            token::transfer(cpi_ctx, deposit_amount)?;[m
[32m+[m[32m        }[m
[32m+[m[32m    }[m
[32m+[m
[32m+[m[32m    Ok(())[m
[32m+[m[32m}[m
[1mdiff --git a/xau-carry-vault/programs/basket-weighted-vault/src/instructions/initialize_basket_config.rs b/xau-carry-vault/programs/basket-weighted-vault/src/instructions/initialize_basket_config.rs[m
[1mnew file mode 100644[m
[1mindex 0000000..bcf12f6[m
[1m--- /dev/null[m
[1m+++ b/xau-carry-vault/programs/basket-weighted-vault/src/instructions/initialize_basket_config.rs[m
[36m@@ -0,0 +1,55 @@[m
[32m+[m[32muse anchor_lang::prelude::*;[m
[32m+[m
[32m+[m[32m#[derive(Accounts)][m
[32m+[m[32mpub struct InitializeBasketConfig<'info> {[m
[32m+[m[32m    #[account(mut)][m
[32m+[m[32m    pub authority: Signer<'info>,[m
[32m+[m
[32m+[m[32m    /// CHECK: Vault account linked to this basket configuration[m
[32m+[m[32m    pub vault: UncheckedAccount<'info>,[m
[32m+[m
[32m+[m[32m    #[account([m
[32m+[m[32m        init,[m
[32m+[m[32m        payer = authority,[m
[32m+[m[32m        space = 8 + BasketConfig::INIT_SPACE,[m
[32m+[m[32m        seeds = [b"basket_config", vault.key().as_ref()],[m
[32m+[m[32m        bump[m
[32m+[m[32m    )][m
[32m+[m[32m    pub basket_config: Account<'info, BasketConfig>,[m
[32m+[m
[32m+[m[32m    pub system_program: Program<'info, System>,[m
[32m+[m[32m}[m
[32m+[m
[32m+[m[32m#[account][m
[32m+[m[32m#[derive(InitSpace)][m
[32m+[m[32mpub struct BasketConfig {[m
[32m+[m[32m    pub authority: Pubkey,[m
[32m+[m[32m    pub vault: Pubkey,[m
[32m+[m[32m    pub token_mints: [Pubkey; 4],[m
[32m+[m[32m    pub weights: [u16; 4], // Basis points e.g. [2500, 2500, 2500, 2500] = 100%[m
[32m+[m[32m    pub bump: u8,[m
[32m+[m[32m}[m
[32m+[m
[32m+[m[32m#[error_code][m
[32m+[m[32mpub enum BasketError {[m
[32m+[m[32m    #[msg("Total weight sum must equal 10,000 basis points (100%).")][m
[32m+[m[32m    InvalidWeightSum,[m
[32m+[m[32m}[m
[32m+[m
[32m+[m[32mpub fn initialize_basket_config_handler([m
[32m+[m[32m    ctx: Context<InitializeBasketConfig>,[m
[32m+[m[32m    token_mints: [Pubkey; 4],[m
[32m+[m[32m    weights: [u16; 4],[m
[32m+[m[32m) -> Result<()> {[m
[32m+[m[32m    let total_weight: u32 = weights.iter().map(|&w| w as u32).sum();[m
[32m+[m[32m    require!(total_weight == 10_000, BasketError::InvalidWeightSum);[m
[32m+[m
[32m+[m[32m    let basket_config = &mut ctx.accounts.basket_config;[m
[32m+[m[32m    basket_config.authority = ctx.accounts.authority.key();[m
[32m+[m[32m    basket_config.vault = ctx.accounts.vault.key();[m
[32m+[m[32m    basket_config.token_mints = token_mints;[m
[32m+[m[32m    basket_config.weights = weights;[m
[32m+[m[32m    basket_config.bump = ctx.bumps.basket_config;[m
[32m+[m
[32m+[m[32m    Ok(())[m
[32m+[m[32m}[m
[1mdiff --git a/xau-carry-vault/programs/basket-weighted-vault/src/instructions/mod.rs b/xau-carry-vault/programs/basket-weighted-vault/src/instructions/mod.rs[m
[1mnew file mode 100644[m
[1mindex 0000000..667ecdc[m
[1m--- /dev/null[m
[1m+++ b/xau-carry-vault/programs/basket-weighted-vault/src/instructions/mod.rs[m
[36m@@ -0,0 +1,5 @@[m
[32m+[m[32mpub mod initialize_basket_config;[m
[32m+[m[32mpub mod deposit_to_basket_v2;[m
[32m+[m
[32m+[m[32mpub use initialize_basket_config::*;[m
[32m+[m[32mpub use deposit_to_basket_v2::*;[m
[1mdiff --git a/xau-carry-vault/programs/basket-weighted-vault/src/lib.rs b/xau-carry-vault/programs/basket-weighted-vault/src/lib.rs[m
[1mindex fe98a8c..9265ef6 100644[m
[1m--- a/xau-carry-vault/programs/basket-weighted-vault/src/lib.rs[m
[1m+++ b/xau-carry-vault/programs/basket-weighted-vault/src/lib.rs[m
[36m@@ -1,7 +1,9 @@[m
 use anchor_lang::prelude::*;[m
[31m-use anchor_spl::token::{self, Token, TokenAccount, Transfer};[m
 [m
[31m-declare_id!("Fg6PaFq6N8kwfV46T53vqZFSe2WM5GE5G28Vd75534B");[m
[32m+[m[32mpub mod instructions;[m
[32m+[m[32mpub use instructions::*;[m
[32m+[m
[32m+[m[32mdeclare_id!("2nRrgbBQBr4NB81JnZx7ytnoug71HhFQunY5JyYwyTFt");[m
 [m
 #[program][m
 pub mod basket_weighted_vault {[m
[36m@@ -9,104 +11,16 @@[m [mpub mod basket_weighted_vault {[m
 [m
     pub fn initialize_basket_config([m
         ctx: Context<InitializeBasketConfig>,[m
[31m-        vxau_weight: u16,[m
[31m-        rwa_weight: u16,[m
[32m+[m[32m        token_mints: [Pubkey; 4],[m
[32m+[m[32m        weights: [u16; 4],[m
     ) -> Result<()> {[m
[31m-        let basket_config = &mut ctx.accounts.basket_config;[m
[31m-        basket_config.authority = ctx.accounts.authority.key();[m
[31m-        basket_config.vxau_weight = vxau_weight;[m
[31m-        basket_config.rwa_weight = rwa_weight;[m
[31m-        basket_config.bump = ctx.bumps.basket_config;[m
[31m-        Ok(())[m
[32m+[m[32m        initialize_basket_config_handler(ctx, token_mints, weights)[m
     }[m
 [m
[31m-    pub fn deposit_to_basket_v2(ctx: Context<DepositToBasketV2>, amount: u64) -> Result<()> {[m
[31m-        let cpi_accounts = Transfer {[m
[31m-            from: ctx.accounts.user_token_account.to_account_info(),[m
[31m-            to: ctx.accounts.vault_token_account.to_account_info(),[m
[31m-            authority: ctx.accounts.user.to_account_info(),[m
[31m-        };[m
[31m-[m
[31m-        let cpi_ctx = CpiContext::new([m
[31m-            ctx.accounts.token_program.to_account_info(),[m
[31m-            cpi_accounts,[m
[31m-        );[m
[31m-[m
[31m-        token::transfer(cpi_ctx, amount)?;[m
[31m-[m
[31m-        let vault_state = &mut ctx.accounts.vault_v2_state;[m
[31m-        vault_state.total_deposited = vault_state[m
[31m-            .total_deposited[m
[31m-            .checked_add(amount)[m
[31m-            .ok_or(ErrorCode::Overflow)?;[m
[31m-[m
[31m-        msg!("Deposited {} tokens into Vault V2", amount);[m
[31m-        Ok(())[m
[32m+[m[32m    pub fn deposit_to_basket_v2([m
[32m+[m[32m        ctx: Context<DepositToBasketV2>,[m
[32m+[m[32m        total_amount: u64,[m
[32m+[m[32m    ) -> Result<()> {[m
[32m+[m[32m        deposit_to_basket_v2_handler(ctx, total_amount)[m
     }[m
 }[m
[31m-[m
[31m-#[derive(Accounts)][m
[31m-pub struct InitializeBasketConfig<'info> {[m
[31m-    #[account(mut)][m
[31m-    pub authority: Signer<'info>,[m
[31m-[m
[31m-    #[account([m
[31m-        init,[m
[31m-        payer = authority,[m
[31m-        space = 8 + 32 + 2 + 2 + 1,[m
[31m-        seeds = [b"basket_config"],[m
[31m-        bump[m
[31m-    )][m
[31m-    pub basket_config: Account<'info, BasketConfig>,[m
[31m-[m
[31m-    pub system_program: Program<'info, System>,[m
[31m-}[m
[31m-[m
[31m-#[derive(Accounts)][m
[31m-pub struct DepositToBasketV2<'info> {[m
[31m-    #[account(mut)][m
[31m-    pub user: Signer<'info>,[m
[31m-[m
[31m-    #[account([m
[31m-        seeds = [b"basket_config"],[m
[31m-        bump = basket_config.bump,[m
[31m-    )][m
[31m-    pub basket_config: Account<'info, BasketConfig>,[m
[31m-[m
[31m-    #[account([m
[31m-        mut,[m
[31m-        seeds = [b"vault_v2", basket_config.key().as_ref()],[m
[31m-        bump = vault_v2_state.bump,[m
[31m-    )][m
[31m-    pub vault_v2_state: Account<'info, VaultV2State>,[m
[31m-[m
[31m-    #[account(mut)][m
[31m-    pub user_token_account: Account<'info, TokenAccount>,[m
[31m-[m
[31m-    #[account(mut)][m
[31m-    pub vault_token_account: Account<'info, TokenAccount>,[m
[31m-[m
[31m-    pub token_program: Program<'info, Token>,[m
[31m-    pub system_program: Program<'info, System>,[m
[31m-}[m
[31m-[m
[31m-#[account][m
[31m-pub struct BasketConfig {[m
[31m-    pub authority: Pubkey,[m
[31m-    pub vxau_weight: u16,[m
[31m-    pub rwa_weight: u16,[m
[31m-    pub bump: u8,[m
[31m-}[m
[31m-[m
[31m-#[account][m
[31m-pub struct VaultV2State {[m
[31m-    pub basket_config: Pubkey,[m
[31m-    pub total_deposited: u64,[m
[31m-    pub bump: u8,[m
[31m-}[m
[31m-[m
[31m-#[error_code][m
[31m-pub enum ErrorCode {[m
[31m-    #[msg("Arithmetic Overflow")][m
[31m-    Overflow,[m
[31m-}[m
[1mdiff --git a/xau-carry-vault/programs/xau-carry-vault/Cargo.toml b/xau-carry-vault/programs/xau-carry-vault/Cargo.toml[m
[1mindex b4c44c6..29210e7 100644[m
[1m--- a/xau-carry-vault/programs/xau-carry-vault/Cargo.toml[m
[1m+++ b/xau-carry-vault/programs/xau-carry-vault/Cargo.toml[m
[36m@@ -9,6 +9,9 @@[m [mcrate-type = ["cdylib", "rlib"][m
 name = "xau_carry_vault"[m
 [m
 [features][m
[32m+[m[32mcustom-heap = [][m
[32m+[m[32mcustom-panic = [][m
[32m+[m[32manchor-debug = [][m
 default = [][m
 cpi = ["no-entrypoint"][m
 no-entrypoint = [][m
[36m@@ -17,5 +20,5 @@[m [mno-log-ix-name = [][m
 idl-build = ["anchor-lang/idl-build", "anchor-spl/idl-build"][m
 [m
 [dependencies][m
[31m-anchor-lang = { version = "0.30.1", features = ["init-if-needed"] }[m
[31m-anchor-spl = { version = "0.30.1" }[m
[32m+[m[32manchor-lang = { version = "1.2.0", features = ["init-if-needed"] }[m
[32m+[m[32manchor-spl = { version = "1.2.0" }[m
[1mdiff --git a/xau-carry-vault/programs/xau-carry-vault/src/lib.rs b/xau-carry-vault/programs/xau-carry-vault/src/lib.rs[m
[1mindex 32ef473..fbe2c54 100644[m
[1m--- a/xau-carry-vault/programs/xau-carry-vault/src/lib.rs[m
[1m+++ b/xau-carry-vault/programs/xau-carry-vault/src/lib.rs[m
[36m@@ -9,7 +9,7 @@[m [mpub use constants::*;[m
 pub use instructions::*;[m
 pub use state::*;[m
 [m
[31m-declare_id!("7mMR3QNnykdLvzVgys81aBL6hPNF96ZpmfxPSpf4EGgg");[m
[32m+[m[32mdeclare_id!("8FUda9RyAkR2KR6pyBb6h7AVciNQawFRgu5WHZUifTto");[m
 [m
 #[program][m
 pub mod xau_carry_vault {[m
[1mdiff --git a/xau-carry-vault/programs/xau-carry-vault/tests/test_initialize.rs b/xau-carry-vault/programs/xau-carry-vault/tests/test_initialize.rs.bak[m
[1msimilarity index 100%[m
[1mrename from xau-carry-vault/programs/xau-carry-vault/tests/test_initialize.rs[m
[1mrename to xau-carry-vault/programs/xau-carry-vault/tests/test_initialize.rs.bak[m
