//! Error types for the NeuroWealth Vault contract.

use soroban_sdk::contracterror;

#[contracterror(export = false)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// Vault has already been initialized.
    AlreadyInitialized = 4,
    /// Initializer is not the expected deployer.
    UnauthorizedDeployer = 5,
    /// Minted shares must be positive.
    SharesToMintMustBePositive = 6,
    /// Vault has no liquidity for the requested withdrawal.
    InsufficientLiquidity = 7,
    /// User has insufficient shares.
    InsufficientShares = 8,
    /// Vault has no assets to withdraw.
    NoAssetsToWithdraw = 9,
    /// Burned shares must be positive.
    SharesToBurnMustBePositive = 10,
    /// User has insufficient shares for the requested amount.
    InsufficientSharesForAmount = 11,
    /// User has no shares to withdraw.
    NoSharesToWithdraw = 12,
    /// Vault has no liquidity available.
    NoLiquidityAvailable = 13,
    /// Vault has no assets to return.
    NoAssetsToReturn = 14,
    /// Vault has no shares to burn.
    NoSharesToBurn = 15,
    /// min_out must be non-negative.
    MinOutMustBeNonNegative = 16,
    /// Protocol is not supported.
    UnsupportedProtocol = 17,
    /// Blend pool is not configured.
    BlendPoolNotConfigured = 18,
    /// Caller is not allowed to pause.
    OnlyOwnerCanPause = 19,
    /// Caller is not allowed to unpause.
    OnlyOwnerCanUnpause = 20,
    /// Vault is not paused.
    NotPaused = 21,
    /// Caller is not allowed to emergency pause.
    OnlyOwnerCanEmergencyPause = 22,
    /// TVL cap cannot be negative.
    TvlCapCannotBeNegative = 23,
    /// User deposit cap cannot be negative.
    UserDepositCapCannotBeNegative = 24,
    /// TVL cap must be greater than or equal to user deposit cap.
    TvlCapBelowUserDepositCap = 25,
    /// Caller is not allowed to configure a protocol pool.
    OnlyOwnerCanConfigurePool = 28,
    /// Caller is not the pending owner.
    CallerIsNotPendingOwner = 29,
    /// Caller is not allowed to update total assets.
    OnlyAgentCanUpdateTotalAssets = 30,
    /// Total assets decrease requires explicit allowance.
    TotalAssetsDecreaseNotAllowed = 31,
    /// Total assets decrease exceeds configured maximum bps.
    DecreaseExceedsMaximumAllowedBps = 32,
    /// Vault balance is insufficient for reported assets.
    InsufficientBalanceForAssets = 33,
    /// Caller is not the owner.
    CallerIsNotOwner = 34,
    /// Vault is paused.
    Paused = 35,
    /// Vault is not initialized.
    NotInitialized = 36,
    /// Amount must be positive.
    AmountMustBePositive = 37,
    /// Deposit is below the configured minimum.
    BelowMinimumDeposit = 38,
    /// Deposit exceeds the configured maximum.
    MaximumDepositExceeded = 39,
    /// Deposit exceeds user cap.
    ExceedsUserDepositCap = 40,
    /// Deposit exceeds TVL cap.
    ExceedsTvlCap = 41,
    /// A protocol leg returned less than min_out.
    MinOutNotMet = 42,
    /// Rebalance called before cooldown elapsed.
    RebalanceCooldownActive = 43,
    /// Approval TTL is too low.
    ApprovalTtlTooLow = 44,
    /// Approval TTL is too high.
    ApprovalTtlTooHigh = 45,
    /// DEX pool is not configured.
    DexPoolNotConfigured = 46,
    /// Strategy must be conservative, balanced, or growth.
    InvalidStrategy = 47,
    /// A timelocked proposal is already pending.
    TimelockAlreadyPending = 48,
    /// No timelocked proposal exists.
    NoTimelockPending = 49,
    /// The timelock delay has not elapsed.
    TimelockNotExpired = 50,
    /// Deployer address cannot be zero.
    DeployerCannotBeZeroAddress = 62,
    /// Owner address cannot be zero.
    OwnerCannotBeZeroAddress = 63,
    /// Agent address cannot be zero.
    AgentCannotBeZeroAddress = 64,
    /// USDC token address cannot be zero.
    UsdcTokenCannotBeZeroAddress = 65,
    /// Max deposit exceeds ceiling.
    MaximumDepositExceedsCeiling = 66,
    /// Migration is paused.
    MigrationPaused = 67,
    /// Migration target is invalid.
    InvalidMigrationTarget = 68,
    /// No shares to migrate.
    NoSharesToMigrate = 69,
    /// Shares are already locked.
    SharesAlreadyLocked = 70,
    /// Lock period has not ended.
    LockPeriodNotEnded = 71,
    /// Lock duration is invalid.
    InvalidLockDuration = 72,
    /// Insufficient unlocked shares.
    InsufficientUnlockedShares = 73,
    /// Emergency withdrawal not allowed.
    EmergencyWithdrawalNotAllowed = 74,
    /// Holding period not elapsed.
    HoldingPeriodNotElapsed = 75,
    /// Invalid holding period configuration.
    InvalidHoldingPeriod = 76,
    /// Multi-protocol allocation is invalid.
    InvalidAllocation = 77,
    /// Multi-protocol mode not enabled.
    MultiProtocolNotEnabled = 78,
    /// Multi-protocol mode is enabled.
    MultiProtocolEnabledError = 79,
    /// Rate limit exceeded.
    RateLimitExceeded = 80,
    /// Invalid rate limit category.
    InvalidRateLimitCategory = 81,
    /// Invalid rate limit config.
    InvalidRateLimitConfig = 82,
    /// Batch size exceeded.
    BatchSizeExceeded = 83,
    /// Protocol adapter not configured.
    ProtocolAdapterNotConfigured = 84,
    /// Protocol not whitelisted.
    ProtocolNotWhitelisted = 85,
}
