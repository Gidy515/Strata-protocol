use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        system_program,
    },
};
use anchor_spl::{associated_token::get_associated_token_address, token};

use crate::{constants::*, error::VaultError};

const CREATE_ORDER_V2_DISCRIMINATOR: [u8; 8] = [200, 157, 3, 182, 3, 164, 162, 240];

const ORDER_SEED: &[u8] = b"order";
const EVENT_AUTHORITY_SEED: &[u8] = b"__event_authority";

/// Inputs for a market short-increase order.
///
/// `collateral_amount` uses USDC base units.
///
/// `size_delta_value` and `acceptable_price` use GMTrade's
/// native value/price precision. They are not USDC base units.
///
/// The eventual handler must validate pricing, exposure limits,
/// share-accounting readiness, and token-account contents.
pub struct ShortIncreaseParams {
    pub collateral_amount: u64,
    pub size_delta_value: u128,
    pub acceptable_price: u128,
    pub execution_lamports: u64,
}

/// Addresses supplied after the handler validates their account data.
///
/// The adapter checks program, market, store, mint, and derivable
/// user/order/escrow/source addresses. The position account's metadata
/// and its full PDA derivation must be checked by the handler.
pub struct ShortOrderAccounts {
    pub perpetual_program: Pubkey,
    pub strategy_authority: Pubkey,
    pub store: Pubkey,
    pub market: Pubkey,
    pub user: Pubkey,
    pub order: Pubkey,
    pub position: Pubkey,
    pub usdc_mint: Pubkey,
    pub order_usdc_escrow: Pubkey,
    pub strategy_usdc_source: Pubkey,
}

/// Derives the external order address.
pub fn order_address(
    program: &Pubkey,
    store: &Pubkey,
    authority: &Pubkey,
    nonce: &[u8; 32],
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[ORDER_SEED, store.as_ref(), authority.as_ref(), nonce],
        program,
    )
}

fn validate_addresses(accounts: &ShortOrderAccounts, nonce: &[u8; 32]) -> Result<()> {
    require!(
        accounts.perpetual_program.to_string() == GMTRADE_PROGRAM_ADDRESS,
        VaultError::InvalidPerpetualProgram
    );

    require!(
        accounts.store.to_string() == GMTRADE_STORE_ADDRESS,
        VaultError::InvalidPerpetualStore
    );

    require!(
        accounts.market.to_string() == GMTRADE_XAU_MARKET_ADDRESS,
        VaultError::InvalidPerpetualMarket
    );

    require!(
        accounts.usdc_mint.to_string() == MAINNET_USDC_MINT,
        VaultError::InvalidStrategyDepositMint
    );

    let (expected_user, _) = Pubkey::find_program_address(
        &[
            GMTRADE_USER_SEED,
            accounts.store.as_ref(),
            accounts.strategy_authority.as_ref(),
        ],
        &accounts.perpetual_program,
    );

    require_keys_eq!(
        accounts.user,
        expected_user,
        VaultError::InvalidPerpetualUser
    );

    let (expected_order, _) = order_address(
        &accounts.perpetual_program,
        &accounts.store,
        &accounts.strategy_authority,
        nonce,
    );

    require_keys_eq!(
        accounts.order,
        expected_order,
        VaultError::InvalidPerpetualOrderAccounts
    );

    let expected_escrow = get_associated_token_address(&accounts.order, &accounts.usdc_mint);

    require_keys_eq!(
        accounts.order_usdc_escrow,
        expected_escrow,
        VaultError::InvalidPerpetualOrderAccounts
    );

    let expected_source =
        get_associated_token_address(&accounts.strategy_authority, &accounts.usdc_mint);

    require_keys_eq!(
        accounts.strategy_usdc_source,
        expected_source,
        VaultError::InvalidPerpetualOrderAccounts
    );

    Ok(())
}

fn encode_short_increase(nonce: &[u8; 32], params: &ShortIncreaseParams) -> Vec<u8> {
    let mut data = CREATE_ORDER_V2_DISCRIMINATOR.to_vec();

    // create_order_v2 argument 1: nonce.
    data.extend_from_slice(nonce);

    // Argument 2: CreateOrderParams, in published Borsh field order.

    // OrderKind::MarketIncrease.
    data.push(3);

    // decrease_position_swap_type: None.
    data.push(0);

    data.extend_from_slice(&params.execution_lamports.to_le_bytes());

    // swap_path_length: no collateral swap.
    data.push(0);

    data.extend_from_slice(&params.collateral_amount.to_le_bytes());
    data.extend_from_slice(&params.size_delta_value.to_le_bytes());

    // is_long: false -> short.
    data.push(0);

    // is_collateral_long: false -> short-side collateral.
    data.push(0);

    // min_output: None.
    data.push(0);

    // trigger_price: None; this is a market order.
    data.push(0);

    // acceptable_price: Some.
    data.push(1);
    data.extend_from_slice(&params.acceptable_price.to_le_bytes());

    // should_unwrap_native_token: false.
    data.push(0);

    // valid_from_ts: None.
    data.push(0);

    // Argument 3: callback_version: None.
    data.push(0);

    data
}

