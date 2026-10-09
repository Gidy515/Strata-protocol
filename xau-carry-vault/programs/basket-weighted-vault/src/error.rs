use anchor_lang::prelude::*;

#[error_code]
pub enum BasketError {
    #[msg("Total weight sum must equal 10,000 basis points (100%).")]
    InvalidWeightSum,
    #[msg("Basket is paused")]
    Paused,
    #[msg("An auction is already live for this basket")]
    AuctionActive,
    #[msg("Rebalance cooldown has not elapsed")]
    Cooldown,
    #[msg("Invalid asset index")]
    InvalidAssetIndex,
    #[msg("Invalid rebalance configuration")]
    InvalidConfig,
    #[msg("Remaining accounts do not match the basket configuration")]
    InvalidRemainingAccounts,
    #[msg("Token account does not match the basket configuration")]
    InvalidVaultAccount,
    #[msg("Basket holds no value")]
    EmptyBasket,
    #[msg("Sell component is not above its target plus band")]
    NotOverweight,
    #[msg("Buy component is not below its target")]
    NotUnderweight,
    #[msg("Auction lot is below the minimum")]
    LotTooSmall,
    #[msg("Price account is not a Pyth PriceUpdateV2")]
    InvalidPriceAccount,
    #[msg("Price update is not fully verified")]
    PriceNotVerified,
    #[msg("Price feed id does not match the configured feed")]
    WrongPriceFeed,
    #[msg("Price is stale")]
    StalePrice,
    #[msg("Price confidence interval is too wide")]
    PriceConfidence,
    #[msg("Price must be positive")]
    InvalidPrice,
    #[msg("Mint is not a Token-2022 mint with a Scaled UI Amount extension")]
    InvalidScaledUiMint,
    #[msg("Scaled UI multiplier change is too close")]
    MultiplierChangeWindow,
    #[msg("Auction does not belong to this basket")]
    InvalidAuction,
    #[msg("Auction is not live")]
    AuctionNotLive,
    #[msg("Auction is fully filled")]
    AuctionFilled,
    #[msg("Auction is still live")]
    AuctionStillLive,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Required amount exceeds the caller's limit")]
    SlippageExceeded,
    #[msg("Vault received less than the amount sent")]
    TransferAmountMismatch,
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Each weight must be positive and larger than its band")]
    InvalidTargetWeights,
    #[msg("Price source must be a positive fixed price or a non-zero Pyth feed id")]
    InvalidPriceSource,
    #[msg("Component 0 must be the Vault 1 vXAU mint")]
    InvalidVxauMint,
    #[msg("Mint uses a Token-2022 extension the basket does not support")]
    UnsupportedMintExtension,
    #[msg("Initial units must be positive")]
    InvalidInitialUnits,
    #[msg("Deposit is too small to mint any shares")]
    ZeroShares,
    #[msg("First deposit must mint more than the locked minimum shares")]
    FirstDepositTooSmall,
    #[msg("token_mints does not match the mint accounts passed in")]
    MintMismatch,
    #[msg("Only the program's upgrade authority can do this")]
    Unauthorized,
    #[msg("Mint decimals are out of range")]
    InvalidMintDecimals,
    #[msg("Two accounts that must differ are the same")]
    SameAccount,
    #[msg("Position does not belong to this user and basket")]
    PositionMismatch,
}
