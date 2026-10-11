use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::{
    extension::{BaseStateWithExtensions, ExtensionType, StateWithExtensions},
    state::Mint,
};

use crate::error::BasketError;

const DENIED_EXTENSIONS: [ExtensionType; 6] = [
    ExtensionType::TransferFeeConfig,
    ExtensionType::TransferHook,
    ExtensionType::NonTransferable,
    ExtensionType::DefaultAccountState,
    ExtensionType::InterestBearingConfig,
    ExtensionType::ConfidentialTransferFeeConfig,
];

pub fn inspect_component_mint(mint_info: &AccountInfo) -> Result<bool> {
    if *mint_info.owner == anchor_spl::token::ID {
        return Ok(false);
    }
    require_keys_eq!(
        *mint_info.owner,
        anchor_spl::token_2022::ID,
        BasketError::UnsupportedMintExtension
    );
    let data = mint_info.try_borrow_data()?;
    let mint = StateWithExtensions::<Mint>::unpack(&data)
        .map_err(|_| error!(BasketError::UnsupportedMintExtension))?;
    let extensions = mint
        .get_extension_types()
        .map_err(|_| error!(BasketError::UnsupportedMintExtension))?;
    require!(
        !extensions.iter().any(|e| DENIED_EXTENSIONS.contains(e)),
        BasketError::UnsupportedMintExtension
    );
    Ok(extensions.contains(&ExtensionType::ScaledUiAmount))
}