/// Builds the CPI instruction. This function does not invoke it.
///
/// Receiver is always the strategy authority, keeping outputs under
/// vault control. Callbacks and swap paths are omitted.
pub fn create_short_order_instruction(
    accounts: &ShortOrderAccounts,
    nonce: &[u8; 32],
    params: &ShortIncreaseParams,
) -> Result<Instruction> {
    require!(
        params.collateral_amount > 0 && params.size_delta_value > 0 && params.acceptable_price > 0,
        VaultError::InvalidShortOrderParameters
    );

    validate_addresses(accounts, nonce)?;

    let program = accounts.perpetual_program;
    let authority = accounts.strategy_authority;
    let mint = accounts.usdc_mint;
    let escrow = accounts.order_usdc_escrow;

    let (event_authority, _) = Pubkey::find_program_address(&[EVENT_AUTHORITY_SEED], &program);

    // Anchor encodes an absent optional account using the called
    // program ID as a readonly placeholder.
    let absent = AccountMeta::new_readonly(program, false);

    Ok(Instruction {
        program_id: program,
        accounts: vec![
            // 0: owner.
            AccountMeta::new(authority, true),
            // 1: receiver.
            AccountMeta::new_readonly(authority, false),
            // 2: store.
            AccountMeta::new_readonly(accounts.store, false),
            // 3: market.
            AccountMeta::new(accounts.market, false),
            // 4: user.
            AccountMeta::new(accounts.user, false),
            // 5: order.
            AccountMeta::new(accounts.order, false),
            // 6: position.
            AccountMeta::new(accounts.position, false),
            // 7: initial_collateral_token.
            AccountMeta::new_readonly(mint, false),
            // 8: final_output_token.
            AccountMeta::new_readonly(mint, false),
            // 9: long_token.
            AccountMeta::new_readonly(mint, false),
            // 10: short_token.
            AccountMeta::new_readonly(mint, false),
            // 11: initial_collateral_token_escrow.
            AccountMeta::new(escrow, false),
            // 12: final_output_token_escrow: omitted.
            absent.clone(),
            // 13: long_token_escrow.
            AccountMeta::new(escrow, false),
            // 14: short_token_escrow.
            AccountMeta::new(escrow, false),
            // 15: initial_collateral_token_source.
            AccountMeta::new(accounts.strategy_usdc_source, false),
            // 16: system_program.
            AccountMeta::new_readonly(system_program::ID, false),
            // 17: token_program.
            AccountMeta::new_readonly(token::ID, false),
            // 18: associated_token_program.
            AccountMeta::new_readonly(anchor_spl::associated_token::ID, false),
            // 19: callback_authority: omitted.
            absent.clone(),
            // 20: callback_program: omitted.
            absent.clone(),
            // 21: callback_shared_data_account: omitted.
            absent.clone(),
            // 22: callback_partitioned_data_account: omitted.
            absent,
            // 23: event_authority.
            AccountMeta::new_readonly(event_authority, false),
            // 24: program, required by event CPI.
            AccountMeta::new_readonly(program, false),
        ],
        data: encode_short_increase(nonce, params),
    })
}

const CLOSE_ORDER_V2_DISCRIMINATOR: [u8; 8] = [213, 217, 98, 100, 225, 205, 76, 184];

const STORE_WALLET_SEED: &[u8] = b"store_wallet";

/// Derives GMTrade's system-owned store wallet.
pub fn store_wallet_address(program: &Pubkey, store: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[STORE_WALLET_SEED, store.as_ref()], program)
}

