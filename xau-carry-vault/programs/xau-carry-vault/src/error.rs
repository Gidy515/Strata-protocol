use anchor_lang::prelude::*;

#[error_code]
pub enum VaultError {
    #[msg("Only the program upgrade authority may initialize a vault")]
    Unauthorized,

    #[msg("The deposit mint must have six decimals")]
    InvalidDecimals,

    #[msg("Deposit amount must be greater than zero")]
    ZeroAmount,

    #[msg("Cumulative deposits overflowed")]
    Overflow,

    #[msg("Insufficient vXAU receipts to redeem")]
    InsufficientReceipts,

    #[msg("Insufficient USDC available in vault custody")]
    InsufficientLiquidity,

    #[msg("Only the recorded vault administrator may configure the strategy")]
    UnauthorizedStrategyAdmin,

    #[msg("The configured strategy requires mainnet USDC")]
    InvalidStrategyDepositMint,

    #[msg("Unexpected gold mint")]
    InvalidGoldMint,

    #[msg("The gold mint must have six decimals")]
    InvalidGoldDecimals,

    #[msg("Unexpected perpetual program")]
    InvalidPerpetualProgram,

    #[msg("Unexpected perpetual store")]
    InvalidPerpetualStore,

    #[msg("Unexpected perpetual market")]
    InvalidPerpetualMarket,

    #[msg("Unsupported external account layout or version")]
    InvalidExternalAccountData,

    #[msg("The perpetual market is disabled or closed")]
    PerpetualMarketUnavailable,

    #[msg("The perpetual market collateral does not match vault USDC")]
    InvalidPerpetualCollateral,

    #[msg("Unexpected perpetual index identifier")]
    InvalidPerpetualIndex,

    #[msg("Strategy configuration does not belong to this vault")]
    InvalidStrategyVault,

    #[msg("The strategy authority must be a system-owned account without data")]
    InvalidStrategyAuthority,

    #[msg("Unexpected perpetual user account")]
    InvalidPerpetualUser,

    #[msg("Unexpected perpetual short-position account")]
    InvalidPerpetualPosition,

    #[msg("Short-order collateral, size, and acceptable price must be positive")]
    InvalidShortOrderParameters,

    #[msg("Unexpected perpetual order or escrow address")]
    InvalidPerpetualOrderAccounts,

    #[msg("Strategy execution is disabled")]
    StrategyExecutionDisabled,

    #[msg("The vault has insufficient idle USDC for this order")]
    InsufficientStrategyCollateral,

    #[msg("The order escrow must be empty before submission")]
    OrderEscrowNotEmpty,

    #[msg("Unexpected token balances after order creation")]
    InvalidOrderCollateralAccounting,

    #[msg("Pending order does not match this vault")]
    InvalidPendingOrder,

    #[msg("Only an unexecuted pending order can use this cancellation path")]
    OrderNotPending,

    #[msg("External order closure did not complete")]
    OrderClosureIncomplete,

    #[msg("Cancellation did not return the recorded collateral")]
    InvalidCancellationRefund,

    #[msg("A new vault cannot bootstrap shares against existing unowned assets")]
    InvalidShareBootstrap,

    #[msg("Outstanding shares exist but the vault has zero net assets")]
    VaultInsolvent,

    #[msg("The deposit is too small to mint one share base unit")]
    DepositTooSmall,

    #[msg("Invalid gold price or oracle validation limits")]
    InvalidGoldPrice,

    #[msg("Gold price is stale or has a future publication timestamp")]
    InvalidGoldPriceTimestamp,

    #[msg("Gold price confidence interval exceeds the accepted limit")]
    GoldPriceTooUncertain,

    #[msg("Unexpected oracle account owner")]
    InvalidOracleOwner,

    #[msg("Unsupported oracle account format")]
    InvalidOracleAccount,

    #[msg("Oracle feed does not match the configured asset")]
    InvalidOracleFeed,

    #[msg("Oracle update is not fully verified")]
    OracleNotFullyVerified,

    #[msg("Oracle update was posted in a future slot")]
    InvalidOracleSlot,

    #[msg("A configured strategy requires full strategy NAV accounting")]
    FullStrategyNavRequired,

    #[msg("Minimum output must be greater than zero")]
    InvalidMinimumOutput,

    #[msg("Calculated output is below the user's minimum")]
    MinimumOutputNotMet,

    #[msg("The redemption is too small to return one USDC base unit")]
    RedemptionTooSmall,

    #[msg("Vault deposits are paused")]
    DepositsPaused,

    #[msg("Vault withdrawals are paused")]
    WithdrawalsPaused,

    #[msg("Strategy submissions are paused")]
    StrategyPaused,

    #[msg("The order exceeds configured size or collateral limits")]
    OrderLimitExceeded,

    #[msg("Order limits must be positive")]
    InvalidOrderLimits,

    #[msg("Invalid proposed administrator")]
    InvalidProposedAdmin,

    #[msg("The signer is not the proposed administrator")]
    NotPendingAdmin,

    #[msg("Withdrawal request does not belong to this vault or owner")]
    InvalidWithdrawalRequest,

    #[msg("Only the request owner may cancel")]
    UnauthorizedWithdrawalCancellation,

    #[msg("Withdrawal escrow contains fewer shares than recorded")]
    InvalidWithdrawalEscrow,

    #[msg("Unowned custody recovery requires zero outstanding shares")]
    OutstandingShares,
    #[msg("Authenticated strategy accounting failed or unsupported GMTrade state")]
    StrategyAccountingFailed,
    #[msg("GMTrade order execution has not been proven")]
    UnprovenOrderExecution,
    #[msg("Order baseline does not match the current pending order")]
    InvalidOrderBaseline,

    #[msg("Final redemption requires the strategy to be fully unwound")]
    StrategyUnwindRequired,

    #[msg("Invalid or unsupported gold swap route")]
    InvalidGoldRoute,
    #[msg("Swap or hedge execution exceeds the trusted oracle bound")]
    StrategyPriceBound,
    #[msg("Strategy token authority, delegate or mint changed during CPI")]
    StrategyAccountChanged,
}
