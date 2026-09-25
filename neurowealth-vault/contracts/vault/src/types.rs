//! Type definitions for the NeuroWealth Vault contract.
//!
//! This module contains all storage key enums, configuration structs, and state types
//! used throughout the vault contract.

use soroban_sdk::{contracttype, Address, Symbol};

// ============================================================================
// STORAGE KEYS
// ============================================================================

/// Storage keys for vault state.
///
/// This enum defines all keys used for both instance and persistent storage.
/// Instance storage is used for contract-wide configuration, while persistent
/// storage is used for per-user data that requires efficient access.
#[contracttype]
pub enum DataKey {
    /// User's share balance (key: user Address).
    Shares(Address),
    /// Total USDC deposits (principal) in the vault.
    TotalDeposits,
    /// Total vault shares in circulation.
    TotalShares,
    /// Total managed assets for the vault (principal + yield).
    TotalAssets,
    /// Authorized AI agent address
    Agent,
    /// USDC token contract address
    UsdcToken,
    /// Contract pause state
    Paused,
    /// Contract owner address
    Owner,
    /// Pending owner address for two-step ownership transfer
    PendingOwner,
    /// Total Value Locked cap
    TvLCap,
    /// Per-user deposit cap
    UserDepositCap,
    /// Minimum deposit amount
    MinDeposit,
    /// Maximum deposit amount
    MaxDeposit,
    /// Contract version for upgrade tracking
    Version,
    /// Blend pool contract address
    BlendPool,
    /// Current protocol where funds are deployed
    CurrentProtocol,
    /// Multi-protocol allocation mode flag
    MultiProtocolEnabled,
    /// Target allocation to Blend (basis points)
    BlendAllocationBps,
    /// Target allocation to DEX (basis points)
    DexAllocationBps,
    /// USDC principal deployed to Blend
    DeployedToBlend,
    /// USDC principal deployed to DEX
    DeployedToDex,
    /// Last reported APY for Blend (basis points)
    BlendApyBps,
    /// Last reported APY for DEX (basis points)
    DexApyBps,
    /// Legacy Blend approval TTL
    BlendApprovalTtl,
    /// Deployer address
    Deployer,
    /// Minimum ledgers between rebalances
    MinRebalanceInterval,
    /// Last rebalance ledger number
    LastRebalanceLedger,
    /// Protocol approval TTL in ledgers
    ApprovalTtl,
    /// DEX liquidity pool address
    DexPool,
    /// Per-user strategy preference
    UserStrategy(Address),
    /// Pending agent awaiting timelock
    PendingAgent,
    /// Agent timelock expiry ledger
    AgentTimelockExpiry,
    /// Pending upgrade WASM hash
    PendingUpgradeHash,
    /// Upgrade timelock expiry ledger
    UpgradeTimelockExpiry,
    /// Circuit-breaker failure threshold
    MaxConsecutiveFailures,
    /// Running count of consecutive failures
    ConsecutiveFailures,
    /// Index of addresses with shares
    UserSharesIndex,
    /// Migration target vault address
    MigrationTarget,
    /// Migration pause state
    MigrationPaused,
    /// User's locked shares
    LockedShares(Address),
    /// ML-predicted APY for protocol
    ApyPrediction(Symbol),
    /// Cumulative MEV loss
    CumulativeMevLoss,
    /// MEV incident count
    MevIncidentCount,
    /// Max acceptable MEV loss
    MaxAcceptableMevLoss,
    /// User's lock expiry ledger
    LockExpiry(Address),
    /// Minimum holding period in ledgers
    MinHoldingPeriod,
    /// Last deposit ledger for user
    LastDepositLedger(Address),
    /// Rate limit configuration
    RateLimitConfig(Symbol),
    /// Global rate limit state
    RateLimitGlobalState(Symbol),
    /// Per-user rate limit state
    RateLimitUserState(Address, Symbol),
    /// Max batch size
    MaxBatchSize,
    /// First-deposit snapshot for user
    DepositSnapshot(Address),
    /// Protocol adapter contract address
    ProtocolAdapter(Symbol),
    /// Protocol whitelist flag
    ProtocolWhitelist(Symbol),
    /// Protocol whitelist index
    ProtocolWhitelistIndex,
    /// Standby agent key
    StandbyAgent,
    /// Withdrawal queue configuration
    QueueConfig,
    /// Next request ID counter
    NextRequestId,
    /// Withdrawal request
    WithdrawalRequest(u32),
    /// Queue order list
    QueueOrder,
    /// Queue storage version
    WithdrawalQueueStorageVersion,
}

/// Owner-configured allowance for one rate-limit category.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Maximum number of accepted calls during one window.
    pub max_calls: u32,
    /// Length of the window in ledger sequences.
    pub window_ledgers: u32,
}

/// Usage of a rate-limit bucket.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimitState {
    /// Ledger at which the current window began.
    pub window_start: u32,
    /// Number of accepted calls in the current window.
    pub calls: u32,
}

/// Owner-configured withdrawal queue settings.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueConfig {
    /// Maximum number of pending requests the queue can hold (0 = unlimited).
    pub max_size: u32,
    /// Request time-to-live in seconds.
    pub ttl: u64,
}

/// A single withdrawal request in the queue.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalRequest {
    /// Address that submitted the request.
    pub user: Address,
    /// Amount of USDC requested.
    pub amount: i128,
    /// Whether fulfilled by agent.
    pub fulfilled: bool,
    /// Whether user cancelled.
    pub cancelled: bool,
    /// Unix timestamp when created.
    pub created_at: u64,
}

/// First-deposit snapshot for APY calculation.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositSnapshot {
    /// User's share balance after first deposit.
    pub shares: i128,
    /// USDC principal of first deposit.
    pub principal: i128,
    /// Unix timestamp of first deposit.
    pub deposited_at: u64,
}

/// Owner-managed configuration for one supported deposit asset.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetConfig {
    /// Token contract address.
    pub token_address: Address,
    /// Minimum deposit amount.
    pub min_deposit: i128,
    /// Maximum deposit amount.
    pub deposit_limit: i128,
    /// TVL cap for this asset.
    pub tvl_cap: i128,
}

/// Per-asset accounting totals.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetTotals {
    /// Total managed value.
    pub assets: i128,
    /// Total shares in circulation.
    pub shares: i128,
    /// Principal deposited.
    pub deposits: i128,
}

impl Default for AssetTotals {
    fn default() -> Self {
        AssetTotals {
            assets: 0,
            shares: 0,
            deposits: 0,
        }
    }
}

/// Combined per-asset balance result.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetBalance {
    /// User's per-asset shares.
    pub shares: i128,
    /// User's per-asset balance.
    pub assets: i128,
    /// Pool's total assets.
    pub pool_assets: i128,
    /// Pool's total shares.
    pub pool_shares: i128,
}

/// Storage keys for multi-asset state.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MultiAssetKey {
    /// Asset configuration.
    Config(Symbol),
    /// Asset totals.
    Totals(Symbol),
    /// User's per-asset shares.
    Shares(Address, Symbol),
    /// Supported assets index.
    SupportedAssets,
}