/// Builds close_order_v2 for an order created by our short-increase
/// adapter: USDC collateral, no callbacks, no final-output escrow.
///
/// This only builds an instruction.
///
/// The caller must:
/// - validate the pending record and external order state;
/// - sign as the strategy authority;
/// - verify closure and refund after CPI;
/// - transfer recovered collateral into vault custody;
/// - clear the pending record only after those checks succeed.
///
/// It is not a general reconciliation adapter for executed orders.
pub fn close_short_order_instruction(
    accounts: &ShortOrderAccounts,
    nonce: &[u8; 32],
) -> Result<Instruction> {
    validate_addresses(accounts, nonce)?;

    let program = accounts.perpetual_program;
    let authority = accounts.strategy_authority;
    let mint = accounts.usdc_mint;
    let escrow = accounts.order_usdc_escrow;
    let refund_ata = accounts.strategy_usdc_source;

    let (store_wallet, _) = store_wallet_address(&program, &accounts.store);

    let (event_authority, _) = Pubkey::find_program_address(&[EVENT_AUTHORITY_SEED], &program);

    let absent = AccountMeta::new_readonly(program, false);

    // Borsh string: u32 byte length followed by UTF-8 bytes.
    let reason = b"Strata: cancel pending short order";

    let mut data = CLOSE_ORDER_V2_DISCRIMINATOR.to_vec();
    data.extend_from_slice(&(reason.len() as u32).to_le_bytes());
    data.extend_from_slice(reason);

    Ok(Instruction {
        program_id: program,
        accounts: vec![
            // 0: executor. The strategy authority signs through CPI.
            AccountMeta::new_readonly(authority, true),
            // 1: store.
            AccountMeta::new(accounts.store, false),
            // 2: store_wallet.
            AccountMeta::new(store_wallet, false),
            // 3: owner.
            AccountMeta::new(authority, false),
            // 4: receiver.
            AccountMeta::new(authority, false),
            // 5: rent_receiver.
            AccountMeta::new(authority, false),
            // 6: user.
            AccountMeta::new(accounts.user, false),
            // 7: referrer_user: omitted.
            absent.clone(),
            // 8: order.
            AccountMeta::new(accounts.order, false),
            // 9: initial_collateral_token.
            AccountMeta::new_readonly(mint, false),
            // 10: final_output_token: omitted.
            absent.clone(),
            // 11: long_token.
            AccountMeta::new_readonly(mint, false),
            // 12: short_token.
            AccountMeta::new_readonly(mint, false),
            // 13: initial_collateral_token_escrow.
            AccountMeta::new(escrow, false),
            // 14: final_output_token_escrow: omitted.
            absent.clone(),
            // 15: long_token_escrow.
            AccountMeta::new(escrow, false),
            // 16: short_token_escrow.
            AccountMeta::new(escrow, false),
            // 17: initial_collateral_token_ata.
            AccountMeta::new(refund_ata, false),
            // 18: final_output_token_ata: omitted.
            absent.clone(),
            // 19: long_token_ata.
            AccountMeta::new(refund_ata, false),
            // 20: short_token_ata.
            AccountMeta::new(refund_ata, false),
            // 21: system_program.
            AccountMeta::new_readonly(system_program::ID, false),
            // 22: token_program.
            AccountMeta::new_readonly(token::ID, false),
            // 23: associated_token_program.
            AccountMeta::new_readonly(anchor_spl::associated_token::ID, false),
            // 24: callback_authority: omitted.
            absent.clone(),
            // 25: callback_program: omitted.
            absent.clone(),
            // 26: callback_shared_data_account: omitted.
            absent.clone(),
            // 27: callback_partitioned_data_account: omitted.
            absent,
            // 28: event_authority.
            AccountMeta::new_readonly(event_authority, false),
            // 29: program.
            AccountMeta::new_readonly(program, false),
        ],
        data,
    })
}

/// Native USD minimum is a value (20 decimals), not a token quantity.
pub struct ShortDecreaseParams {
    pub collateral_withdrawal: u64,
    pub size_delta_value: u128,
    pub acceptable_price: u128,
    pub min_output_value: u128,
    pub execution_lamports: u64,
}
pub fn create_short_decrease_instruction(
    a: &ShortOrderAccounts,
    nonce: &[u8; 32],
    p: &ShortDecreaseParams,
) -> Result<Instruction> {
    validate_addresses(a, nonce)?;
    require!(
        p.size_delta_value > 0 && p.acceptable_price > 0,
        VaultError::InvalidShortOrderParameters
    );
    let mut ix = create_short_order_instruction(
        a,
        nonce,
        &ShortIncreaseParams {
            collateral_amount: 1,
            size_delta_value: p.size_delta_value,
            acceptable_price: p.acceptable_price,
            execution_lamports: p.execution_lamports,
        },
    )?;
    let absent = AccountMeta::new_readonly(a.perpetual_program, false);
    ix.accounts[7] = absent.clone();
    ix.accounts[11] = absent.clone();
    ix.accounts[12] = AccountMeta::new(a.order_usdc_escrow, false);
    ix.accounts[15] = absent;
    let mut data = CREATE_ORDER_V2_DISCRIMINATOR.to_vec();
    data.extend_from_slice(nonce);
    data.push(4); // MarketDecrease
    data.push(0); // no decrease swap: both collateral and payout are USDC
    data.extend_from_slice(&p.execution_lamports.to_le_bytes());
    data.push(0); // no swap path
    data.extend_from_slice(&p.collateral_withdrawal.to_le_bytes());
    data.extend_from_slice(&p.size_delta_value.to_le_bytes());
    data.extend_from_slice(&[0, 0, 1]); // short, short collateral, Some min-output value
    data.extend_from_slice(&p.min_output_value.to_le_bytes());
    data.push(0); // no trigger
    data.push(1);
    data.extend_from_slice(&p.acceptable_price.to_le_bytes());
    data.extend_from_slice(&[0, 0, 0]); // no unwrap, valid-from or callback
    ix.data = data;
    Ok(ix)
}
pub fn close_short_decrease_instruction(
    a: &ShortOrderAccounts,
    nonce: &[u8; 32],
) -> Result<Instruction> {
    let mut ix = close_short_order_instruction(a, nonce)?;
    let absent = AccountMeta::new_readonly(a.perpetual_program, false);
    ix.accounts[9] = absent.clone();
    ix.accounts[10] = AccountMeta::new_readonly(a.usdc_mint, false);
    ix.accounts[13] = absent.clone();
    ix.accounts[14] = AccountMeta::new(a.order_usdc_escrow, false);
    ix.accounts[17] = absent;
    ix.accounts[18] = AccountMeta::new(a.strategy_usdc_source, false);
    Ok(ix)
}
