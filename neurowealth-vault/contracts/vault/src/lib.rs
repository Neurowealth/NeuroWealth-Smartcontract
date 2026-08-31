//! # NeuroWealth Vault Contract
//!
//! An ERC-4626 inspired vault contract for the NeuroWealth AI-powered DeFi yield platform on Stellar.
//!
//! ## Architecture Overview
//!
//! This contract implements a non-custodial vault where users deposit USDC and an AI agent
//! automatically deploys those funds across various yield-generating protocols on the Stellar
//! blockchain.
//!
//! ## Share Accounting Model
//!
//! This implementation follows an ERC-4626-inspired share-based model where:
//! - Users deposit USDC and receive vault shares representing proportional ownership
//! - Total shares remain constant while yield is accrued
//! - The value of each share increases as `total_assets` grows
//! - Withdrawals burn shares and return the user's proportional share of total assets
//!
//! Core math:
//! - `shares_to_mint = (assets * total_shares) / total_assets`
//!   - Bootstrap case: when `total_shares == 0 || total_assets == 0`, `shares_to_mint = assets`
//! - `assets_to_return = (shares * total_assets) / total_shares`
//!
//! ## Rounding Policy (ERC-4626 Best Practice)
//!
//! This contract follows the ERC-4626 rounding convention:
//! - **Floor mint**: When depositing, shares minted are rounded DOWN to protect the vault.
//!   - `shares_to_mint = floor(assets * total_shares / total_assets)`
//! - **Ceil burn**: When withdrawing, shares burned are rounded UP to protect the vault.
//!   - `shares_to_burn = ceil(assets * total_shares / total_assets)`
//! - **Floor return**: When converting burned shares to returned assets, assets are rounded DOWN.
//!   - `assets_to_return = floor(shares * total_assets / total_shares)`
//!
//! This ensures:
//! - The vault never loses value due to rounding
//! - Dust attacks are prevented (at least 1 share burned when assets > 0)
//! - Users cannot gain from rounding
//! - Automatic yield growth tracking
//! - Fair distribution of earnings
//! - Mathematically consistent deposits and withdrawals
//!
//! ## Asset Flow
//!
//! ```text
//! Deposit Flow:
//! User → [USDC Token] → [Vault Contract] → [AI Agent monitors]
//!                      ↓
//!              Shares recorded per user
//!              DepositEvent emitted
//!
//! Rebalance Flow (AI Agent):
//! AI Agent → [Vault.rebalance()] → [External Protocols (Blend, DEX)]
//!                              ↓
//!                      RebalanceEvent emitted
//!
//! Withdraw Flow:
//! User → [Vault.withdraw()] → [Vault Contract] → [USDC Token] → User
//!         ↓
//! Shares burned
//! WithdrawEvent emitted
//! ```
//!
//! ## Storage Layout
//!
//! ### Instance Storage (Contract-Wide, Expensive to Read/Write)
//! - `Agent`: The authorized AI agent address that can call rebalance()
//! - `UsdcToken`: The USDC token contract address
//! - `TotalDeposits`: Total USDC principal deposited by users; never includes yield.
//!   Use `TotalAssets` for share pricing and cap guards (see issue #299 / ARCHITECTURE.md).
//! - `Paused`: Boolean flag for emergency pause state
//! - `Owner`: Contract owner address for administrative functions
//! - `TvlCap`: Maximum total value locked in the vault
//! - `UserDepositCap`: Maximum deposit per user
//! - `ApprovalTtl`: Shared approval lifetime in ledgers for Blend and DEX approvals
//! - `Version`: Contract version for upgrade tracking
//! - `MinRebalanceInterval`: Minimum ledgers between rebalances (owner-configurable, Issue #59)
//! - `LastRebalanceLedger`: Ledger number of the most recent successful rebalance call (Issue #59)
//! - `RateLimitConfig(Symbol)`: Owner-configured call allowance and ledger window
//! - `RateLimitGlobalState(Symbol)`: Global rate-limit usage buckets
//! - `RateLimitUserState(Address, Symbol)`: Per-user rate-limit usage buckets
//! - `MaxBatchSize`: Maximum entries accepted by `batch_deposit`
//!
//! ### Persistent Storage (Per-User, Cheaper)
//! - `Shares(user)`: vault shares owned by each user address
//!
//! ## Event Design Philosophy
//!
//! Events are emitted for all state-changing operations to enable:
//! - AI agent to detect deposits/withdrawals and react accordingly
//! - Frontend applications to track user balances in real-time
//! - External indexers to build transaction histories
//! - Security auditors to verify contract behavior
//!
//! ## Upgrade Model
//!
//! This contract supports upgradeability through Soroban's built-in contract upgrade
//! mechanism. The owner can upgrade the contract code while preserving storage state.
//! Upgrades must be performed carefully to maintain:
//! - User balances
//! - Total deposits
//! - Agent and owner addresses
//! - Configuration parameters
//!
//! # Examples
//!
//! ## Deposit USDC
//! ```ignore
//! let token_client = token::Client::new(&env, &usdc_token);
//! token_client.transfer(&user, &vault_address, &amount);
//! vault_client.deposit(&user, &amount);
//! ```
//!
//! Note: This example is marked `ignore` because doctests cannot construct a live
//! Soroban Env with the required token and contract setup. For comprehensive
//! deposit/withdrawal testing, see `tests/test_deposit.rs` and
//! `tests/test_rebalance_integration.rs` which cover the full lifecycle with
//! mocked environments.
//!
//! ## Withdraw USDC
//! ```ignore
//! vault_client.withdraw(&user, &amount);
//! ```

// `missing_docs` cannot be denied crate-wide: `#[contract]`, `#[contracttype]`,
// and `#[contracterror]` each expand to an undocumented static and associated
// function whose spans point at the attribute itself, so no source-level
// `#[allow]` can annotate them. The crate-level allow below exists solely to
// suppress that macro-generated noise — it is *not* a licence to ship
// undocumented API:
//
// - `#[warn(missing_docs)]` on `impl NeuroWealthVault` (see below) re-enables
//   the lint for every public contract entrypoint. CI runs clippy with
//   `-D warnings`, so an undocumented `pub fn` fails the build.
// - Every hand-written public item — event structs and their fields,
//   `DataKey` variants, `VaultError` variants — is documented explicitly.
//
// Per-item `#[allow(missing_docs)]` attributes were removed in favour of this
// single, explained crate-level allow (issues #422 / #423).
#![allow(missing_docs)]
#![no_std]
#![allow(deprecated)]
// These mixed-case aliases are part of the legacy `VaultError` API and are
// retained for source compatibility with existing clients and tests.
#![allow(non_upper_case_globals)]

pub mod topics;

use core::cmp::min;
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    vec, Address, BytesN, Env, IntoVal, String, Symbol, Val, Vec,
};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Default TVL cap applied at initialization: 100,000,000,000 stroops = 100M USDC.
///
/// Expressed in USDC's 7-decimal representation (1 USDC = 10_000_000 stroops).
/// The owner can raise or lower this after deployment via `set_tvl_cap`.
const DEFAULT_TVL_CAP: i128 = 100_000_000_000;

// ============================================================================
// ERROR TYPES
// ============================================================================

// Soroban's embedded contract-spec error union is limited to 50 cases. The
// vault keeps a larger, numerically stable error surface for compatibility, so
// the repository's generated contract spec is the source of truth for this
// type instead of embedding an oversized union in the WASM.
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
    /// Caller is not the pending owner (or no pending ownership transfer exists).
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
    /// Rebalance called before the configured cooldown has elapsed.
    RebalanceCooldownActive = 43,
    /// Approval TTL is below the allowed floor.
    ApprovalTtlTooLow = 44,
    /// Approval TTL is above the allowed ceiling.
    ApprovalTtlTooHigh = 45,
    /// DEX liquidity pool is not configured.
    DexPoolNotConfigured = 46,
    /// Strategy must be one of "conservative", "balanced", or "growth".
    InvalidStrategy = 47,
    /// A timelocked proposal (agent update or upgrade) is already pending.
    ///
    /// Shared by the agent timelock (#317) and the upgrade timelock (#316).
    /// The SDK caps `#[contracterror]` enums at 50 cases, so both two-step flows
    /// reuse one set of generic timelock error codes rather than each defining
    /// their own.
    TimelockAlreadyPending = 48,
    /// No timelocked proposal exists to confirm/execute or cancel.
    NoTimelockPending = 49,
    /// The timelock delay has not yet elapsed.
    TimelockNotExpired = 50,
    /// Deployer address supplied to `initialize` is the zero address.
    DeployerCannotBeZeroAddress = 62,
    /// Owner address supplied to `initialize` is the zero address.
    OwnerCannotBeZeroAddress = 63,
    /// Agent address supplied to `initialize` is the zero address.
    AgentCannotBeZeroAddress = 64,
    /// USDC token address supplied to `initialize` is the zero address.
    UsdcTokenCannotBeZeroAddress = 65,
    /// Maximum per-transaction deposit exceeds the absolute configured ceiling.
    MaximumDepositExceedsCeiling = 66,
    /// Migration is paused by the owner.
    MigrationPaused = 67,
    /// Migration target vault address is not set by owner.
    InvalidMigrationTarget = 68,
    /// User has no shares to migrate.
    NoSharesToMigrate = 69,
    /// Shares are already locked.
    SharesAlreadyLocked = 70,
    /// Lock period has not ended.
    LockPeriodNotEnded = 71,
    /// Lock duration is not supported.
    InvalidLockDuration = 72,
    /// Insufficient unlocked shares to lock.
    InsufficientUnlockedShares = 73,
    /// Emergency withdrawal not allowed (vault not paused).
    EmergencyWithdrawalNotAllowed = 74,
    /// Withdrawal rejected: minimum holding period since last deposit has not elapsed (#659).
    HoldingPeriodNotElapsed = 75,
    /// Holding period configuration is invalid (must be non-negative) (#659).
    InvalidHoldingPeriod = 76,
    /// The configured call rate for an operation has been exhausted.
    RateLimitExceeded = 77,
    /// The owner supplied an unsupported rate-limit category.
    InvalidRateLimitCategory = 78,
    /// A rate-limit window must be non-zero when a limit is enabled.
    InvalidRateLimitConfig = 79,
    /// A batch contains more entries than the configured maximum.
    BatchSizeExceeded = 80,
}

impl VaultError {
    pub const NegativeMin: Self = Self::InvalidStrategy;
    pub const NegativeMax: Self = Self::InvalidStrategy;
    pub const MaxLessThanMin: Self = Self::InvalidStrategy;
    pub const MinimumDepositTooLow: Self = Self::InvalidStrategy;
    pub const MaximumDepositBelowMinimum: Self = Self::InvalidStrategy;
    pub const ShareConversionOverflow: Self = Self::InvalidStrategy;
    pub const TotalDepositsOverflow: Self = Self::InvalidStrategy;
    pub const SharesOverflow: Self = Self::InvalidStrategy;
    pub const TotalSharesOverflow: Self = Self::InvalidStrategy;
    pub const TotalAssetsOverflow: Self = Self::InvalidStrategy;
    pub const ShareToAssetConversionOverflow: Self = Self::InvalidStrategy;
    pub const ExchangeRateOverflow: Self = Self::InvalidStrategy;
    pub const WithdrawalUnderflow: Self = Self::InvalidStrategy;
    pub const MaxDecreaseOverflow: Self = Self::InvalidStrategy;
    pub const TotalAvailableOverflow: Self = Self::InvalidStrategy;
    pub const VersionOverflow: Self = Self::InvalidStrategy;
    pub const InvalidWasmHash: Self = Self::InvalidStrategy;
    // The SDK caps `#[contracterror]` at 50 cases. Extra names stay as
    // associated constants (same pattern as `InvalidWasmHash`) so call sites
    // compile without growing the on-chain error enum past the spec limit.
    pub const DeployerCannotBeZeroAddress: Self = Self::UnauthorizedDeployer;
    pub const OwnerCannotBeZeroAddress: Self = Self::CallerIsNotOwner;
    pub const AgentCannotBeZeroAddress: Self = Self::UnauthorizedDeployer;
    pub const UsdcTokenCannotBeZeroAddress: Self = Self::UnauthorizedDeployer;
    pub const MaximumDepositExceedsCeiling: Self = Self::MaximumDepositExceeded;
    pub const MigrationPaused: Self = Self::Paused;
    pub const InvalidMigrationTarget: Self = Self::InvalidStrategy;
    pub const NoSharesToMigrate: Self = Self::NoSharesToWithdraw;
    pub const SharesAlreadyLocked: Self = Self::InvalidStrategy;
    pub const LockPeriodNotEnded: Self = Self::InvalidStrategy;
    pub const InvalidLockDuration: Self = Self::InvalidStrategy;
    pub const InsufficientUnlockedShares: Self = Self::InsufficientShares;
    pub const EmergencyWithdrawalNotAllowed: Self = Self::NotPaused;
    pub const HoldingPeriodNotElapsed: Self = Self::InvalidStrategy;
    pub const InvalidHoldingPeriod: Self = Self::InvalidStrategy;
}

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
    /// Legacy user's principal USDC balance (key: user Address).
    ///
    /// Deprecated: retained only to preserve the serialized `DataKey` layout
    /// across upgrades. New accounting must not read or write this key; user
    /// balances are derived from `Shares(user)` and the current exchange rate.
    Balance(Address),
    /// User's share balance (key: user Address).
    /// Represents proportional ownership of the vault's total assets.
    Shares(Address),
    /// Total USDC deposits (principal) in the vault.
    /// Stored in instance storage (single value, frequently read).
    /// This tracks deposited principal only and does NOT include yield.
    TotalDeposits,
    /// Total vault shares in circulation.
    /// Used for share-based accounting and conversions.
    TotalShares,
    /// Total managed assets for the vault (principal + yield).
    /// This is the authoritative value used for share pricing.
    TotalAssets,
    /// Authorized AI agent address
    /// Can only call rebalance() to move funds between yield strategies
    Agent,
    /// USDC token contract address
    /// The vault accepts only this token for deposits
    UsdcToken,
    /// Contract pause state
    /// When true, deposits and withdrawals are disabled
    Paused,
    /// Contract owner address
    /// Can perform administrative functions (pause, upgrade, set limits)
    Owner,
    /// Pending owner address for two-step ownership transfer
    PendingOwner,
    /// Total Value Locked cap
    /// Maximum total USDC that can be deposited in the vault
    TvLCap,
    /// Per-user deposit cap
    /// Maximum amount a single user can deposit
    UserDepositCap,
    /// Minimum deposit amount
    /// Minimum amount required for a single deposit
    MinDeposit,
    /// Maximum deposit amount
    /// Maximum amount allowed for a single deposit
    MaxDeposit,
    /// Contract version for upgrade tracking
    Version,
    /// Blend pool contract address
    /// The address of the Blend lending pool contract for on-chain integration
    BlendPool,
    /// Current protocol where funds are deployed
    /// Symbol indicating the active protocol (e.g., "blend", "none")
    CurrentProtocol,
    /// Legacy Blend-specific approval TTL key.
    ///
    /// Retained for backward compatibility with already-initialized instances.
    /// The live approval path now reads `ApprovalTtl`, falling back to this key
    /// when no shared TTL has been written yet.
    BlendApprovalTtl,
    /// Deployer address - the address that deployed the contract
    /// Used for signature verification during initialization to prevent front-running
    Deployer,
    /// Minimum number of ledgers that must elapse between rebalance() calls.
    /// Configurable by the owner. When absent, no cooldown is enforced.
    /// (Issue #59)
    MinRebalanceInterval,
    /// Ledger sequence number of the most recent successful rebalance() call.
    /// Written at the end of every successful rebalance.
    /// (Issue #59)
    LastRebalanceLedger,
    /// Number of ledgers added to the current ledger for protocol approvals.
    ApprovalTtl,
    /// DEX liquidity pool contract address
    /// The address of the Stellar DEX liquidity pool contract used by the
    /// Balanced/Growth strategies for on-chain liquidity provision.
    DexPool,
    /// Per-user investment strategy preference.
    /// Set by the user, read by the AI agent to determine yield deployment.
    UserStrategy(Address),
    /// Pending agent address awaiting timelock confirmation (#317).
    PendingAgent,
    /// Ledger sequence at which the pending agent update becomes effective (#317).
    AgentTimelockExpiry,
    /// Pending contract WASM hash awaiting timelock execution (#316).
    PendingUpgradeHash,
    /// Ledger sequence at which the pending upgrade becomes executable (#316).
    UpgradeTimelockExpiry,
    /// Owner-configurable circuit-breaker threshold (#439).
    ///
    /// The number of consecutive failed rebalances that trips an automatic
    /// emergency pause. Falls back to a default value when unset (e.g. instances
    /// initialized before the circuit breaker existed).
    MaxConsecutiveFailures,
    /// Running count of consecutive failed rebalances (#439).
    ///
    /// Incremented on every `"failed"` rebalance and reset to `0` on any
    /// `"success"`. When it reaches [`DataKey::MaxConsecutiveFailures`] the vault
    /// auto-pauses.
    ConsecutiveFailures,
    /// Append-only index of addresses that have ever held non-zero shares (#440).
    ///
    /// Stored as a `Vec<Address>` in instance storage so it shares the vault
    /// instance's lifetime. Read by `get_users_with_shares` for indexer
    /// pagination; entries are never removed, so fully-withdrawn users leave a
    /// stale slot that is filtered out at read time.
    UserSharesIndex,
    /// Migration target vault address (#637).
    ///
    /// Set by the owner to enable vault migration during contract upgrades.
    /// Users can migrate their shares to the new vault via `migrate_shares`.
    MigrationTarget,
    /// Migration pause state (#637).
    ///
    /// When true, users cannot migrate shares even if a migration target is set.
    /// The owner can pause migration independently of the main vault pause.
    MigrationPaused,
    /// User's locked shares (key: user Address) (#636).
    ///
    /// Number of shares locked by the user for boosted APY. Locked shares
    /// cannot be withdrawn until the lock period expires.
    LockedShares(Address),
    /// Latest ML-model APY prediction for a given protocol (#650).
    /// Written by the agent via `submit_apy_prediction`. Keyed by protocol symbol.
    ApyPrediction(Symbol),
    /// Cumulative suspected MEV loss across all rebalance operations (#658).
    /// Incremented by `submit_mev_report` when the agent detects extraction.
    CumulativeMevLoss,
    /// Count of rebalance operations where MEV extraction was suspected (#658).
    MevIncidentCount,
    /// Maximum acceptable MEV loss per rebalance in stroops, configured by
    /// the owner. When a reported loss exceeds this value, an alert event is
    /// emitted (#658). Zero means no threshold is set.
    MaxAcceptableMevLoss,
    /// User's lock expiry ledger (key: user Address) (#636).
    ///
    /// The ledger number at which the user's locked shares can be unlocked.
    /// Set when shares are locked and used to enforce lock period.
    LockExpiry(Address),
    /// Minimum number of ledgers a user must wait after depositing before
    /// they can withdraw. Configurable by the owner. When absent (or zero)
    /// no holding period is enforced. Used for flash-loan protection (#659).
    MinHoldingPeriod,
    /// Ledger sequence of the most recent deposit for a given user (#659).
    /// Written on every successful `deposit`. Used together with
    /// `MinHoldingPeriod` to enforce the flash-loan protection window.
    LastDepositLedger(Address),
    /// Owner-configured call allowance and ledger window for a rate-limit category.
    /// Appended to preserve the serialized discriminants of existing keys.
    RateLimitConfig(Symbol),
    /// Global fixed-window rate-limit usage for a category.
    RateLimitGlobalState(Symbol),
    /// Per-user fixed-window rate-limit usage for a category.
    RateLimitUserState(Address, Symbol),
    /// Maximum entries accepted by `batch_deposit`; `0` means unlimited.
    MaxBatchSize,
}

/// Owner-configured allowance for one rate-limit category.
///
/// `max_calls == 0` and `window_ledgers == 0` disables the category. An
/// enabled configuration uses a fixed window that starts with the first
/// accepted call and resets once `window_ledgers` have elapsed.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimitConfig {
    /// Maximum number of accepted calls during one window.
    pub max_calls: u32,
    /// Length of the window in ledger sequences.
    pub window_ledgers: u32,
}

/// Usage of a rate-limit bucket.
///
/// The state is deliberately stored independently from the configuration so
/// changing a limit cannot rewrite or enumerate every user's bucket.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimitState {
    /// Ledger at which the current window began.
    pub window_start: u32,
    /// Number of accepted calls in the current window.
    pub calls: u32,
}

// ============================================================================
// EVENTS
// ============================================================================

/// Emitted when a user deposits USDC into the vault.
///
/// AI agents monitor this event to detect new deposits and initiate
/// yield deployment. External indexers use this for transaction tracking.
///
/// # Topics
/// - `0`: `SymbolShort("deposit")` (`TOPIC_DEPOSIT`) - Event identifier
/// - `1`: `Address` - the depositing user, published as an indexed topic so
///   indexers can filter by user without scanning payloads
#[contracttype]
pub struct DepositEvent {
    /// The user who made the deposit
    pub user: Address,
    /// Amount of USDC deposited (7 decimal places)
    pub amount: i128,
    /// Number of vault shares minted for this deposit
    pub shares: i128,
}

/// Emitted when a user withdraws USDC from the vault.
///
/// AI agents monitor this event to update their internal records.
/// External indexers use this for transaction tracking.
///
/// # Topics
/// - `0`: `SymbolShort("withdraw")` (`TOPIC_WITHDRAW`) - Event identifier
/// - `1`: `Address` - the withdrawing user, published as an indexed topic so
///   indexers can filter by user without scanning payloads
#[contracttype]
pub struct WithdrawEvent {
    /// The user who made the withdrawal
    pub user: Address,
    /// Amount of USDC withdrawn (7 decimal places)
    pub amount: i128,
    /// Number of vault shares burned for this withdrawal
    pub shares: i128,
}

/// Emitted when the AI agent rebalances funds between yield strategies.
///
/// This event signals that the agent is moving funds between different
/// yield-generating protocols. The protocol symbol indicates the new
/// target allocation.
///
/// # Topics
/// - `SymbolShort("rebalance")` (`TOPIC_REBALANCE`) - Event identifier
#[contracttype]
pub struct RebalanceEvent {
    /// The target protocol (supported: "blend", "none")
    pub protocol: Symbol,
    /// Expected APY in basis points (e.g., 850 = 8.5%)
    pub expected_apy: i128,
    /// Status: "success", "failed", "partial", or "noop" (no funds moved)
    pub status: Symbol,
    /// Amount attempted to be moved
    pub amount_attempted: i128,
    /// Amount actually moved
    pub amount_moved: i128,
    /// Amount supplied into the target protocol
    pub amount_supplied: i128,
    /// Amount withdrawn from the current protocol
    pub amount_withdrawn: i128,
}

/// Emitted when accrued yield is harvested and compounded.
///
/// # Topics
/// - `SymbolShort("harvest")` (`TOPIC_HARVEST`) - Event identifier
#[contracttype]
pub struct HarvestEvent {
    /// The protocol harvested from (e.g., "blend", "dex")
    pub protocol: Symbol,
    /// Amount withdrawn and re-deposited
    pub amount_harvested: i128,
}

/// Emitted when the owner triggers an emergency harvest fallback.
///
/// This is a distinct event from [`HarvestEvent`] so that indexers can
/// differentiate agent-initiated harvests from owner-initiated emergency
/// harvests. The emergency harvest is gated by owner auth rather than agent
/// auth, allowing yield compounding during an agent-key outage or rotation.
///
/// # Topics
/// - `SymbolShort("em_harv")` (`TOPIC_EMERGENCY_HARVEST`) - Event identifier
#[contracttype]
pub struct EmergencyHarvestEvent {
    /// The protocol harvested from (e.g., "blend", "dex")
    pub protocol: Symbol,
    /// Amount withdrawn and re-deposited
    pub amount_harvested: i128,
}

/// Emitted when [`DataKey::CurrentProtocol`] changes.
///
/// Indexers should prefer this event over inferring protocol from rebalance
/// events alone.
///
/// # Topics
/// - `SymbolShort("proto_chg")` (`TOPIC_PROTOCOL_CHANGED`) - Event identifier
#[contracttype]
pub struct ProtocolChangedEvent {
    /// Protocol the vault was deployed to before the change
    /// (`"blend"`, `"dex"`, or `"none"`)
    pub old_protocol: Symbol,
    /// Protocol the vault is deployed to after the change
    /// (`"blend"`, `"dex"`, or `"none"`)
    pub new_protocol: Symbol,
}

/// Combined pause/unpause payload.
///
/// # Reserved
/// This struct is part of the contract type surface but is **not currently
/// emitted**. `pause`, `unpause`, and `emergency_pause` publish the dedicated
/// [`VaultPausedEvent`], [`VaultUnpausedEvent`], and [`EmergencyPausedEvent`]
/// payloads instead. Indexers should not subscribe to this type.
///
/// # Topics
/// - None (never published).
#[contracttype]
pub struct PauseEvent {
    /// `true` if the vault is now paused, `false` if it is now unpaused
    pub paused: bool,
    /// Address that triggered the pause/unpause transition
    pub caller: Address,
}

/// Emitted once when the vault is initialized via `initialize`.
///
/// # Topics
/// - `SymbolShort("init")` (`TOPIC_INIT`) - Event identifier
#[contracttype]
pub struct VaultInitializedEvent {
    /// Initial owner address, authorized for every administrative entrypoint
    /// (pause, caps, pool configuration, upgrades, ownership transfer)
    pub owner: Address,
    /// Authorized AI agent address; the only address allowed to call
    /// `rebalance` and `update_total_assets`
    pub agent: Address,
    /// USDC token contract address; the only token the vault accepts
    pub usdc_token: Address,
    /// TVL cap applied at initialization, in USDC raw units (7 decimals)
    pub tvl_cap: i128,
}

/// Initialization-failure payload.
///
/// # Reserved
/// This struct is part of the contract type surface but is **not currently
/// emitted**. A failed `initialize` panics with a [`VaultError`] and the
/// transaction is reverted, so no event survives. Indexers should observe the
/// transaction result code instead.
///
/// # Topics
/// - None (never published).
#[contracttype]
pub struct InitFailedEvent {
    /// Address that attempted the initialization
    pub caller: Address,
    /// Short reason code describing why initialization was rejected
    pub reason: Symbol,
}

/// Emitted when the vault is paused via `pause`.
///
/// # Topics
/// - `SymbolShort("paused")` (`TOPIC_PAUSED`) - Event identifier
#[contracttype]
pub struct VaultPausedEvent {
    /// Owner address that triggered the pause (read from storage, not the caller argument)
    pub owner: Address,
}

/// Emitted when the vault is unpaused via `unpause`.
///
/// # Topics
/// - `SymbolShort("unpaused")` (`TOPIC_UNPAUSED`) - Event identifier
#[contracttype]
pub struct VaultUnpausedEvent {
    /// Owner address that triggered the unpause (read from storage, not the caller argument)
    pub owner: Address,
}

/// Emitted when the vault is emergency-paused via `emergency_pause`.
///
/// Distinguished from [`VaultPausedEvent`] so monitoring can alert on
/// emergency halts specifically.
///
/// # Topics
/// - `SymbolShort("emerg")` (`TOPIC_EMERGENCY_PAUSED`) - Event identifier
#[contracttype]
pub struct EmergencyPausedEvent {
    /// Owner address that triggered the emergency pause (read from storage, not the caller argument)
    pub owner: Address,
}

#[contracttype]
pub struct CircuitBreakerTriggeredEvent {
    pub reason: String,
    pub threshold_value: i128,
}

#[contracttype]
pub struct CircuitBreakerResetEvent {
    pub owner: Address,
}

/// Emitted when the TVL cap is updated via `set_tvl_cap`.
///
/// # Topics
/// - `SymbolShort("tvl_cap")` (`TOPIC_TVL_CAP_UPDATED`) - Event identifier
#[contracttype]
pub struct TvlCapUpdatedEvent {
    /// TVL cap before the change, in USDC raw units (7 decimals)
    pub old_cap: i128,
    /// TVL cap after the change, in USDC raw units (7 decimals)
    pub new_cap: i128,
    /// Ledger timestamp when the cap was changed
    pub timestamp: u64,
}

/// Emitted when the per-user deposit cap is updated via `set_user_deposit_cap`.
///
/// # Topics
/// - `SymbolShort("user_cap")` (`TOPIC_USER_CAP_UPDATED`) - Event identifier
#[contracttype]
pub struct UserDepositCapUpdatedEvent {
    /// Per-user deposit cap before the change, in USDC raw units (7 decimals)
    pub old_cap: i128,
    /// Per-user deposit cap after the change, in USDC raw units (7 decimals)
    pub new_cap: i128,
    /// Ledger timestamp when the cap was changed
    pub timestamp: u64,
}

/// Emitted when both user deposit cap and TVL cap are updated.
///
/// # Topics
/// - `SymbolShort("caps_upd")` (`TOPIC_CAPS_UPDATED`) - Event identifier
#[contracttype]
pub struct CapsUpdatedEvent {
    /// Per-user deposit cap before the change, in USDC raw units (7 decimals)
    pub old_user_cap: i128,
    /// Per-user deposit cap after the change, in USDC raw units (7 decimals)
    pub new_user_cap: i128,
    /// TVL cap before the change, in USDC raw units (7 decimals)
    pub old_tvl_cap: i128,
    /// TVL cap after the change, in USDC raw units (7 decimals)
    pub new_tvl_cap: i128,
}

/// Emitted by the deprecated `set_limits` function only.
///
/// # Deprecated
/// Use `DepositLimitsUpdatedEvent` (topic `"dep_lim"`) for per-transaction
/// deposit-limit changes, and `TvlCapUpdatedEvent` / `UserDepositCapUpdatedEvent`
/// for cap changes. This event is retained only for backward compatibility with
/// indexers that still observe the `set_limits` call path.
///
/// # Topics
/// - `SymbolShort("l_upd")` (`TOPIC_LIMITS_UPDATED`) - Event identifier
#[contracttype]
pub struct LimitsUpdatedEvent {
    /// Minimum per-transaction deposit before the change, in USDC raw units (7 decimals)
    pub old_min: i128,
    /// Minimum per-transaction deposit after the change, in USDC raw units (7 decimals)
    pub new_min: i128,
    /// Maximum per-transaction deposit before the change, in USDC raw units (7 decimals)
    pub old_max: i128,
    /// Maximum per-transaction deposit after the change, in USDC raw units (7 decimals)
    pub new_max: i128,
}

/// Emitted when per-transaction deposit limits (min/max per deposit) are updated
/// via `set_deposit_limits`.
///
/// # Topics
/// - `SymbolShort("dep_lim")` (`TOPIC_DEPOSIT_LIMITS_UPDATED`) - Event identifier
#[contracttype]
pub struct DepositLimitsUpdatedEvent {
    /// Minimum per-transaction deposit before the change, in USDC raw units (7 decimals)
    pub old_min: i128,
    /// Minimum per-transaction deposit after the change, in USDC raw units (7 decimals)
    pub new_min: i128,
    /// Maximum per-transaction deposit before the change, in USDC raw units (7 decimals)
    pub old_max: i128,
    /// Maximum per-transaction deposit after the change, in USDC raw units (7 decimals)
    pub new_max: i128,
}

/// Emitted when the minimum rebalance cooldown is updated via
/// `set_rebalance_cooldown`.
///
/// # Topics
/// - `SymbolShort("reb_cd")` (`TOPIC_REBALANCE_COOLDOWN_UPDATED`) - Event identifier
#[contracttype]
pub struct RebalanceCooldownUpdatedEvent {
    /// Minimum ledgers between rebalances before the change, or `0` if disabled
    pub old_interval: u32,
    /// Minimum ledgers between rebalances after the change, or `0` if disabled
    pub new_interval: u32,
}

/// Emitted when the shared Blend/DEX approval TTL is updated via
/// `set_approval_ttl`.
///
/// # Topics
/// - `SymbolShort("ttl_upd")` (`TOPIC_APPROVAL_TTL_UPDATED`) - Event identifier
#[contracttype]
pub struct ApprovalTtlUpdatedEvent {
    /// Approval TTL in ledgers before the change
    pub old_ttl: u32,
    /// Approval TTL in ledgers after the change
    pub new_ttl: u32,
}

/// Emitted when the owner changes the circuit-breaker threshold via
/// `set_max_consecutive_failures`.
///
/// # Topics
/// - `SymbolShort("maxf_upd")` (`TOPIC_MAX_FAILURES_UPDATED`) - Event identifier
#[contracttype]
pub struct MaxConsecutiveFailuresUpdatedEvent {
    /// Effective threshold before the change (the default if never configured)
    pub old_threshold: u32,
    /// Threshold after the change
    pub new_threshold: u32,
}

/// Emitted when the AI agent address changes.
///
/// Published alongside [`AgentUpdateConfirmedEvent`] by `confirm_agent_update`
/// so legacy indexers that only track this topic keep working.
///
/// # Topics
/// - `SymbolShort("agent")` (`TOPIC_AGENT_UPDATED`) - Event identifier
#[contracttype]
pub struct AgentUpdatedEvent {
    /// Agent address that was authorized before the change
    pub old_agent: Address,
    /// Agent address authorized after the change
    pub new_agent: Address,
}

/// Emitted when an agent update is proposed via `update_agent()` (timelock step 1).
///
/// # Topics
/// - `SymbolShort("agt_prop")` (`TOPIC_AGENT_UPDATE_PROPOSED`) - Event identifier
#[contracttype]
pub struct AgentUpdateProposedEvent {
    /// Agent address currently authorized; remains active for the whole timelock window
    pub old_agent: Address,
    /// Proposed agent address, activated only by `confirm_agent_update`
    pub new_agent: Address,
    /// Ledger at which `confirm_agent_update()` becomes callable.
    pub effective_ledger: u32,
}

/// Emitted when a pending agent update is confirmed via `confirm_agent_update()` (timelock step 2).
///
/// # Topics
/// - `SymbolShort("agt_conf")` (`TOPIC_AGENT_UPDATE_CONFIRMED`) - Event identifier
#[contracttype]
pub struct AgentUpdateConfirmedEvent {
    /// Agent address that was authorized before confirmation
    pub old_agent: Address,
    /// Agent address now authorized to call `rebalance` and `update_total_assets`
    pub new_agent: Address,
}

/// Emitted when a pending agent update is cancelled via `cancel_agent_update()`.
///
/// # Topics
/// - `SymbolShort("agt_cncl")` (`TOPIC_AGENT_UPDATE_CANCELLED`) - Event identifier
#[contracttype]
pub struct AgentUpdateCancelledEvent {
    /// Agent address that stays authorized; cancelling never changes the active agent
    pub old_agent: Address,
    /// Agent address that had been proposed and is now discarded
    pub proposed_new_agent: Address,
}

/// Emitted when an ownership transfer is initiated via `transfer_ownership`
/// (step 1 of the two-step transfer).
///
/// # Topics
/// - `SymbolShort("own_init")` (`TOPIC_OWNERSHIP_INITIATED`) - Event identifier
#[contracttype]
pub struct OwnershipTransferInitiatedEvent {
    /// Owner address that remains in control until the transfer is accepted
    pub current_owner: Address,
    /// Proposed owner address that must call `accept_ownership` to take over
    pub pending_owner: Address,
}

/// Emitted when an ownership transfer completes via `accept_ownership`
/// (step 2 of the two-step transfer).
///
/// # Topics
/// - `SymbolShort("own_xfer")` (`TOPIC_OWNERSHIP_TRANSFERRED`) - Event identifier
#[contracttype]
pub struct OwnershipTransferredEvent {
    /// Owner address that held control before the transfer
    pub old_owner: Address,
    /// Owner address now authorized for administrative entrypoints
    pub new_owner: Address,
}

/// Emitted when a pending ownership transfer is cancelled via
/// `cancel_ownership_transfer`.
///
/// # Topics
/// - `SymbolShort("own_cncl")` (`TOPIC_OWNERSHIP_CANCELLED`) - Event identifier
#[contracttype]
pub struct OwnershipTransferCancelledEvent {
    /// Owner address that stays in control; cancelling never changes the owner
    pub owner: Address,
    /// Pending owner address that was discarded
    pub cancelled_pending: Address,
}

/// Information about a pending ownership transfer.
///
/// Returned by `get_pending_ownership` when a transfer is in progress.
#[contracttype]
pub struct PendingOwnershipInfo {
    pub pending_owner: Address,
    pub timelock_expiry: u64,
}

/// Emitted when the agent reports new total assets via `update_total_assets`
/// (yield accrual or loss reporting).
///
/// Because share price is derived from `TotalAssets`, this event is the
/// authoritative signal that the exchange rate has moved.
///
/// # Topics
/// - `SymbolShort("assets")` (`TOPIC_ASSETS_UPDATED`) - Event identifier
#[contracttype]
pub struct AssetsUpdatedEvent {
    /// Total managed assets before the update, in USDC raw units (7 decimals)
    pub old_total: i128,
    /// Total managed assets after the update, in USDC raw units (7 decimals)
    pub new_total: i128,
}

/// Emitted when the contract is upgraded to a new WASM implementation.
///
/// # Topics
/// - `SymbolShort("upgraded")` (`TOPIC_UPGRADED`) - Event identifier
#[contracttype]
pub struct UpgradedEvent {
    /// The contract version before the upgrade
    pub old_version: u32,
    /// The contract version after the upgrade
    pub new_version: u32,
}

/// Emitted when an upgrade is scheduled via `schedule_upgrade()` (timelock step 1). (#316)
///
/// # Topics
/// - `SymbolShort("upg_sched")` (`TOPIC_UPGRADE_SCHEDULED`) - Event identifier
#[contracttype]
pub struct UpgradeScheduledEvent {
    /// Hash of the WASM binary that will be activated once the timelock elapses.
    pub new_wasm_hash: BytesN<32>,
    /// Ledger at which `execute_upgrade()` becomes callable.
    pub effective_ledger: u32,
}

/// Emitted when a pending upgrade is cancelled via `cancel_upgrade()`. (#316)
///
/// # Topics
/// - `SymbolShort("upg_cncl")` (`TOPIC_UPGRADE_CANCELLED`) - Event identifier
#[contracttype]
pub struct UpgradeCancelledEvent {
    /// Hash of the WASM binary whose pending upgrade was cancelled.
    pub cancelled_wasm_hash: BytesN<32>,
}

/// Emitted when assets are supplied to Blend protocol.
///
/// # Topics
/// - `SymbolShort("blend_sup")` (`TOPIC_BLEND_SUPPLY`) - Event identifier
#[contracttype]
pub struct BlendSupplyEvent {
    /// The asset address (USDC)
    pub asset: Address,
    /// Actual amount transferred to Blend (may be less than requested due to pool limits)
    pub amount_actual: i128,
    /// Whether the supply was successful
    pub success: bool,
}

/// Emitted when assets are withdrawn from Blend protocol.
///
/// # Topics
/// - `SymbolShort("blend_wd")` (`TOPIC_BLEND_WITHDRAW`) - Event identifier
#[contracttype]
pub struct BlendWithdrawEvent {
    /// The asset address (USDC)
    pub asset: Address,
    /// Actual amount received from Blend (may be less than requested due to pool liquidity)
    pub amount_actual: i128,
    /// Whether the withdrawal succeeded
    pub success: bool,
}

/// Emitted when the Blend pool address is configured.
///
/// # Topics
/// - `SymbolShort("blend_cfg")` (`TOPIC_BLEND_POOL_CONFIGURED`) - Event identifier
#[contracttype]
pub struct BlendPoolConfiguredEvent {
    /// Previous Blend pool address, or None if it was not configured
    pub old_pool: Option<Address>,
    /// Newly configured Blend pool address
    pub new_pool: Address,
    /// Owner who triggered the configuration change
    pub owner: Address,
}

/// Emitted when assets are supplied to a DEX liquidity pool.
///
/// # Topics
/// - `SymbolShort("dex_sup")` (`TOPIC_DEX_SUPPLY`) - Event identifier
#[contracttype]
pub struct DexSupplyEvent {
    /// The asset address (USDC)
    pub asset: Address,
    /// Actual amount transferred to the DEX pool (may be less than requested due to slippage/limits)
    pub amount_actual: i128,
    /// Whether the supply was successful
    pub success: bool,
}

/// Emitted when assets are withdrawn from a DEX liquidity pool.
///
/// # Topics
/// - `SymbolShort("dex_wd")` (`TOPIC_DEX_WITHDRAW`) - Event identifier
#[contracttype]
pub struct DexWithdrawEvent {
    /// The asset address (USDC)
    pub asset: Address,
    /// Actual amount received from the DEX pool (may be less than requested due to liquidity)
    pub amount_actual: i128,
    /// Whether the withdrawal succeeded
    pub success: bool,
}

/// Emitted when the DEX pool address is configured.
///
/// # Topics
/// - `SymbolShort("dex_cfg")` (`TOPIC_DEX_POOL_CONFIGURED`) - Event identifier
#[contracttype]
pub struct DexPoolConfiguredEvent {
    /// Previous DEX pool address, or None if it was not configured
    pub old_pool: Option<Address>,
    /// Newly configured DEX pool address
    pub new_pool: Address,
    /// Owner who triggered the configuration change
    pub owner: Address,
}

/// Emitted when a rebalance aborts due to a protocol exit failure.
///
/// Emitted instead of panicking so the failure is observable on-chain without
/// reverting the transaction. State remains unchanged when this event fires.
///
/// # Topics
/// - `SymbolShort("reb_fail")` (`TOPIC_REBALANCE_FAILED`) - Event identifier
#[contracttype]
pub struct RebalanceFailedEvent {
    /// The protocol the vault was trying to exit
    pub from_protocol: Symbol,
    /// Short reason code ("exit_fail" = incomplete withdrawal)
    pub reason: Symbol,
}

/// Emitted when a user updates their investment strategy preference.
///
/// AI agents read this event to adjust yield deployment per user.
///
/// # Topics
/// - `0`: `SymbolShort("usr_strat")` (`TOPIC_USER_STRATEGY_UPDATED`) - Event identifier
/// - `1`: `Address` - the user whose strategy changed, published as an indexed
///   topic so agents can subscribe per user
#[contracttype]
pub struct UserStrategyUpdatedEvent {
    /// The user who updated their strategy
    pub user: Address,
    /// Previous strategy symbol ("conservative", "balanced", "growth", or "")
    pub old_strategy: Symbol,
    /// New strategy symbol
    pub new_strategy: Symbol,
}

/// Aggregate view of a single user's position, returned by
/// [`NeuroWealthVault::get_user_info`].
///
/// This is a return type, not an event: it is never published to the event log.
#[contracttype]
pub struct UserInfo {
    /// Deprecated compatibility field.
    ///
    /// This value is now the user's share-derived asset balance, not a separate
    /// stored principal record. Use `shares` plus share conversion helpers when
    /// exact accounting provenance matters.
    pub principal: i128,
    /// The user's vault share balance, representing proportional ownership of
    /// `TotalAssets`. Convert to USDC with
    /// [`NeuroWealthVault::convert_to_assets`].
    pub shares: i128,
}

/// Emitted when a user migrates their shares to a new vault (#637).
///
/// # Topics
/// - `SymbolShort("migrate")` (`TOPIC_MIGRATE`) - Event identifier
/// - `Address` - the migrating user, published as an indexed topic
#[contracttype]
pub struct SharesMigratedEvent {
    /// The user who migrated their shares
    pub user: Address,
    /// Old vault contract address
    pub old_vault: Address,
    /// New vault contract address
    pub new_vault: Address,
    /// Number of shares burned from old vault
    pub shares_burned: i128,
    /// Amount of assets (USDC) transferred to new vault
    pub assets_transferred: i128,
}

/// Emitted when the owner sets or updates the migration target vault (#637).
///
/// # Topics
/// - `SymbolShort("mig_tgt")` (`TOPIC_MIGRATION_TARGET_UPDATED`) - Event identifier
#[contracttype]
pub struct MigrationTargetUpdatedEvent {
    /// Previous migration target address, or None if not set
    pub old_target: Option<Address>,
    /// New migration target address
    pub new_target: Address,
    /// Owner who triggered the change
    pub owner: Address,
}

/// Emitted when migration is paused or unpaused by the owner (#637).
///
/// # Topics
/// - `SymbolShort("mig_pse")` (`TOPIC_MIGRATION_PAUSED`) - Event identifier
#[contracttype]
pub struct MigrationPausedEvent {
    /// `true` if migration is now paused, `false` if unpaused
    pub paused: bool,
    /// Owner who triggered the pause/unpause
    pub owner: Address,
}

/// Emitted when a user locks their shares for boosted APY (#636).
///
/// # Topics
/// - `SymbolShort("lock")` (`TOPIC_SHARES_LOCKED`) - Event identifier
/// - `Address` - the user locking shares, published as an indexed topic
#[contracttype]
pub struct SharesLockedEvent {
    /// The user who locked their shares
    pub user: Address,
    /// Number of shares locked
    pub shares_locked: i128,
    /// Lock duration in days
    pub lock_duration_days: u32,
    /// Boost multiplier applied (e.g., 1.1x, 1.25x, 1.5x)
    pub boost_multiplier: u32,
    /// Ledger when lock expires
    pub expiry_ledger: u32,
}

/// Emitted when a user unlocks their shares (#636).
///
/// # Topics
/// - `SymbolShort("unlock")` (`TOPIC_SHARES_UNLOCKED`) - Event identifier
/// - `Address` - the user unlocking shares, published as an indexed topic
#[contracttype]
pub struct SharesUnlockedEvent {
    /// The user who unlocked their shares
    pub user: Address,
    /// Number of shares unlocked
    pub shares_unlocked: i128,
}

/// Emitted when a user performs an emergency withdrawal while vault is paused (#635).
///
/// # Topics
/// - `SymbolShort("em_wd")` (`TOPIC_EMERGENCY_WITHDRAWAL`) - Event identifier
/// - `Address` - the withdrawing user, published as an indexed topic
#[contracttype]
pub struct EmergencyWithdrawalEvent {
    /// The user who performed the emergency withdrawal
    pub user: Address,
    /// Amount of USDC withdrawn (7 decimal places)
    pub amount: i128,
    /// Number of shares burned
    pub shares: i128,
    /// Whether funds were taken from idle balance (true) or protocol (false)
    pub from_idle: bool,
}

/// On-chain record of an ML model APY forecast submitted by the agent (#650).
///
/// The off-chain LSTM / Prophet model produces a prediction for each supported
/// protocol. The agent writes the latest forecast on-chain so that the rebalance
/// decision and its inputs are fully auditable.
///
/// `predicted_apy_bps` is in basis points (100 bps = 1%). `confidence_bps` is
/// the model's confidence expressed the same way (e.g., 8000 bps = 80%).
/// `horizon_ledgers` is how many ledgers ahead the prediction covers.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ApyPrediction {
    /// Protocol this prediction applies to ("blend", "dex", …).
    pub protocol: Symbol,
    /// Predicted APY in basis points (100 bps = 1%).
    pub predicted_apy_bps: i128,
    /// 1-hour-ahead APY forecast in basis points.
    pub apy_1h_bps: i128,
    /// 6-hour-ahead APY forecast in basis points.
    pub apy_6h_bps: i128,
    /// 24-hour-ahead APY forecast in basis points.
    pub apy_24h_bps: i128,
    /// Model confidence (0–10000 bps, where 10000 = 100%).
    pub confidence_bps: u32,
    /// Ledger at which this prediction was submitted.
    pub submitted_at_ledger: u32,
}

/// Emitted when the off-chain agent reports suspected MEV extraction on a
/// rebalance transaction (#658).
///
/// Indexers should aggregate `estimated_loss_stroops` across incidents to
/// calculate the cumulative cost of MEV extraction to vault users.
///
/// # Topics
/// - `0`: `SymbolShort("mev_alert")` — Event identifier
#[contracttype]
pub struct MevExtractionSuspectedEvent {
    /// The rebalance protocol where MEV was suspected (e.g., "blend", "dex").
    pub protocol: Symbol,
    /// Agent-estimated amount lost to MEV in this transaction (stroops).
    pub estimated_loss_stroops: i128,
    /// The `min_out` value that was set for the rebalance that triggered the report.
    pub min_out_used: i128,
    /// Running total of suspected MEV losses since vault inception.
    pub cumulative_loss_stroops: i128,
    /// Number of MEV incidents recorded so far.
    pub incident_count: u32,
}

/// Emitted when a withdrawal attempt is blocked by the flash-loan protection
/// holding period (#659).
///
/// Indexers can use this event to track how often the protection triggers and
/// to flag wallet addresses that repeatedly attempt same-ledger withdrawals.
///
/// # Topics
/// - `0`: `SymbolShort("fl_block")` — Event identifier
/// - `1`: `Address` — the user whose withdrawal was blocked (indexed topic)
#[contracttype]
pub struct FlashLoanProtectionTriggeredEvent {
    /// The user whose withdrawal was rejected.
    pub user: Address,
    /// Ledger at which the user last deposited.
    pub last_deposit_ledger: u32,
    /// Current ledger at the time of the rejected withdrawal.
    pub current_ledger: u32,
    /// Configured minimum holding period (in ledgers).
    pub min_holding_period: u32,
}

/// Emitted when the owner changes a call rate-limit configuration.
///
/// # Topics
/// - `SymbolShort("rate_cfg")` (`TOPIC_RATE_LIMIT_CONFIG_UPDATED`)
#[contracttype]
pub struct RateLimitConfigUpdatedEvent {
    /// Category whose allowance changed.
    pub category: Symbol,
    /// Previous maximum accepted calls per window.
    pub old_max_calls: u32,
    /// Previous window length in ledgers.
    pub old_window_ledgers: u32,
    /// New maximum accepted calls per window.
    pub new_max_calls: u32,
    /// New window length in ledgers.
    pub new_window_ledgers: u32,
    /// Owner that made the change.
    pub owner: Address,
}

/// Emitted when the owner changes the maximum `batch_deposit` size.
///
/// # Topics
/// - `SymbolShort("batch_lim")` (`TOPIC_BATCH_SIZE_LIMIT_UPDATED`)
#[contracttype]
pub struct BatchSizeLimitUpdatedEvent {
    /// Previous maximum number of entries; zero means unlimited.
    pub old_max_entries: u32,
    /// New maximum number of entries; zero means unlimited.
    pub new_max_entries: u32,
    /// Owner that made the change.
    pub owner: Address,
}

/// Emitted immediately before a call is rejected because its rate-limit bucket
/// is exhausted.
///
/// The event is published before returning the contract error so Soroban
/// diagnostics and test environments can correlate the rejection with its
/// category and window. Indexers should also monitor the transaction error
/// code because a reverted transaction's event visibility is ledger-dependent.
///
/// # Topics
/// - `SymbolShort("rate_hit")` (`TOPIC_RATE_LIMIT_HIT`)
#[contracttype]
pub struct RateLimitExceededEvent {
    /// Category whose allowance was exhausted.
    pub category: Symbol,
    /// User bucket that was exhausted, or `None` for a global bucket.
    pub user: Option<Address>,
    /// Ledger at which the bucket was exhausted or the rejected call was attempted.
    pub current_ledger: u32,
    /// Start ledger of the exhausted window.
    pub window_start: u32,
    /// Configured maximum calls for the window.
    pub max_calls: u32,
    /// Number of accepted calls already recorded in the window.
    pub calls: u32,
}

// ============================================================================
// BLEND POOL CLIENT INTERFACE
// ============================================================================

/// Helper functions for interacting with Blend Protocol v2 pool contract.
///
/// Production Blend Soroban pools use request-based fund management:
/// - `submit_with_allowance(from, spender, to, requests)` — supply with token allowance
/// - `submit(from, to, requests)` — withdraw (request type 1)
/// - `balance(asset, user)` — supplied balance for the vault position
///
/// See `docs/BLEND_INTEGRATION_RESEARCH.md` and
/// https://docs.blend.capital/tech-docs/core-contracts/lending-pool/fund-management
struct BlendPoolClient;

#[derive(Clone)]
#[contracttype]
struct BlendRequest {
    request_type: u32,
    address: Address,
    amount: i128,
}

const BLEND_REQUEST_TYPE_SUPPLY: u32 = 0;
const BLEND_REQUEST_TYPE_WITHDRAW: u32 = 1;
const DEFAULT_USER_DEPOSIT_CAP: i128 = 10_000_000_000_i128;
const DEFAULT_MIN_DEPOSIT: i128 = 1_000_000_i128;
const DEFAULT_MAX_DEPOSIT: i128 = 10_000_000_000_i128;
/// Absolute upper bound the owner can configure via `set_deposit_limits`.
///
/// Comfortably above any realistic per-transaction limit, this rejects
/// configuration mistakes (e.g. an accidental `i128::MAX`) that would
/// otherwise disable the per-transaction maximum-deposit guard entirely.
const MAX_DEPOSIT_CEILING: i128 = 100_000_000_000_i128;
/// Default Blend token approval lifetime.
/// 100_000 ledgers × ~5s per ledger ≈ 5.7 days on Stellar mainnet.
pub(crate) const DEFAULT_APPROVAL_TTL: u32 = 100_000;
const MIN_APPROVAL_TTL: u32 = 1_000;
const MAX_APPROVAL_TTL: u32 = 500_000;

/// Default circuit-breaker threshold (#439): the number of consecutive failed
/// rebalances that trips an automatic emergency pause when the owner has not
/// configured a value via `set_max_consecutive_failures`.
const DEFAULT_MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Minimum ledger delay before a proposed agent update can be confirmed (~24 h on Stellar mainnet).
/// 17,280 ledgers × ~5 s per ledger ≈ 86,400 s = 24 h.
const AGENT_TIMELOCK_LEDGERS: u32 = 17_280;

/// Number of ledgers an upgrade must wait between `schedule_upgrade` and
/// `execute_upgrade` (#316). Same 24-hour window as the agent timelock, giving
/// users and operators a recovery window to react to a malicious or mistaken
/// upgrade proposal (and to `cancel_upgrade`) before new WASM takes effect.
const UPGRADE_TIMELOCK_LEDGERS: u32 = 17_280;

/// Rate-limit category for single and batched deposits.
///
/// The category symbols are part of the public API used by `set_rate_limit`.
/// Keep them at nine characters or fewer because Soroban short symbols have a
/// nine-character limit.
pub const RATE_LIMIT_DEPOSIT: Symbol = symbol_short!("deposit");
/// Rate-limit category for `withdraw` and `withdraw_all`.
pub const RATE_LIMIT_WITHDRAW: Symbol = symbol_short!("withdraw");
/// Rate-limit category for agent `rebalance` calls.
pub const RATE_LIMIT_REBALANCE: Symbol = symbol_short!("rebalance");
/// Rate-limit category for permissionless `touch_user_ttl` calls.
pub const RATE_LIMIT_TOUCH_TTL: Symbol = symbol_short!("touch_ttl");
/// Rate-limit category shared by all preview/conversion entrypoints.
pub const RATE_LIMIT_PREVIEW: Symbol = symbol_short!("preview");
/// Rate-limit category for `batch_deposit` calls.
pub const RATE_LIMIT_BATCH_DEPOSIT: Symbol = symbol_short!("batch_dep");

/// Default single-user deposit allowance: 100 calls per 720 ledgers (~1 hour).
const DEFAULT_DEPOSIT_RATE_LIMIT_MAX_CALLS: u32 = 100;
const DEFAULT_DEPOSIT_RATE_LIMIT_WINDOW: u32 = 720;
/// Default single-user withdrawal allowance: 100 calls per 720 ledgers (~1 hour).
const DEFAULT_WITHDRAW_RATE_LIMIT_MAX_CALLS: u32 = 100;
const DEFAULT_WITHDRAW_RATE_LIMIT_WINDOW: u32 = 720;
/// Default global rebalance allowance: 100 calls per 720 ledgers (~1 hour).
/// The owner should normally set a much lower value on production deployments;
/// this compatibility-safe default does not supersede `MinRebalanceInterval`.
const DEFAULT_REBALANCE_RATE_LIMIT_MAX_CALLS: u32 = 100;
const DEFAULT_REBALANCE_RATE_LIMIT_WINDOW: u32 = 720;
/// Default per-user TTL maintenance allowance: five calls per ledger.
const DEFAULT_TOUCH_TTL_RATE_LIMIT_MAX_CALLS: u32 = 5;
const DEFAULT_TOUCH_TTL_RATE_LIMIT_WINDOW: u32 = 1;
/// Default global preview/conversion allowance: 1,000 calls per ledger.
/// This bounds computational work without breaking clients that make several
/// previews while composing a transaction.
const DEFAULT_PREVIEW_RATE_LIMIT_MAX_CALLS: u32 = 1_000;
const DEFAULT_PREVIEW_RATE_LIMIT_WINDOW: u32 = 1;
/// Default per-user batch-deposit allowance: 100 calls per 720 ledgers.
const DEFAULT_BATCH_DEPOSIT_RATE_LIMIT_MAX_CALLS: u32 = 100;
const DEFAULT_BATCH_DEPOSIT_RATE_LIMIT_WINDOW: u32 = 720;
/// Maximum number of `(token, amount)` entries accepted by `batch_deposit` by default.
const DEFAULT_MAX_BATCH_SIZE: u32 = 50;

/// Minimum ledgers remaining before `touch_user_ttl` extends a user's `Shares` entry.
const USER_SHARES_TTL_THRESHOLD: u32 = 100;

/// Lock period constants for boosted APY tiers (#636).
/// Assuming ~5 seconds per ledger on Stellar mainnet.
const LOCK_30_DAYS_LEDGERS: u32 = 30 * 24 * 60 * 60 / 5; // ~518,400 ledgers
const LOCK_90_DAYS_LEDGERS: u32 = 90 * 24 * 60 * 60 / 5; // ~1,555,200 ledgers
const LOCK_180_DAYS_LEDGERS: u32 = 180 * 24 * 60 * 60 / 5; // ~3,110,400 ledgers

/// Boost multipliers for lock periods (#636).
/// Expressed in basis points (10000 = 1.0x, 11000 = 1.1x, etc.)
const BOOST_30_DAYS: u32 = 11000; // 1.1x
const BOOST_90_DAYS: u32 = 12500; // 1.25x
const BOOST_180_DAYS: u32 = 15000; // 1.5x
/// Target ledgers to extend a user's `Shares` entry to when maintaining TTL.
const USER_SHARES_TTL_EXTEND_TO: u32 = 100;
/// Default ledgers kept alive for Blend token approvals.
///
/// The approval expiration ledger is calculated as:
/// `current_ledger_sequence + ApprovalTtl`.
const DEFAULT_BLEND_APPROVAL_TTL: u32 = 100_000;

use topics::{
    TOPIC_AGENT_UPDATED, TOPIC_AGENT_UPDATE_CANCELLED, TOPIC_AGENT_UPDATE_CONFIRMED,
    TOPIC_AGENT_UPDATE_PROPOSED, TOPIC_APPROVAL_TTL_UPDATED, TOPIC_ASSETS_UPDATED,

    TOPIC_BATCH_SIZE_LIMIT_UPDATED, TOPIC_BLEND_POOL_CONFIGURED, TOPIC_BLEND_SUPPLY,
    TOPIC_BLEND_WITHDRAW, TOPIC_CAPS_UPDATED, TOPIC_DEPOSIT, TOPIC_DEPOSIT_LIMITS_UPDATED,
    TOPIC_DEX_POOL_CONFIGURED, TOPIC_DEX_SUPPLY, TOPIC_DEX_WITHDRAW, TOPIC_EMERGENCY_HARVEST,
    TOPIC_EMERGENCY_PAUSED, TOPIC_EMERGENCY_WITHDRAWAL, TOPIC_HARVEST, TOPIC_INIT,
    TOPIC_LIMITS_UPDATED, TOPIC_MIGRATE, TOPIC_MIGRATION_PAUSED, TOPIC_MIGRATION_TARGET_UPDATED,
    TOPIC_OWNERSHIP_CANCELLED, TOPIC_OWNERSHIP_INITIATED, TOPIC_OWNERSHIP_TRANSFERRED,
    TOPIC_PAUSED, TOPIC_PROTOCOL_CHANGED, TOPIC_RATE_LIMIT_CONFIG_UPDATED, TOPIC_RATE_LIMIT_HIT,
    TOPIC_REBALANCE, TOPIC_REBALANCE_COOLDOWN_UPDATED, TOPIC_REBALANCE_FAILED, TOPIC_SHARES_LOCKED,
    TOPIC_SHARES_UNLOCKED, TOPIC_TVL_CAP_UPDATED, TOPIC_UNPAUSED, TOPIC_UPGRADED,
    TOPIC_UPGRADE_CANCELLED, TOPIC_UPGRADE_SCHEDULED, TOPIC_USER_CAP_UPDATED,
    TOPIC_USER_STRATEGY_UPDATED, TOPIC_WITHDRAW,
    TOPIC_BLEND_POOL_CONFIGURED, TOPIC_BLEND_SUPPLY, TOPIC_BLEND_WITHDRAW, TOPIC_CAPS_UPDATED,
    TOPIC_DEPOSIT, TOPIC_DEPOSIT_LIMITS_UPDATED, TOPIC_DEX_POOL_CONFIGURED, TOPIC_DEX_SUPPLY,
    TOPIC_DEX_WITHDRAW, TOPIC_EMERGENCY_HARVEST, TOPIC_EMERGENCY_PAUSED, TOPIC_EMERGENCY_WITHDRAWAL,
    TOPIC_HARVEST, TOPIC_INIT, TOPIC_LIMITS_UPDATED, TOPIC_MIGRATE, TOPIC_MIGRATION_PAUSED,
    TOPIC_MIGRATION_TARGET_UPDATED, TOPIC_OWNERSHIP_CANCELLED, TOPIC_OWNERSHIP_INITIATED,
    TOPIC_OWNERSHIP_TRANSFERRED, TOPIC_PAUSED, TOPIC_PROTOCOL_CHANGED, TOPIC_REBALANCE,
    TOPIC_REBALANCE_COOLDOWN_UPDATED, TOPIC_REBALANCE_FAILED, TOPIC_TVL_CAP_UPDATED,
    TOPIC_UNPAUSED, TOPIC_UPGRADED, TOPIC_UPGRADE_CANCELLED, TOPIC_UPGRADE_SCHEDULED,
    TOPIC_USER_CAP_UPDATED, TOPIC_USER_STRATEGY_UPDATED, TOPIC_WITHDRAW,
    TOPIC_MAX_FAILURES_UPDATED,

};

impl BlendPoolClient {
    /// Deposits assets to the Blend pool.
    ///
    /// Uses Blend's `submit_with_allowance()` function with a supply request (type 0).
    /// Reference: https://docs.blend.capital/tech-docs/core-contracts/lending-pool/fund-management
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `pool_address` - The Blend pool contract address
    /// * `asset` - The asset token address (USDC)
    /// * `amount` - Amount of assets to deposit
    /// * `to` - Address to receive the pool tokens (vault address)
    ///
    /// # Returns
    /// The amount of assets actually supplied (returned by Blend)
    ///
    /// # Panics
    /// - If the Blend pool call fails
    /// - If the pool status is frozen (status > 3)
    fn supply(
        env: &Env,
        pool_address: &Address,
        asset: &Address,
        amount: i128,
        to: &Address,
    ) -> i128 {
        use soroban_sdk::{vec, IntoVal, Symbol};

        // Track vault balance before to calculate actual supplied amount
        let token_client = token::Client::new(env, asset);
        let vault_address = env.current_contract_address();
        let balance_before = token_client.balance(&vault_address);

        // Create supply request (type 0 = supply)
        let request = BlendRequest {
            request_type: BLEND_REQUEST_TYPE_SUPPLY,
            address: asset.clone(),
            amount,
        };
        let requests: Vec<BlendRequest> = vec![env, request];

        // submit_with_allowance(from: Address, spender: Address, to: Address, requests: Vec<Request>)
        let args: Vec<Val> = vec![
            env,
            to.into_val(env),       // from: vault address (token owner)
            to.into_val(env),       // spender: vault address (authorized spender)
            to.into_val(env),       // to: vault address (receives pool position)
            requests.into_val(env), // requests: vector of supply requests
        ];

        // Invoke Blend's submit_with_allowance function
        env.invoke_contract::<Val>(
            pool_address,
            &Symbol::new(env, "submit_with_allowance"),
            args,
        );

        // Calculate actual amount supplied by balance change
        let balance_after = token_client.balance(&vault_address);
        balance_before.saturating_sub(balance_after)
    }

    /// Redeems assets from the Blend pool.
    ///
    /// Uses Blend's `submit()` function with a withdraw request (type 1).
    /// Reference: https://docs.blend.capital/tech-docs/core-contracts/lending-pool/fund-management
    fn withdraw(
        env: &Env,
        pool_address: &Address,
        asset: &Address,
        amount: i128,
        to: &Address,
    ) -> i128 {
        use soroban_sdk::{vec, IntoVal, Symbol};

        // Track vault balance before to calculate actual withdrawn amount
        let token_client = token::Client::new(env, asset);
        let vault_address = env.current_contract_address();
        let balance_before = token_client.balance(&vault_address);

        // Create withdraw request (type 1 = withdraw)
        let request = BlendRequest {
            request_type: BLEND_REQUEST_TYPE_WITHDRAW,
            address: asset.clone(),
            amount,
        };
        let requests: Vec<BlendRequest> = vec![env, request];

        // submit(from: Address, to: Address, requests: Vec<Request>)
        let args: Vec<Val> = vec![
            env,
            to.into_val(env),       // from: vault address (position owner)
            to.into_val(env),       // to: vault address (receives withdrawn assets)
            requests.into_val(env), // requests: vector of withdraw requests
        ];

        // Invoke Blend's submit function
        env.invoke_contract::<Val>(pool_address, &Symbol::new(env, "submit"), args);

        // Calculate actual amount withdrawn by balance change
        let balance_after = token_client.balance(&vault_address);
        balance_after.saturating_sub(balance_before)
    }

    /// Gets the balance of assets supplied to the Blend pool.
    fn get_balance(env: &Env, pool_address: &Address, asset: &Address, user: &Address) -> i128 {
        use soroban_sdk::{vec, IntoVal, Symbol};
        let args: Vec<Val> = vec![env, asset.into_val(env), user.into_val(env)];
        env.invoke_contract::<i128>(pool_address, &Symbol::new(env, "balance"), args)
    }
}

// ============================================================================
// DEX LIQUIDITY POOL CLIENT INTERFACE
// ============================================================================

/// Helper functions for interacting with a Stellar DEX liquidity pool contract.
///
/// The vault provides single-asset (USDC) liquidity to a DEX pool to execute the
/// Balanced/Growth strategies described in the README. The pool is treated as a
/// single-asset adapter exposing:
/// - `add_liquidity(from, asset, amount, min_out)` — supply liquidity after USDC `approve`
/// - `remove_liquidity(to, asset, amount, min_out)` — withdraw liquidity
/// - `balance(asset, user)` — the vault's current liquidity position
///
/// Actual amounts are derived from the vault's USDC balance delta, mirroring the
/// Blend integration so partial fills (slippage) are observable on-chain.
///
/// See `docs/DEX_INTEGRATION.md` for the full interface research and rationale.
struct DexPoolClient;

impl DexPoolClient {
    /// Supplies assets to the DEX liquidity pool via `add_liquidity`.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `pool_address` - The DEX pool contract address
    /// * `asset` - The asset token address (USDC)
    /// * `amount` - Amount of assets to supply
    /// * `min_out` - Minimum accepted liquidity (forwarded to the pool for slippage protection)
    /// * `to` - Address providing/owning the liquidity position (vault address)
    ///
    /// # Returns
    /// The amount of assets actually supplied (derived from the vault balance delta).
    fn supply(
        env: &Env,
        pool_address: &Address,
        asset: &Address,
        amount: i128,
        min_out: i128,
        to: &Address,
    ) -> i128 {
        use soroban_sdk::{vec, IntoVal, Symbol};

        let token_client = token::Client::new(env, asset);
        let vault_address = env.current_contract_address();
        let balance_before = token_client.balance(&vault_address);

        // add_liquidity(from: Address, asset: Address, amount: i128, min_out: i128)
        let args: Vec<Val> = vec![
            env,
            to.into_val(env),     // from: vault address (liquidity provider)
            asset.into_val(env),  // asset: USDC token
            amount.into_val(env), // amount: desired liquidity
            min_out.into_val(env),
        ];

        env.invoke_contract::<Val>(pool_address, &Symbol::new(env, "add_liquidity"), args);

        let balance_after = token_client.balance(&vault_address);
        balance_before.saturating_sub(balance_after)
    }

    /// Removes assets from the DEX liquidity pool via `remove_liquidity`.
    fn withdraw(
        env: &Env,
        pool_address: &Address,
        asset: &Address,
        amount: i128,
        min_out: i128,
        to: &Address,
    ) -> i128 {
        use soroban_sdk::{vec, IntoVal, Symbol};

        let token_client = token::Client::new(env, asset);
        let vault_address = env.current_contract_address();
        let balance_before = token_client.balance(&vault_address);

        // remove_liquidity(to: Address, asset: Address, amount: i128, min_out: i128)
        let args: Vec<Val> = vec![
            env,
            to.into_val(env),     // to: vault address (receives withdrawn assets)
            asset.into_val(env),  // asset: USDC token
            amount.into_val(env), // amount: liquidity to remove
            min_out.into_val(env),
        ];

        env.invoke_contract::<Val>(pool_address, &Symbol::new(env, "remove_liquidity"), args);

        let balance_after = token_client.balance(&vault_address);
        balance_after.saturating_sub(balance_before)
    }

    /// Gets the vault's current liquidity position in the DEX pool.
    fn get_balance(env: &Env, pool_address: &Address, asset: &Address, user: &Address) -> i128 {
        use soroban_sdk::{vec, IntoVal, Symbol};
        let args: Vec<Val> = vec![env, asset.into_val(env), user.into_val(env)];
        env.invoke_contract::<i128>(pool_address, &Symbol::new(env, "balance"), args)
    }
}

// ============================================================================
// CONTRACT
// ============================================================================

/// NeuroWealth Vault - AI-Managed DeFi Yield Vault on Stellar
///
/// A non-custodial vault that accepts USDC deposits and allows an authorized
/// AI agent to automatically deploy those funds across various yield-generating
/// protocols on the Stellar blockchain.
///
/// # Security Model
///
/// - Users can only withdraw their own funds (enforced via `require_auth()`)
/// - Only the designated AI agent can call `rebalance()`
/// - Only the owner can call administrative functions
/// - Minimum deposit: 1 USDC
/// - Maximum per-user deposit: configurable (default 10,000 USDC)
/// - Emergency pause functionality available to owner
///
/// # Upgradeability
///
/// This contract can be upgraded by the owner while preserving all storage state.
#[contract]
pub struct NeuroWealthVault;

#[contractimpl]
// Re-enables `missing_docs` for the contract's public entrypoints, which the
// crate-level allow would otherwise silence. Keep this attribute: it is what
// makes an undocumented `pub fn` fail CI (`clippy -D warnings`).
#[warn(missing_docs)]
impl NeuroWealthVault {
    #[inline]
    fn require(env: &Env, condition: bool, error: VaultError) {
        if !condition {
            panic_with_error!(env, error);
        }
    }

    /// The canonical "burned" Stellar account: the ed25519 public key whose
    /// 32-byte payload is all zeros. No known private key can sign for it.
    ///
    /// `soroban_sdk::Address` has no `Default` impl, so this is the
    /// zero-address sentinel used to reject burned addresses in `initialize`
    /// (issue #434).
    #[inline]
    fn zero_address(env: &Env) -> Address {
        Address::from_string(&String::from_str(
            env,
            "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
        ))
    }

    // ==========================================================================
    // INITIALIZATION
    // ==========================================================================

    /// Initializes the vault with required configuration.
    ///
    /// This function must be called exactly once after contract deployment
    /// to set up the vault's core configuration. After initialization,
    /// the vault is ready to accept deposits.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `deployer` - The deployer address authorizing the initialization.
    /// * `owner` - The initial owner address of the vault.
    /// * `agent` - The authorized AI agent address that can call rebalance().
    /// * `usdc_token` - The USDC token contract address.
    /// * `salt` - The salt used during deployment for verification.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `VaultInitializedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the vault has already been initialized (Agent key already exists).
    /// - If the caller is not the expected deployer.
    /// - If deployer authorization fails.
    /// - If `deployer`, `owner`, `agent`, or `usdc_token` is the zero address.
    pub fn initialize(
        env: Env,
        deployer: Address,
        owner: Address,
        agent: Address,
        usdc_token: Address,
        salt: BytesN<32>,
    ) {
        if env.storage().instance().has(&DataKey::Agent) {
            panic_with_error!(&env, VaultError::AlreadyInitialized);
        }

        // Reject the zero address for every role so a vault can never be
        // initialized with a burned/unusable address (issue #434).
        let zero = Self::zero_address(&env);
        if deployer == zero {
            panic!("deployer cannot be zero address");
        }
        if owner == zero {
            panic!("owner cannot be zero address");
        }
        if agent == zero {
            panic!("agent cannot be zero address");
        }
        if usdc_token == zero {
            panic!("usdc_token cannot be zero address");
        }

        // Verify the deployer is the one that actually deployed the contract
        let expected_contract_address = env
            .deployer()
            .with_address(deployer.clone(), salt)
            .deployed_address();
        if expected_contract_address != env.current_contract_address() {
            panic_with_error!(&env, VaultError::UnauthorizedDeployer);
        }

        // Verify the deployer is calling - this prevents front-running
        // The deployer must be the one calling initialize()
        deployer.require_auth();

        // Store the deployer address for future reference and signature verification
        env.storage().instance().set(&DataKey::Deployer, &deployer);

        let tvl_cap = DEFAULT_TVL_CAP;

        env.storage().instance().set(&DataKey::Agent, &agent);
        env.storage()
            .instance()
            .set(&DataKey::UsdcToken, &usdc_token);
        env.storage()
            .instance()
            .set(&DataKey::TotalDeposits, &0_i128);
        env.storage().instance().set(&DataKey::TotalShares, &0_i128);
        env.storage().instance().set(&DataKey::TotalAssets, &0_i128);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::Owner, &owner);
        env.storage().instance().set(&DataKey::TvLCap, &tvl_cap);
        env.storage()
            .instance()
            .set(&DataKey::UserDepositCap, &DEFAULT_USER_DEPOSIT_CAP);
        env.storage()
            .instance()
            .set(&DataKey::MinDeposit, &DEFAULT_MIN_DEPOSIT);
        env.storage()
            .instance()
            .set(&DataKey::MaxDeposit, &DEFAULT_MAX_DEPOSIT);
        env.storage()
            .instance()
            .set(&DataKey::BlendApprovalTtl, &DEFAULT_BLEND_APPROVAL_TTL);
        env.storage().instance().set(
            &DataKey::MaxConsecutiveFailures,
            &DEFAULT_MAX_CONSECUTIVE_FAILURES,
        );
        Self::initialize_rate_limit_defaults(&env);
        env.storage().instance().set(&DataKey::Version, &1_u32);

        env.events().publish(
            (TOPIC_INIT,),
            VaultInitializedEvent {
                owner: owner.clone(),
                agent: agent.clone(),
                usdc_token: usdc_token.clone(),
                tvl_cap,
            },
        );
    }

    // ==========================================================================
    // CORE LIFECYCLE - DEPOSIT
    // ==========================================================================

    /// Deposits USDC into the vault on behalf of a user.
    ///
    /// The user must authorize this transaction with their signature.
    /// The vault transfers USDC from the user and records their balance.
    /// An event is emitted for the AI agent to detect and initiate yield deployment.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address making the deposit (must authorize).
    /// * `amount` - Amount of USDC to deposit (7 decimal places).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `DepositEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the vault is paused.
    /// - If amount is not positive.
    /// - If amount is less than the minimum deposit.
    /// - If amount would exceed the user's deposit cap.
    /// - If amount would exceed the TVL cap.
    /// - If the USDC transfer fails.
    /// - If shares to mint rounds down to zero.
    /// - If the user's deposit rate-limit bucket is exhausted.
    pub fn deposit(env: Env, user: Address, amount: i128) {
        Self::require_initialized(&env);
        user.require_auth();

        Self::require_not_paused(&env);
        Self::require_positive_amount(&env, amount);
        Self::require_minimum_deposit(&env, amount);
        Self::require_maximum_deposit(&env, amount);
        Self::require_within_deposit_cap(&env, &user, amount);
        Self::require_within_tvl_cap(&env, amount);
        // Count only a fully validated operation, before any token transfer.
        Self::enforce_user_rate_limit(&env, &user, RATE_LIMIT_DEPOSIT);

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&user, &env.current_contract_address(), &amount);

        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0_i128);
        let new_total = total
            .checked_add(amount)
            .expect("vault: total deposits overflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalDeposits, &new_total);

        // Mint shares based on current share price and update total assets.
        // Inflation-attack mitigation: reject any deposit that would round down
        // to zero shares. Together with storage-based asset accounting (donations
        // can't move the price) and the minimum-deposit floor, this defeats the
        // first-depositor/donation inflation attack. See `deposit` docs.
        let shares_to_mint = Self::convert_to_shares_internal(&env, amount);
        Self::require(
            &env,
            shares_to_mint > 0,
            VaultError::SharesToMintMustBePositive,
        );

        // Update user shares
        let current_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);
        let new_user_shares = current_shares
            .checked_add(shares_to_mint)
            .expect("vault: user shares overflow");
        env.storage()
            .persistent()
            .set(&DataKey::Shares(user.clone()), &new_user_shares);

        // Register the user in the active-share index the first time they hold
        // non-zero shares, so the `get_users_with_shares` indexer view can page
        // over holders (Issue #440). The `current_shares == 0` gate also covers a
        // user who fully withdrew earlier and is re-entering; `add_to_user_index`
        // dedupes so their slot is not duplicated.
        if current_shares == 0 {
            Self::add_to_user_index(&env, &user);
        }

        // Set default strategy for first-time depositors
        if current_shares == 0
            && !env
                .storage()
                .persistent()
                .has(&DataKey::UserStrategy(user.clone()))
        {
            let default_strategy = Symbol::new(&env, "balanced");
            env.storage()
                .persistent()
                .set(&DataKey::UserStrategy(user.clone()), &default_strategy);

            env.events().publish(
                (TOPIC_USER_STRATEGY_UPDATED, user.clone()),
                UserStrategyUpdatedEvent {
                    user: user.clone(),
                    old_strategy: Symbol::new(&env, ""),
                    new_strategy: default_strategy,
                },
            );
        }

        // Update total shares
        let total_shares: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalShares)
            .unwrap_or(0_i128);
        let new_total_shares = total_shares
            .checked_add(shares_to_mint)
            .expect("vault: total shares overflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &new_total_shares);

        // Update total assets (principal + yield)
        let total_assets = Self::get_total_assets_internal(&env);
        let new_total_assets = total_assets
            .checked_add(amount)
            .expect("vault: total assets overflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &new_total_assets);

        // Record deposit ledger for flash-loan protection (#659).
        env.storage().persistent().set(
            &DataKey::LastDepositLedger(user.clone()),
            &env.ledger().sequence(),
        );

        env.events().publish(
            (TOPIC_DEPOSIT, user.clone()),
            DepositEvent {
                user,
                amount,
                // Shares minted for this deposit
                shares: shares_to_mint,
            },
        );
    }

    /// Deposits multiple amounts in a single transaction. Each entry specifies a
    /// token address and amount. Currently only the vault's USDC token is
    /// accepted; other tokens will be supported when multi-asset functionality
    /// is enabled (Phase 3).
    ///
    /// The entire batch is processed atomically — if any transfer fails, the
    /// whole transaction reverts. Shares are minted once based on the aggregate
    /// deposit amount, reducing transaction costs for multi-token deposits.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address depositing funds (must authorize).
    /// * `entries` - A vector of `(token_address, amount)` pairs.
    ///
    /// # Events
    ///
    /// Emits one `DepositEvent` per entry.
    ///
    /// # Panics
    ///
    /// - If any entry's token is not the vault's USDC token (until multi-asset).
    /// - If any entry's amount fails validation.
    /// - If the aggregate deposit exceeds the TVL or user cap.
    /// - If the batch exceeds the configured entry limit.
    /// - If the user's deposit or batch rate-limit bucket is exhausted.
    /// - If shares to mint rounds down to zero.
    pub fn batch_deposit(env: Env, user: Address, entries: Vec<(Address, i128)>) {
        Self::require_initialized(&env);
        user.require_auth();
        Self::require_not_paused(&env);

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let total_entries = entries.len();
        Self::require_batch_size(&env, total_entries);
        // A batch is one deposit operation for the per-user deposit bucket and
        // one operation for the separate batch bucket. This closes the bypass
        // where a caller could avoid the single-deposit limit by batching.
        Self::enforce_user_rate_limit(&env, &user, RATE_LIMIT_DEPOSIT);
        Self::enforce_user_rate_limit(&env, &user, RATE_LIMIT_BATCH_DEPOSIT);

        // First pass: validate every entry before any transfer (fail-fast).
        let mut total_amount: i128 = 0;
        for i in 0..total_entries {
            let (token, amount) = entries.get(i).unwrap();
            // Until multi-asset is enabled, require all entries to use USDC.
            if token != usdc_token {
                panic!(
                    "batch_deposit: token {:?} is not supported; only USDC is accepted",
                    token
                );
            }
            Self::require_positive_amount(&env, amount);
            total_amount = total_amount
                .checked_add(amount)
                .expect("batch_deposit: total amount overflow");
        }

        // Validate aggregate against vault limits.
        if total_entries > 0 {
            Self::require_minimum_deposit(&env, total_amount);
            Self::require_maximum_deposit(&env, total_amount);
            Self::require_within_deposit_cap(&env, &user, total_amount);
            Self::require_within_tvl_cap(&env, total_amount);
        }

        // Second pass: execute transfers.
        let token_client = token::Client::new(&env, &usdc_token);
        for i in 0..total_entries {
            let (_token, amount) = entries.get(i).unwrap();
            token_client.transfer(&user, &env.current_contract_address(), &amount);
        }

        // Update total deposits and mint shares once for the aggregate.
        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0_i128);
        env.storage().instance().set(
            &DataKey::TotalDeposits,
            &(total
                .checked_add(total_amount)
                .expect("batch_deposit: total deposits overflow")),
        );

        let shares_to_mint = Self::convert_to_shares_internal(&env, total_amount);
        Self::require(
            &env,
            shares_to_mint > 0,
            VaultError::SharesToMintMustBePositive,
        );

        let current_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);
        env.storage().persistent().set(
            &DataKey::Shares(user.clone()),
            &(current_shares
                .checked_add(shares_to_mint)
                .expect("batch_deposit: shares overflow")),
        );

        if current_shares == 0
            && !env
                .storage()
                .persistent()
                .has(&DataKey::UserStrategy(user.clone()))
        {
            let default_strategy = Symbol::new(&env, "balanced");
            env.storage()
                .persistent()
                .set(&DataKey::UserStrategy(user.clone()), &default_strategy);
            env.events().publish(
                (TOPIC_USER_STRATEGY_UPDATED, user.clone()),
                UserStrategyUpdatedEvent {
                    user: user.clone(),
                    old_strategy: Symbol::new(&env, ""),
                    new_strategy: default_strategy,
                },
            );
        }

        let total_shares: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalShares)
            .unwrap_or(0_i128);
        env.storage().instance().set(
            &DataKey::TotalShares,
            &(total_shares
                .checked_add(shares_to_mint)
                .expect("batch_deposit: total shares overflow")),
        );

        for i in 0..total_entries {
            let (_token, amount) = entries.get(i).unwrap();
            env.events().publish(
                (TOPIC_DEPOSIT, user.clone()),
                DepositEvent {
                    user: user.clone(),
                    amount,
                    shares: shares_to_mint,
                },
            );
        }
    }

    // ==========================================================================
    // CORE LIFECYCLE - WITHDRAW
    // ==========================================================================

    /// Withdraws USDC from the vault for a user.
    ///
    /// The user must authorize this transaction with their signature.
    /// The vault transfers USDC from its balance to the user.
    ///
    /// If funds are deployed in Blend, this function will pull liquidity back
    /// first to ensure funds are available for withdrawal.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address withdrawing funds (must authorize).
    /// * `amount` - Amount of USDC to withdraw (7 decimal places).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `WithdrawEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the vault is paused.
    /// - If amount is not positive.
    /// - If user has insufficient balance or shares.
    /// - If the vault has insufficient liquidity and cannot retrieve enough from Blend.
    /// - If the USDC transfer fails.
    /// - If the user's withdrawal rate-limit bucket is exhausted.
    pub fn withdraw(env: Env, user: Address, amount: i128) {
        Self::require_initialized(&env);
        user.require_auth();

        Self::require_not_paused(&env);
        Self::require_positive_amount(&env, amount);
        Self::enforce_user_rate_limit(&env, &user, RATE_LIMIT_WITHDRAW);

        // Flash-loan protection: enforce minimum holding period (#659).
        // If the owner has configured a non-zero MinHoldingPeriod, reject any
        // withdrawal attempted before `last_deposit_ledger + min_holding_period`.
        let min_holding: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MinHoldingPeriod)
            .unwrap_or(0_u32);
        if min_holding > 0 {
            if let Some(last_deposit_ledger) = env
                .storage()
                .persistent()
                .get::<DataKey, u32>(&DataKey::LastDepositLedger(user.clone()))
            {
                let current_ledger = env.ledger().sequence();
                let elapsed = current_ledger.saturating_sub(last_deposit_ledger);
                if elapsed < min_holding {
                    env.events().publish(
                        (symbol_short!("fl_block"), user.clone()),
                        FlashLoanProtectionTriggeredEvent {
                            user: user.clone(),
                            last_deposit_ledger,
                            current_ledger,
                            min_holding_period: min_holding,
                        },
                    );
                    panic_with_error!(&env, VaultError::HoldingPeriodNotElapsed);
                }
            }
        }

        // Check if user has locked shares (#636)
        let locked_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LockedShares(user.clone()))
            .unwrap_or(0_i128);
        if locked_shares > 0 {
            let total_user_shares: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::Shares(user.clone()))
                .unwrap_or(0_i128);
            let unlocked_shares = total_user_shares - locked_shares;

            // Calculate what the user can withdraw based on unlocked shares
            let total_shares = Self::get_total_shares_internal(&env);
            let total_assets = Self::get_total_assets_internal(&env);
            let max_withdrawable = if total_shares > 0 && total_assets > 0 {
                Self::convert_to_assets_internal(&env, unlocked_shares)
            } else {
                0
            };

            Self::require(
                &env,
                amount <= max_withdrawable,
                VaultError::InsufficientShares,
            );
        }

        // Check if funds are deployed in Blend and need to be retrieved
        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);

        // We use actual_to_return to track how much we can really give back.
        // Initially, we assume we can fulfill the whole request.
        let mut actual_to_return = amount;

        if current_protocol == symbol_short!("blend") || current_protocol == symbol_short!("dex") {
            // Check vault's USDC balance
            let vault_balance = token_client.balance(&env.current_contract_address());

            // If vault doesn't have enough USDC, try to withdraw from the active protocol
            if vault_balance < amount {
                // Calculate how much we need to withdraw
                let needed = amount
                    .checked_sub(vault_balance)
                    .expect("vault: withdrawal underflow");

                // Attempt to withdraw from the active protocol (Blend or DEX).
                // If this returns less than needed, we will reconcile below
                let _withdrawn =
                    Self::withdraw_amount_from_protocol(&env, &current_protocol, needed, 0);

                // RECONCILIATION: Check actual available USDC after the withdrawal.
                // We cap the withdrawal to what the vault actually has available.
                let available_usdc = token_client.balance(&env.current_contract_address());
                actual_to_return = min(amount, available_usdc);
            }
        }

        Self::require(
            &env,
            actual_to_return > 0,
            VaultError::InsufficientLiquidity,
        );

        // Share-based withdrawal:
        // - Convert reconciled asset amount to shares
        // - Burn shares from user
        // - Return proportional assets based on current share price

        let user_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);
        Self::require(&env, user_shares > 0, VaultError::InsufficientShares);

        let total_shares = Self::get_total_shares_internal(&env);
        let total_assets = Self::get_total_assets_internal(&env);
        Self::require(
            &env,
            total_shares > 0 && total_assets > 0,
            VaultError::NoAssetsToWithdraw,
        );

        // We use actual_to_return to determine how many shares to burn.
        // If Blend returned less than needed, the user will receive a partial
        // withdrawal and keep their remaining shares.
        // Use ceiling division to prevent dust attacks (ensure at least 1 share burned when assets > 0).
        let shares_to_burn = Self::convert_to_shares_internal_ceil(&env, actual_to_return);
        Self::require(
            &env,
            shares_to_burn > 0,
            VaultError::SharesToBurnMustBePositive,
        );
        Self::require(
            &env,
            user_shares >= shares_to_burn,
            VaultError::InsufficientSharesForAmount,
        );

        // Calculate actual assets to return based on burned shares.
        // Due to integer division, this may be slightly less than `actual_to_return`,
        // but never more (prevents over-withdrawal due to rounding).
        let usdc_to_return = Self::convert_to_assets_internal(&env, shares_to_burn);

        // Update user shares and total shares
        let new_user_shares = user_shares
            .checked_sub(shares_to_burn)
            .expect("vault: withdrawal underflow");
        env.storage()
            .persistent()
            .set(&DataKey::Shares(user.clone()), &new_user_shares);

        let new_total_shares = total_shares
            .checked_sub(shares_to_burn)
            .expect("vault: withdrawal underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &new_total_shares);

        // Update total assets (principal + yield)
        let new_total_assets = total_assets
            .checked_sub(usdc_to_return)
            .expect("vault: withdrawal underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &new_total_assets);

        Self::reduce_total_deposits_on_withdraw(&env, usdc_to_return);

        token_client.transfer(&env.current_contract_address(), &user, &usdc_to_return);

        env.events().publish(
            (TOPIC_WITHDRAW, user.clone()),
            WithdrawEvent {
                user,
                amount: usdc_to_return,
                shares: shares_to_burn,
            },
        );
    }

    // ==========================================================================
    // CORE LIFECYCLE - WITHDRAW ALL
    // ==========================================================================

    /// Withdraws all USDC from the vault for a user by burning all their shares.
    ///
    /// This function allows users to withdraw their entire balance without worrying
    /// about rounding issues in share-to-asset conversions. It burns all user shares
    /// and returns the proportional amount of assets.
    ///
    /// If funds are deployed in Blend, this function will pull liquidity back
    /// first to ensure funds are available for withdrawal.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address withdrawing funds (must authorize).
    ///
    /// # Returns
    ///
    /// Returns the amount of USDC withdrawn.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `WithdrawEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the vault is paused.
    /// - If user has no shares to withdraw.
    /// - If the vault has no assets.
    /// - If the USDC transfer fails.
    /// - If the user's withdrawal rate-limit bucket is exhausted.
    pub fn withdraw_all(env: Env, user: Address) -> i128 {
        Self::require_initialized(&env);
        user.require_auth();

        Self::require_not_paused(&env);
        Self::enforce_user_rate_limit(&env, &user, RATE_LIMIT_WITHDRAW);

        // Check if user has locked shares (#636)
        let locked_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LockedShares(user.clone()))
            .unwrap_or(0_i128);

        let user_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);

        // If user has locked shares, only withdraw unlocked shares
        let shares_to_withdraw = if locked_shares > 0 {
            let unlocked_shares = user_shares - locked_shares;
            if unlocked_shares == 0 {
                panic_with_error!(&env, VaultError::InsufficientShares);
            }
            unlocked_shares
        } else {
            user_shares
        };

        // Check if funds are deployed in Blend and need to be retrieved
        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);

        Self::require(&env, shares_to_withdraw > 0, VaultError::NoSharesToWithdraw);

        let total_shares = Self::get_total_shares_internal(&env);
        let total_assets = Self::get_total_assets_internal(&env);
        Self::require(
            &env,
            total_shares > 0 && total_assets > 0,
            VaultError::NoAssetsToWithdraw,
        );

        // Calculate assets user is entitled to based on their shares
        let entitled_amount = Self::convert_to_assets_internal(&env, shares_to_withdraw);
        let mut usdc_to_return = entitled_amount;
        let mut shares_to_burn = shares_to_withdraw;

        if current_protocol == symbol_short!("blend") || current_protocol == symbol_short!("dex") {
            // Check vault's USDC balance
            let vault_balance = token_client.balance(&env.current_contract_address());

            // If vault doesn't have enough USDC, try to withdraw from the active protocol
            if vault_balance < entitled_amount {
                // Attempt to withdraw from the active protocol (Blend or DEX)
                let needed = entitled_amount
                    .checked_sub(vault_balance)
                    .expect("vault: withdrawal underflow");
                let _ = Self::withdraw_amount_from_protocol(&env, &current_protocol, needed, 0);

                // RECONCILIATION: Check actual available USDC after the potential withdrawal
                let available_usdc = token_client.balance(&env.current_contract_address());

                // If vault has less than entitled, we cap the withdrawal.
                // The user receives what's available and keeps their remaining shares.
                if available_usdc < entitled_amount {
                    usdc_to_return = available_usdc;
                    Self::require(&env, usdc_to_return > 0, VaultError::NoLiquidityAvailable);
                    // Use ceiling division to prevent dust attacks (ensure at least 1 share burned).
                    shares_to_burn = Self::convert_to_shares_internal_ceil(&env, usdc_to_return);
                }
            }
        }

        Self::require(&env, usdc_to_return > 0, VaultError::NoAssetsToReturn);
        Self::require(&env, shares_to_burn > 0, VaultError::NoSharesToBurn);

        // Update user shares
        let new_user_shares = user_shares
            .checked_sub(shares_to_burn)
            .expect("vault: withdrawal underflow");
        env.storage()
            .persistent()
            .set(&DataKey::Shares(user.clone()), &new_user_shares);

        // Update total shares
        let new_total_shares = total_shares
            .checked_sub(shares_to_burn)
            .expect("vault: withdrawal underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &new_total_shares);

        // Update total assets
        let new_total_assets = total_assets
            .checked_sub(usdc_to_return)
            .expect("vault: withdrawal underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &new_total_assets);

        Self::reduce_total_deposits_on_withdraw(&env, usdc_to_return);

        // Transfer USDC to user
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&env.current_contract_address(), &user, &usdc_to_return);

        env.events().publish(
            (TOPIC_WITHDRAW, user.clone()),
            WithdrawEvent {
                user,
                amount: usdc_to_return,
                shares: shares_to_burn,
            },
        );

        usdc_to_return
    }

    // ==========================================================================
    // CORE LIFECYCLE - MIGRATION (#637)
    // ==========================================================================

    /// Migrates user shares from this vault to a new vault contract.
    ///
    /// This function enables trustless migration during contract upgrades:
    /// - Burns user's shares in the old vault (this contract)
    /// - Calculates the asset value using current exchange rate
    /// - Calls deposit on the new vault on behalf of the user
    /// - Preserves share value through exchange rate conversion
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address migrating shares (must authorize).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `SharesMigratedEvent`
    ///
    /// # Errors
    ///
    /// - [`VaultError::MigrationPaused`] if migration is paused by owner.
    /// - [`VaultError::InvalidMigrationTarget`] if no migration target is set.
    /// - [`VaultError::NoSharesToMigrate`] if user has no shares.
    ///
    /// # Panics
    ///
    /// - If the vault is not initialized.
    /// - If the caller is not the user (authentication check).
    pub fn migrate_shares(env: Env, user: Address) {
        Self::require_initialized(&env);
        user.require_auth();

        // Check migration pause state
        let migration_paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::MigrationPaused)
            .unwrap_or(false);
        Self::require(&env, !migration_paused, VaultError::MigrationPaused);

        // Check migration target is set
        let migration_target: Address = env
            .storage()
            .instance()
            .get(&DataKey::MigrationTarget)
            .unwrap_or_else(|| panic_with_error!(&env, VaultError::InvalidMigrationTarget));

        // Get user's current shares
        let user_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);
        Self::require(&env, user_shares > 0, VaultError::NoSharesToMigrate);

        // Calculate asset value using current exchange rate
        let total_shares = Self::get_total_shares_internal(&env);
        let total_assets = Self::get_total_assets_internal(&env);
        let assets_to_transfer = Self::convert_to_assets_internal(&env, user_shares);

        // Burn shares from user
        let new_user_shares = 0_i128;
        env.storage()
            .persistent()
            .set(&DataKey::Shares(user.clone()), &new_user_shares);

        let new_total_shares = total_shares
            .checked_sub(user_shares)
            .expect("vault: migration underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &new_total_shares);

        // Update total assets
        let new_total_assets = total_assets
            .checked_sub(assets_to_transfer)
            .expect("vault: migration underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &new_total_assets);

        // Transfer USDC to new vault and call deposit on behalf of user
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(
            &env.current_contract_address(),
            &migration_target,
            &assets_to_transfer,
        );

        // Call deposit on new vault on behalf of user using cross-contract call
        // The new vault must implement a deposit function with signature (Env, Address, i128)
        let deposit_args: Vec<Val> = vec![
            &env,
            user.clone().into_val(&env),
            assets_to_transfer.into_val(&env),
        ];
        env.invoke_contract::<()>(
            &migration_target,
            &Symbol::new(&env, "deposit"),
            deposit_args,
        );

        // Emit migration event
        env.events().publish(
            (TOPIC_MIGRATE, user.clone()),
            SharesMigratedEvent {
                user: user.clone(),
                old_vault: env.current_contract_address(),
                new_vault: migration_target,
                shares_burned: user_shares,
                assets_transferred: assets_to_transfer,
            },
        );
    }

    // ==========================================================================
    // CORE LIFECYCLE - SHARE LOCKING (#636)
    // ==========================================================================

    /// Locks user shares for a configurable period to earn boosted APY.
    ///
    /// Users can voluntarily lock their shares for higher yields:
    /// - 30 days = 1.1x boost
    /// - 90 days = 1.25x boost
    /// - 180 days = 1.5x boost
    ///
    /// Locked shares cannot be withdrawn until the lock period expires.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address locking shares (must authorize).
    /// * `shares` - Number of shares to lock.
    /// * `lock_duration_days` - Lock period in days (30, 90, or 180).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `SharesLockedEvent`
    ///
    /// # Errors
    ///
    /// - [`VaultError::SharesAlreadyLocked`] if user already has locked shares.
    /// - [`VaultError::InvalidLockDuration`] if duration is not 30, 90, or 180 days.
    /// - [`VaultError::InsufficientUnlockedShares`] if user doesn't have enough unlocked shares.
    ///
    /// # Panics
    ///
    /// - If the vault is not initialized.
    /// - If the caller is not the user (authentication check).
    pub fn lock_shares(env: Env, user: Address, shares: i128, lock_duration_days: u32) {
        Self::require_initialized(&env);
        user.require_auth();

        // Check if user already has locked shares
        let existing_locked: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LockedShares(user.clone()))
            .unwrap_or(0_i128);
        Self::require(&env, existing_locked == 0, VaultError::SharesAlreadyLocked);

        // Validate lock duration and get boost multiplier
        let (lock_ledgers, boost_multiplier) = match lock_duration_days {
            30 => (LOCK_30_DAYS_LEDGERS, BOOST_30_DAYS),
            90 => (LOCK_90_DAYS_LEDGERS, BOOST_90_DAYS),
            180 => (LOCK_180_DAYS_LEDGERS, BOOST_180_DAYS),
            _ => panic_with_error!(&env, VaultError::InvalidLockDuration),
        };

        // Check user has enough unlocked shares
        let total_user_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);
        let unlocked_shares = total_user_shares - existing_locked;
        Self::require(
            &env,
            shares <= unlocked_shares,
            VaultError::InsufficientUnlockedShares,
        );

        // Calculate lock expiry ledger
        let current_ledger = env.ledger().sequence();
        let expiry_ledger = current_ledger + lock_ledgers;

        // Store locked shares and expiry
        env.storage()
            .persistent()
            .set(&DataKey::LockedShares(user.clone()), &shares);
        env.storage()
            .persistent()
            .set(&DataKey::LockExpiry(user.clone()), &expiry_ledger);

        // Emit lock event
        env.events().publish(
            (TOPIC_SHARES_LOCKED, user.clone()),
            SharesLockedEvent {
                user: user.clone(),
                shares_locked: shares,
                lock_duration_days,
                boost_multiplier,
                expiry_ledger,
            },
        );
    }

    /// Unlocks user shares after the lock period has expired.
    ///
    /// Users can only unlock shares after the lock period has ended.
    /// This releases the shares for normal withdrawal.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address unlocking shares (must authorize).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `SharesUnlockedEvent`
    ///
    /// # Errors
    ///
    /// - [`VaultError::LockPeriodNotEnded`] if the lock period has not expired.
    ///
    /// # Panics
    ///
    /// - If the vault is not initialized.
    /// - If the caller is not the user (authentication check).
    /// - If the user has no locked shares.
    pub fn unlock_shares(env: Env, user: Address) {
        Self::require_initialized(&env);
        user.require_auth();

        // Get locked shares and expiry
        let locked_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LockedShares(user.clone()))
            .unwrap_or(0_i128);
        Self::require(&env, locked_shares > 0, VaultError::NoSharesToMigrate);

        let expiry_ledger: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LockExpiry(user.clone()))
            .unwrap();

        // Check if lock period has ended
        let current_ledger = env.ledger().sequence();
        Self::require(
            &env,
            current_ledger >= expiry_ledger,
            VaultError::LockPeriodNotEnded,
        );

        // Clear locked shares and expiry
        env.storage()
            .persistent()
            .set(&DataKey::LockedShares(user.clone()), &0_i128);
        env.storage()
            .persistent()
            .remove(&DataKey::LockExpiry(user.clone()));

        // Emit unlock event
        env.events().publish(
            (TOPIC_SHARES_UNLOCKED, user.clone()),
            SharesUnlockedEvent {
                user: user.clone(),
                shares_unlocked: locked_shares,
            },
        );
    }

    /// Gets user's locked shares and lock expiry information.
    ///
    /// Returns the number of locked shares and the ledger when they can be unlocked.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address to query.
    ///
    /// # Returns
    ///
    /// A tuple of (locked_shares, unlock_ledger) where:
    /// - `locked_shares` is the number of shares currently locked (0 if none)
    /// - `unlock_ledger` is the ledger number when shares can be unlocked (0 if none)
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the vault is not initialized.
    pub fn get_locked_shares(env: Env, user: Address) -> (i128, u32) {
        Self::require_initialized(&env);

        let locked_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::LockedShares(user.clone()))
            .unwrap_or(0_i128);
        let unlock_ledger: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::LockExpiry(user.clone()))
            .unwrap_or(0_u32);

        (locked_shares, unlock_ledger)
    }

    // ==========================================================================
    // CORE LIFECYCLE - EMERGENCY WITHDRAWAL (#635)
    // ==========================================================================

    /// Emergency withdrawal function that works when the vault is paused.
    ///
    /// When the vault is paused, users cannot withdraw through normal means.
    /// This function provides a safety mechanism for users to recover their funds
    /// if the owner pauses the vault for an extended period or during governance disputes.
    ///
    /// This function:
    /// - Works even when the vault is paused
    /// - Requires user authentication (only their own funds)
    /// - Deducts from idle balance first, then from protocol if needed
    /// - Does not affect rebalance or other admin operations
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address withdrawing funds (must authorize).
    /// * `amount` - Amount of USDC to withdraw (7 decimal places).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `EmergencyWithdrawalEvent`
    ///
    /// # Errors
    ///
    /// - [`VaultError::EmergencyWithdrawalNotAllowed`] if the vault is not paused.
    ///
    /// # Panics
    ///
    /// - If the vault is not initialized.
    /// - If the caller is not the user (authentication check).
    /// - If user has insufficient shares.
    /// - If there's insufficient liquidity.
    pub fn emergency_withdraw(env: Env, user: Address, amount: i128) {
        Self::require_initialized(&env);
        user.require_auth();

        // Only allow emergency withdrawal when vault is paused
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        Self::require(&env, paused, VaultError::EmergencyWithdrawalNotAllowed);

        Self::require_positive_amount(&env, amount);

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);

        // Check vault's idle USDC balance
        let vault_balance = token_client.balance(&env.current_contract_address());
        let mut from_idle = false;
        let mut actual_to_return = amount;

        // If vault doesn't have enough idle USDC, try to withdraw from protocol
        if vault_balance < amount {
            let current_protocol: Symbol = env
                .storage()
                .instance()
                .get(&DataKey::CurrentProtocol)
                .unwrap_or(symbol_short!("none"));

            if current_protocol == symbol_short!("blend")
                || current_protocol == symbol_short!("dex")
            {
                // Calculate how much we need to withdraw
                let needed = amount
                    .checked_sub(vault_balance)
                    .expect("vault: withdrawal underflow");

                // Attempt to withdraw from the active protocol
                let _withdrawn =
                    Self::withdraw_amount_from_protocol(&env, &current_protocol, needed, 0);

                // Check actual available USDC after the withdrawal
                let available_usdc = token_client.balance(&env.current_contract_address());
                actual_to_return = min(amount, available_usdc);
            } else {
                actual_to_return = vault_balance;
            }
        } else {
            from_idle = true;
        }

        Self::require(
            &env,
            actual_to_return > 0,
            VaultError::InsufficientLiquidity,
        );

        // Share-based withdrawal
        let user_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);
        Self::require(&env, user_shares > 0, VaultError::InsufficientShares);

        let total_shares = Self::get_total_shares_internal(&env);
        let total_assets = Self::get_total_assets_internal(&env);
        Self::require(
            &env,
            total_shares > 0 && total_assets > 0,
            VaultError::NoAssetsToWithdraw,
        );

        // Use ceiling division to prevent dust attacks
        let shares_to_burn = Self::convert_to_shares_internal_ceil(&env, actual_to_return);
        Self::require(
            &env,
            shares_to_burn > 0,
            VaultError::SharesToBurnMustBePositive,
        );
        Self::require(
            &env,
            user_shares >= shares_to_burn,
            VaultError::InsufficientSharesForAmount,
        );

        // Calculate actual assets to return based on burned shares
        let usdc_to_return = Self::convert_to_assets_internal(&env, shares_to_burn);

        // Update user shares and total shares
        let new_user_shares = user_shares
            .checked_sub(shares_to_burn)
            .expect("vault: withdrawal underflow");
        env.storage()
            .persistent()
            .set(&DataKey::Shares(user.clone()), &new_user_shares);

        let new_total_shares = total_shares
            .checked_sub(shares_to_burn)
            .expect("vault: withdrawal underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &new_total_shares);

        // Update total assets
        let new_total_assets = total_assets
            .checked_sub(usdc_to_return)
            .expect("vault: withdrawal underflow");
        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &new_total_assets);

        Self::reduce_total_deposits_on_withdraw(&env, usdc_to_return);

        // Transfer USDC to user
        token_client.transfer(&env.current_contract_address(), &user, &usdc_to_return);

        // Emit emergency withdrawal event
        env.events().publish(
            (TOPIC_EMERGENCY_WITHDRAWAL, user.clone()),
            EmergencyWithdrawalEvent {
                user: user.clone(),
                amount: usdc_to_return,
                shares: shares_to_burn,
                from_idle,
            },
        );
    }

    // ==========================================================================
    // CORE LIFECYCLE - REBALANCE
    // ==========================================================================

    // NOTE: There is no `harvest()` entrypoint in this contract (Issue #496).
    // Yield is reported via `update_total_assets()` called by the agent, which
    // does not involve a separate harvest step. Consequently there is no
    // `harvest()` failure path to wire into a circuit breaker. If a future
    // version introduces a `harvest()` function, it should report outcomes to
    // the circuit-breaker helper alongside `rebalance()`.

    /// Rebalances vault funds between yield strategies.
    ///
    /// Only the authorized AI agent can call this function. The agent uses
    /// this to move funds between different yield-generating protocols based
    /// on market conditions and strategy performance.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `protocol` - The target protocol to move funds to ("blend", "dex", or "none").
    /// * `expected_apy` - Expected APY in basis points (e.g., 850 = 8.5%).
    /// * `min_out` - Minimum assets expected to remain (slippage protection).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `RebalanceEvent`
    /// - `ProtocolChangedEvent`
    /// - `RebalanceFailedEvent` (if exit fails)
    /// - `BlendWithdrawEvent` / `BlendSupplyEvent` (Blend legs)
    /// - `DexWithdrawEvent` / `DexSupplyEvent` (DEX legs)
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If caller is not the authorized agent.
    /// - If vault is paused.
    /// - If the rebalance cooldown has not elapsed (`RebalanceCooldownActive`).
    /// - If protocol is unsupported.
    /// - If slippage protection (min_out) is triggered.
    /// - If protocol interaction fails.
    /// - If Blend pool is not configured and protocol is "blend"
    /// - If the DEX pool is not configured and protocol is "dex"
    /// - If a leg moves fewer assets than `min_out` when `min_out > 0`
    /// - If the global rebalance rate-limit bucket is exhausted.
    pub fn rebalance(env: Env, protocol: Symbol, expected_apy: i128, min_out: i128) {
        Self::require_initialized(&env);
        Self::require_not_paused(&env);
        Self::require_is_agent(&env);
        // Reuse `InvalidStrategy` for invalid rebalance configuration inputs.
        // The contract error enum is already at Soroban's 50-variant limit.
        Self::require(
            &env,
            (0..=10_000).contains(&expected_apy),
            VaultError::InvalidStrategy,
        );

        // ── Rebalance cooldown guard (Issue #59) ──────────────────────────────
        // If a minimum interval has been configured by the owner, enforce it.
        // Only applies after the first rebalance — if LastRebalanceLedger has
        // never been written, there is no prior call to measure elapsed time from.
        if let Some(min_interval) = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::MinRebalanceInterval)
        {
            if min_interval > 0 {
                if let Some(last_rebalance) = env
                    .storage()
                    .instance()
                    .get::<DataKey, u32>(&DataKey::LastRebalanceLedger)
                {
                    let current_ledger = env.ledger().sequence();
                    let elapsed = current_ledger.saturating_sub(last_rebalance);
                    if elapsed < min_interval {
                        panic_with_error!(&env, VaultError::RebalanceCooldownActive);
                    }
                }
            }
        }
        // ─────────────────────────────────────────────────────────────────────

        if min_out < 0 {
            panic_with_error!(&env, VaultError::MinOutMustBeNonNegative);
        }

        // Validate protocol against allowlist
        let supported_protocols = vec![
            &env,
            symbol_short!("blend"),
            symbol_short!("dex"),
            symbol_short!("none"),
        ];
        if !supported_protocols.contains(protocol.clone()) {
            panic_with_error!(&env, VaultError::UnsupportedProtocol);
        }

        // Enforce the global frequency cap in addition to the legacy minimum
        // interval cooldown. The bucket is consumed before external protocol
        // calls, so a gracefully handled failed exit still counts as an attempt.
        Self::enforce_global_rate_limit(&env, RATE_LIMIT_REBALANCE);

        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        let mut amount_attempted = 0_i128;
        let mut amount_moved = 0_i128;
        let mut amount_supplied = 0_i128;
        let mut amount_withdrawn = 0_i128;

        // If switching protocols, exit the current one first.
        // On incomplete exit, emit RebalanceFailedEvent and abort — no further
        // state mutations occur so the vault remains consistent (Issue #145).
        if current_protocol != protocol && current_protocol != symbol_short!("none") {
            let expected_withdrawal = Self::get_protocol_balance(&env, &current_protocol);
            amount_attempted = amount_attempted.saturating_add(expected_withdrawal);

            let withdrawn = Self::withdraw_from_protocol(&env, &current_protocol, min_out);
            amount_withdrawn = amount_withdrawn.saturating_add(withdrawn);
            amount_moved = amount_moved.saturating_add(withdrawn);

            if expected_withdrawal > 0 {
                let remaining_balance = Self::get_protocol_balance(&env, &current_protocol);
                if remaining_balance > 0 {
                    // Protocol exit incomplete — abort rebalance gracefully so
                    // the failure is observable without reverting the tx.
                    env.events().publish(
                        (TOPIC_REBALANCE_FAILED,),
                        RebalanceFailedEvent {
                            from_protocol: current_protocol,
                            reason: symbol_short!("exit_fail"),
                        },
                    );
                    return;
                }
            }
        }

        if protocol == symbol_short!("blend") {
            if !env.storage().instance().has(&DataKey::BlendPool) {
                panic_with_error!(&env, VaultError::BlendPoolNotConfigured);
            }

            let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
            let token_client = token::Client::new(&env, &usdc_token);
            let vault_balance = token_client.balance(&env.current_contract_address());

            let mut status = symbol_short!("success");

            if vault_balance > 0 {
                amount_attempted = amount_attempted.saturating_add(vault_balance);
                let supplied = Self::supply_to_blend(&env, vault_balance, min_out);
                amount_supplied = amount_supplied.saturating_add(supplied);
                amount_moved = amount_moved.saturating_add(supplied);

                if supplied == 0 {
                    status = symbol_short!("failed");
                } else if supplied < vault_balance {
                    status = symbol_short!("partial");
                }
            } else if amount_moved == 0 {
                // Noop: no funds to supply, but protocol target is blend.
                // Update CurrentProtocol so tracking matches intent (Issue #146).
                Self::set_current_protocol(&env, symbol_short!("blend"));
                status = symbol_short!("noop");
            }

            env.events().publish(
                (TOPIC_REBALANCE,),
                RebalanceEvent {
                    protocol,
                    expected_apy,
                    status: status.clone(),
                    amount_attempted,
                    amount_moved,
                    amount_supplied,
                    amount_withdrawn,
                },
            );

            // Circuit breaker: fold this outcome into the consecutive-failure
            // counter, auto-pausing the vault if the threshold is reached (#439).
            Self::record_rebalance_outcome(&env, &status);
        } else if protocol == symbol_short!("dex") {
            if !env.storage().instance().has(&DataKey::DexPool) {
                panic_with_error!(&env, VaultError::DexPoolNotConfigured);
            }

            let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
            let token_client = token::Client::new(&env, &usdc_token);
            let vault_balance = token_client.balance(&env.current_contract_address());

            let mut status = symbol_short!("success");

            if vault_balance > 0 {
                amount_attempted = amount_attempted.saturating_add(vault_balance);
                let supplied = Self::supply_to_dex(&env, vault_balance, min_out);
                amount_supplied = amount_supplied.saturating_add(supplied);
                amount_moved = amount_moved.saturating_add(supplied);

                if supplied == 0 {
                    status = symbol_short!("failed");
                } else if supplied < vault_balance {
                    status = symbol_short!("partial");
                }
            } else if amount_moved == 0 {
                // Noop: no funds to supply, but protocol target is dex.
                // Update CurrentProtocol so tracking matches intent (mirrors Blend, Issue #146).
                Self::set_current_protocol(&env, symbol_short!("dex"));
                status = symbol_short!("noop");
            }

            env.events().publish(
                (TOPIC_REBALANCE,),
                RebalanceEvent {
                    protocol,
                    expected_apy,
                    status: status.clone(),
                    amount_attempted,
                    amount_moved,
                    amount_supplied,
                    amount_withdrawn,
                },
            );

            // Circuit breaker: fold this outcome into the consecutive-failure
            // counter, auto-pausing the vault if the threshold is reached (#439).
            Self::record_rebalance_outcome(&env, &status);
        } else if protocol == symbol_short!("none") {
            let mut status = symbol_short!("success");

            if current_protocol != symbol_short!("none") {
                let expected_withdrawal = Self::get_protocol_balance(&env, &current_protocol);
                amount_attempted = amount_attempted.saturating_add(expected_withdrawal);

                let withdrawn = Self::withdraw_from_protocol(&env, &current_protocol, min_out);
                amount_withdrawn = amount_withdrawn.saturating_add(withdrawn);
                amount_moved = amount_moved.saturating_add(withdrawn);

                if expected_withdrawal > 0 {
                    let remaining_balance = Self::get_protocol_balance(&env, &current_protocol);
                    if remaining_balance > 0 {
                        // Protocol exit incomplete — abort gracefully (Issue #145).
                        env.events().publish(
                            (TOPIC_REBALANCE_FAILED,),
                            RebalanceFailedEvent {
                                from_protocol: current_protocol,
                                reason: symbol_short!("exit_fail"),
                            },
                        );
                        return;
                    }
                }
                Self::set_current_protocol(&env, symbol_short!("none"));
            } else if amount_moved == 0 {
                status = symbol_short!("noop");
            }

            env.events().publish(
                (TOPIC_REBALANCE,),
                RebalanceEvent {
                    protocol,
                    expected_apy,
                    status: status.clone(),
                    amount_attempted,
                    amount_moved,
                    amount_supplied,
                    amount_withdrawn,
                },
            );

            // Circuit breaker: fold this outcome into the consecutive-failure
            // counter, auto-pausing the vault if the threshold is reached (#439).
            Self::record_rebalance_outcome(&env, &status);
        }

        // Persist the ledger of this successful rebalance so the next call can
        // be checked against the cooldown interval (Issue #59).
        env.storage()
            .instance()
            .set(&DataKey::LastRebalanceLedger, &env.ledger().sequence());
    }

    // ==========================================================================
    // ADMINISTRATIVE - PAUSE CONTROL
    // ==========================================================================

    /// Pauses the vault, disabling deposits and withdrawals.
    ///
    /// Emergency function to halt all user-facing operations.
    /// When paused:
    /// - Deposits are rejected
    /// - Withdrawals are rejected
    /// - Rebalancing is rejected
    /// - Read functions remain operational
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `owner` - The owner address (must authorize this call).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `VaultPausedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    pub fn harvest(env: Env, min_out: i128) {
        Self::require_initialized(&env);
        Self::require_not_paused(&env);
        Self::require_is_agent(&env);

        if min_out < 0 {
            panic_with_error!(&env, VaultError::MinOutMustBeNonNegative);
        }

        // Cooldown check
        if let Some(min_interval) = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::MinRebalanceInterval)
        {
            if min_interval > 0 {
                if let Some(last_rebalance) = env
                    .storage()
                    .instance()
                    .get::<DataKey, u32>(&DataKey::LastRebalanceLedger)
                {
                    let current_ledger = env.ledger().sequence();
                    let elapsed = current_ledger.saturating_sub(last_rebalance);
                    if elapsed < min_interval {
                        panic_with_error!(&env, VaultError::RebalanceCooldownActive);
                    }
                }
            }
        }

        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        if current_protocol == symbol_short!("none") {
            panic_with_error!(&env, VaultError::UnsupportedProtocol);
        }

        // Harvest also performs an external protocol round-trip. Reuse the
        // global rebalance bucket so it cannot bypass the frequency guard by
        // alternating between `rebalance` and `harvest`.
        Self::enforce_global_rate_limit(&env, RATE_LIMIT_REBALANCE);

        let withdrawn = Self::withdraw_from_protocol(&env, &current_protocol, min_out);

        if withdrawn > 0 {
            if current_protocol == symbol_short!("blend") {
                Self::supply_to_blend(&env, withdrawn, min_out);
            } else if current_protocol == symbol_short!("dex") {
                Self::supply_to_dex(&env, withdrawn, min_out);
            }
        }

        env.events().publish(
            (TOPIC_HARVEST,),
            HarvestEvent {
                protocol: current_protocol,
                amount_harvested: withdrawn,
            },
        );

        env.storage()
            .instance()
            .set(&DataKey::LastRebalanceLedger, &env.ledger().sequence());
    }

    /// Pauses deposits, withdrawals, and agent operations until the owner calls
    /// `unpause` or a circuit-breaker reset path.
    ///
    /// The owner must authorize the call. Read-only getters, rate-limit
    /// configuration, and TTL maintenance remain available while paused.
    pub fn pause(env: Env, owner: Address) {
        Self::require_initialized(&env);
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::OnlyOwnerCanPause);

        env.storage().instance().set(&DataKey::Paused, &true);

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        env.events()
            .publish((TOPIC_PAUSED,), VaultPausedEvent { owner });
    }

    /// Unpauses the vault, re-enabling deposits and withdrawals.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `owner` - The owner address (must authorize this call).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `VaultUnpausedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If the vault is not currently paused.
    pub fn unpause(env: Env, owner: Address) {
        Self::require_initialized(&env);
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::OnlyOwnerCanUnpause);

        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        Self::require(&env, paused, VaultError::NotPaused);

        env.storage().instance().set(&DataKey::Paused, &false);

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        env.events()
            .publish((TOPIC_UNPAUSED,), VaultUnpausedEvent { owner });
    }

    /// Emergency pause function that immediately halts all operations.
    ///
    /// This is a separate function from pause() to distinguish emergency
    /// situations in event logs. Functionally identical to pause().
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `owner` - The owner address (must authorize this call).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `EmergencyPausedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    pub fn emergency_pause(env: Env, owner: Address) {
        Self::require_initialized(&env);
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(
            &env,
            owner == stored_owner,
            VaultError::OnlyOwnerCanEmergencyPause,
        );

        let already_paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if already_paused {
            return;
        }

        env.storage().instance().set(&DataKey::Paused, &true);

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        env.events().publish(
            (topics::TOPIC_EMERGENCY_PAUSED,),
            EmergencyPausedEvent { owner },
        );
    }

    /// Resets the circuit breaker and unpauses the vault.
    pub fn reset_circuit_breaker(env: Env, owner: Address) {
        Self::require_initialized(&env);
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::OnlyOwnerCanUnpause);
        env.storage()
            .instance()
            .set(&DataKey::ConsecutiveFailures, &0_u32);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish(
            (topics::TOPIC_CIRCUIT_BREAKER_RESET,),
            CircuitBreakerResetEvent { owner },
        );
    }

    /// Owner-callable emergency harvest fallback for agent-key outages.
    ///
    /// When the agent key is lost, compromised, or mid-rotation via
    /// `update_agent`'s timelock, accrued yield cannot be harvested through the
    /// normal `harvest()` path (which requires agent auth). This function
    /// provides a fallback gated by owner auth so that yield compounding can
    /// continue during an agent-key outage.
    ///
    /// Unlike `harvest()`, this function:
    /// - Requires **owner** auth instead of agent auth
    /// - Bypasses the paused-state check (owner may need to compound during
    ///   an emergency pause)
    /// - Still enforces: initialization, cooldown, and an active protocol
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `min_out` - Minimum amount the withdrawal must return (slippage floor).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `EmergencyHarvestEvent` with topic `TOPIC_EMERGENCY_HARVEST`
    ///
    /// # Errors
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - [`VaultError::CallerIsNotOwner`] if the caller is not the stored owner.
    /// - [`VaultError::UnsupportedProtocol`] if no active protocol exists.
    /// - [`VaultError::MinOutMustBeNonNegative`] if `min_out` is negative.
    /// - [`VaultError::RebalanceCooldownActive`] if called before the cooldown expires.
    ///
    /// # Panics
    ///
    /// None beyond the documented errors.
    pub fn emergency_harvest(env: Env, min_out: i128) {
        Self::require_initialized(&env);

        // Owner-gated (not agent-gated).
        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::CallerIsNotOwner);

        if min_out < 0 {
            panic_with_error!(&env, VaultError::MinOutMustBeNonNegative);
        }

        // Cooldown check (same as rebalance / harvest)
        if let Some(min_interval) = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::MinRebalanceInterval)
        {
            if min_interval > 0 {
                if let Some(last_rebalance) = env
                    .storage()
                    .instance()
                    .get::<DataKey, u32>(&DataKey::LastRebalanceLedger)
                {
                    let current_ledger = env.ledger().sequence();
                    let elapsed = current_ledger.saturating_sub(last_rebalance);
                    if elapsed < min_interval {
                        panic_with_error!(&env, VaultError::RebalanceCooldownActive);
                    }
                }
            }
        }

        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        if current_protocol == symbol_short!("none") {
            panic_with_error!(&env, VaultError::UnsupportedProtocol);
        }

        // Harvest also performs an external protocol round-trip. Reuse the
        // global rebalance bucket so it cannot bypass the frequency guard by
        // alternating between `rebalance` and `harvest`.
        Self::enforce_global_rate_limit(&env, RATE_LIMIT_REBALANCE);

        let withdrawn = Self::withdraw_from_protocol(&env, &current_protocol, min_out);

        if withdrawn > 0 {
            if current_protocol == symbol_short!("blend") {
                Self::supply_to_blend(&env, withdrawn, min_out);
            } else if current_protocol == symbol_short!("dex") {
                Self::supply_to_dex(&env, withdrawn, min_out);
            }
        }

        env.events().publish(
            (TOPIC_EMERGENCY_HARVEST,),
            EmergencyHarvestEvent {
                protocol: current_protocol,
                amount_harvested: withdrawn,
            },
        );

        env.storage()
            .instance()
            .set(&DataKey::LastRebalanceLedger, &env.ledger().sequence());
    }

    // ==========================================================================
    // ADMINISTRATIVE - MIGRATION CONTROL (#637)
    // ==========================================================================

    /// Sets the migration target vault address.
    ///
    /// Only the owner can set this address. Users can only migrate to the
    /// address set by the owner to prevent migration to malicious contracts.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `target` - The new vault contract address to allow migration to.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `MigrationTargetUpdatedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    pub fn set_migration_target(env: Env, target: Address) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        let old_target: Option<Address> = env.storage().instance().get(&DataKey::MigrationTarget);

        env.storage()
            .instance()
            .set(&DataKey::MigrationTarget, &target);

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        env.events().publish(
            (TOPIC_MIGRATION_TARGET_UPDATED,),
            MigrationTargetUpdatedEvent {
                old_target,
                new_target: target,
                owner,
            },
        );
    }

    /// Pauses or unpauses share migration independently of the main vault pause.
    ///
    /// The owner can pause migration without pausing deposits/withdrawals, or
    /// vice versa. This provides granular control during upgrades.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `paused` - `true` to pause migration, `false` to unpause.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `MigrationPausedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    pub fn set_migration_paused(env: Env, paused: bool) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        env.storage()
            .instance()
            .set(&DataKey::MigrationPaused, &paused);

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        env.events().publish(
            (TOPIC_MIGRATION_PAUSED,),
            MigrationPausedEvent { paused, owner },
        );
    }

    // ==========================================================================
    // ADMINISTRATIVE - CONFIGURATION
    // ==========================================================================

    /// Sets the TVL (Total Value Locked) cap for the vault.
    ///
    /// Maximum total USDC that can be deposited in the vault.
    /// Setting to 0 removes the cap.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `cap` - New TVL cap in USDC units (7 decimal places).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `TvlCapUpdatedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If the cap is negative.
    pub fn set_tvl_cap(env: Env, cap: i128) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        if cap < 0 {
            panic_with_error!(&env, VaultError::TvlCapCannotBeNegative);
        }

        let old_tvl_cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TvLCap)
            .unwrap_or(0_i128);

        env.storage().instance().set(&DataKey::TvLCap, &cap);

        env.events().publish(
            (TOPIC_TVL_CAP_UPDATED,),
            TvlCapUpdatedEvent {
                old_cap: old_tvl_cap,
                new_cap: cap,
                timestamp: env.ledger().timestamp(),
            },
        );
    }

    /// Sets the maximum deposit amount per user.
    ///
    /// Maximum amount that any single user can have deposited in the vault.
    /// Setting to 0 removes the cap.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `cap` - New per-user deposit cap in USDC units (7 decimal places).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `UserDepositCapUpdatedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If the cap is negative.
    pub fn set_user_deposit_cap(env: Env, cap: i128) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        if cap < 0 {
            panic_with_error!(&env, VaultError::UserDepositCapCannotBeNegative);
        }

        let old_user_cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::UserDepositCap)
            .unwrap_or(0_i128);

        env.storage().instance().set(&DataKey::UserDepositCap, &cap);

        env.events().publish(
            (TOPIC_USER_CAP_UPDATED,),
            UserDepositCapUpdatedEvent {
                old_cap: old_user_cap,
                new_cap: cap,
                timestamp: env.ledger().timestamp(),
            },
        );
    }

    /// Sets both the user deposit cap and TVL cap in a single transaction.
    ///
    /// This function allows updating both caps atomically and emits a
    /// `CapsUpdatedEvent` with all old and new values.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user_deposit_cap` - New per-user deposit cap in USDC units (7 decimal places).
    /// * `tvl_cap` - New TVL cap in USDC units (7 decimal places).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `CapsUpdatedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If user_deposit_cap is negative.
    /// - If tvl_cap is negative.
    /// - If tvl_cap is less than user_deposit_cap (when both are non-zero).
    pub fn set_caps(env: Env, user_deposit_cap: i128, tvl_cap: i128) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        if user_deposit_cap < 0 {
            panic_with_error!(&env, VaultError::UserDepositCapCannotBeNegative);
        }
        if tvl_cap < 0 {
            panic_with_error!(&env, VaultError::TvlCapCannotBeNegative);
        }
        if tvl_cap > 0 && user_deposit_cap > 0 && tvl_cap < user_deposit_cap {
            panic_with_error!(&env, VaultError::TvlCapBelowUserDepositCap);
        }

        let old_user_cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::UserDepositCap)
            .unwrap_or(0_i128);
        let old_tvl_cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TvLCap)
            .unwrap_or(0_i128);

        env.storage()
            .instance()
            .set(&DataKey::UserDepositCap, &user_deposit_cap);
        env.storage().instance().set(&DataKey::TvLCap, &tvl_cap);

        let timestamp = env.ledger().timestamp();

        env.events().publish(
            (TOPIC_USER_CAP_UPDATED,),
            UserDepositCapUpdatedEvent {
                old_cap: old_user_cap,
                new_cap: user_deposit_cap,
                timestamp,
            },
        );
        env.events().publish(
            (TOPIC_TVL_CAP_UPDATED,),
            TvlCapUpdatedEvent {
                old_cap: old_tvl_cap,
                new_cap: tvl_cap,
                timestamp,
            },
        );
        env.events().publish(
            (TOPIC_CAPS_UPDATED,),
            CapsUpdatedEvent {
                old_user_cap,
                new_user_cap: user_deposit_cap,
                old_tvl_cap,
                new_tvl_cap: tvl_cap,
            },
        );
    }

    /// Sets both the user deposit cap (min) and TVL cap (max) in a single transaction.
    ///
    /// # Deprecated
    /// This function is deprecated because its name and parameters ("min" / "max")
    /// are confusing and conflict with per-transaction deposit limits.
    /// Use `set_caps` instead.
    ///
    /// This function allows updating both limits atomically and emits a single
    /// `LimitsUpdatedEvent` with all old and new values.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `min` - New per-user deposit cap in USDC units (7 decimal places).
    ///   Despite the name, this is the user-deposit cap, not a per-deposit minimum.
    /// * `max` - New TVL cap in USDC units (7 decimal places).
    ///   Despite the name, this is the TVL cap, not a per-deposit maximum.
    ///
    /// Both values are validated to be non-negative, and `max` must be `>= min`,
    /// so a negative input can never silently disable a cap (issues #280, #281).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `VaultError` if validation fails.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `LimitsUpdatedEvent`
    ///
    /// # Errors
    ///
    /// Returns:
    /// - `VaultError::NegativeMin` if min is negative.
    /// - `VaultError::NegativeMax` if max is negative.
    /// - `VaultError::MaxLessThanMin` if max < min.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    pub fn set_limits(env: Env, min: i128, max: i128) -> Result<(), VaultError> {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        if min < 0 {
            return Err(VaultError::NegativeMin);
        }
        if max < 0 {
            return Err(VaultError::NegativeMax);
        }
        if max < min {
            return Err(VaultError::MaxLessThanMin);
        }

        let old_user_cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::UserDepositCap)
            .unwrap_or(0_i128);
        let old_tvl_cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TvLCap)
            .unwrap_or(0_i128);

        env.storage().instance().set(&DataKey::UserDepositCap, &min);
        env.storage().instance().set(&DataKey::TvLCap, &max);

        env.events().publish(
            (TOPIC_LIMITS_UPDATED,),
            LimitsUpdatedEvent {
                old_min: old_user_cap,
                new_min: min,
                old_max: old_tvl_cap,
                new_max: max,
            },
        );

        Ok(())
    }

    /// Sets both the minimum and maximum deposit limits in a single transaction.
    ///
    /// This function allows updating both deposit limits atomically and emits a
    /// `LimitsUpdatedEvent` with all old and new values.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `min` - New minimum deposit limit in USDC units (7 decimal places).
    /// * `max` - New maximum deposit limit in USDC units (7 decimal places).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `DepositLimitsUpdatedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If min is less than 1 USDC (1_000_000 stroops).
    /// - If max is less than min.
    /// - If max exceeds the absolute deposit ceiling.
    pub fn set_deposit_limits(env: Env, min: i128, max: i128) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        // Validate limits
        Self::require(
            &env,
            min >= DEFAULT_MIN_DEPOSIT,
            VaultError::InvalidStrategy,
        );
        Self::require(&env, max >= min, VaultError::InvalidStrategy);
        Self::require(
            &env,
            max <= MAX_DEPOSIT_CEILING,
            VaultError::MaximumDepositExceeded,
        );

        let old_min = env
            .storage()
            .instance()
            .get(&DataKey::MinDeposit)
            .unwrap_or(DEFAULT_MIN_DEPOSIT);
        let old_max = env
            .storage()
            .instance()
            .get(&DataKey::MaxDeposit)
            .unwrap_or(DEFAULT_MAX_DEPOSIT);

        env.storage().instance().set(&DataKey::MinDeposit, &min);
        env.storage().instance().set(&DataKey::MaxDeposit, &max);

        env.events().publish(
            (TOPIC_DEPOSIT_LIMITS_UPDATED,),
            DepositLimitsUpdatedEvent {
                old_min,
                new_min: min,
                old_max,
                new_max: max,
            },
        );
    }

    // ==========================================================================
    // ADMINISTRATIVE - REBALANCE COOLDOWN (Issue #59)
    // ==========================================================================

    /// Sets the minimum number of ledgers that must elapse between consecutive
    /// rebalance() calls.
    ///
    /// Only the owner can call this function. Setting `interval` to `0` disables
    /// the cooldown entirely (no throttle).
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `interval` - Minimum ledgers between rebalances. `0` = no cooldown.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `RebalanceCooldownUpdatedEvent`
    ///
    /// # Errors
    ///
    /// None. Failures panic rather than returning a `Result`.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - [`VaultError::CallerIsNotOwner`] if the caller is not the stored owner.
    ///
    /// # Storage
    ///
    /// Writes [`DataKey::MinRebalanceInterval`] in instance storage, or removes
    /// the key entirely when `interval == 0`. `rebalance` compares
    /// [`DataKey::LastRebalanceLedger`] against this value and panics with
    /// [`VaultError::RebalanceCooldownActive`] while the window is open.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Throttle the agent to roughly one rebalance per hour
    /// // (720 ledgers x ~5 s per ledger = 3,600 s).
    /// vault_client.set_rebalance_cooldown(&720);
    /// assert_eq!(vault_client.get_rebalance_cooldown(), 720);
    ///
    /// // Disable the throttle again.
    /// vault_client.set_rebalance_cooldown(&0);
    /// assert_eq!(vault_client.get_rebalance_cooldown(), 0);
    /// ```
    pub fn set_rebalance_cooldown(env: Env, interval: u32) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        let old_interval: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MinRebalanceInterval)
            .unwrap_or(0);

        if interval == 0 {
            // Removing the key disables the cooldown entirely.
            env.storage()
                .instance()
                .remove(&DataKey::MinRebalanceInterval);
        } else {
            env.storage()
                .instance()
                .set(&DataKey::MinRebalanceInterval, &interval);
        }

        env.events().publish(
            (TOPIC_REBALANCE_COOLDOWN_UPDATED,),
            RebalanceCooldownUpdatedEvent {
                old_interval,
                new_interval: interval,
            },
        );
    }

    /// Returns the configured minimum rebalance interval (ledgers), or `0` if
    /// no cooldown has been set.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    /// The minimum ledgers between rebalances, or `0` when disabled.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    pub fn get_rebalance_cooldown(env: Env) -> u32 {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::MinRebalanceInterval)
            .unwrap_or(0)
    }

    // ==========================================================================
    // ADMINISTRATIVE - RATE LIMITS
    // ==========================================================================

    /// Configures the call allowance for one rate-limit category.
    ///
    /// The limit is a fixed-window allowance. A value of `max_calls == 0`
    /// disables the category. When enabled, `window_ledgers` must be greater
    /// than zero and a bucket accepts at most `max_calls` calls before the
    /// window resets. Deposit, withdrawal, TTL, and batch categories are
    /// tracked per user; rebalance and preview categories are tracked globally.
    ///
    /// The category must be one of the public `RATE_LIMIT_*` symbols. This
    /// generic entrypoint lets the owner change policy without an upgrade.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `category` - Rate-limit category symbol.
    /// * `max_calls` - Maximum accepted calls in one window; `0` disables it.
    /// * `window_ledgers` - Window length in ledgers when enabled.
    ///
    /// # Events
    ///
    /// Emits `RateLimitConfigUpdatedEvent`.
    ///
    /// # Panics
    ///
    /// - [`VaultError::CallerIsNotOwner`] if the caller is not the owner.
    /// - [`VaultError::InvalidRateLimitCategory`] for an unknown category.
    /// - [`VaultError::InvalidRateLimitConfig`] when an enabled limit has a
    ///   zero-length window.
    pub fn set_rate_limit(env: Env, category: Symbol, max_calls: u32, window_ledgers: u32) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);
        Self::require_valid_rate_limit_category(&env, &category);

        if max_calls > 0 && window_ledgers == 0 {
            panic_with_error!(&env, VaultError::InvalidRateLimitConfig);
        }

        // A disabled category has one canonical representation. Accepting any
        // window with max_calls == 0 makes emergency disabling convenient while
        // keeping the getter unambiguous.
        let new_config = if max_calls == 0 {
            RateLimitConfig {
                max_calls: 0,
                window_ledgers: 0,
            }
        } else {
            RateLimitConfig {
                max_calls,
                window_ledgers,
            }
        };
        let old_config = Self::get_rate_limit_config_internal(&env, &category);

        env.storage()
            .instance()
            .set(&DataKey::RateLimitConfig(category.clone()), &new_config);
        // A global bucket can be reset without enumerating any user buckets.
        // User buckets retain their consumption until their configured window
        // expires, which prevents a caller from bypassing a newly tightened
        // policy by relying on a stale reset. Avoid an unnecessary storage
        // operation for per-user categories.
        if category == RATE_LIMIT_REBALANCE || category == RATE_LIMIT_PREVIEW {
            env.storage()
                .instance()
                .remove(&DataKey::RateLimitGlobalState(category.clone()));
        }

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        env.events().publish(
            (TOPIC_RATE_LIMIT_CONFIG_UPDATED,),
            RateLimitConfigUpdatedEvent {
                category,
                old_max_calls: old_config.max_calls,
                old_window_ledgers: old_config.window_ledgers,
                new_max_calls: new_config.max_calls,
                new_window_ledgers: new_config.window_ledgers,
                owner,
            },
        );
    }

    /// Alias for `set_rate_limit` using an explicit configuration-oriented
    /// name for SDK consumers.
    pub fn set_rate_limit_config(env: Env, category: Symbol, max_calls: u32, window_ledgers: u32) {
        Self::set_rate_limit(env, category, max_calls, window_ledgers);
    }

    /// Returns the configured allowance for a rate-limit category.
    ///
    /// The result includes the deployment default when the key is absent, so
    /// this getter remains useful for vaults initialized before rate limiting
    /// was added. Unknown categories are rejected rather than silently
    /// returning an unprotected configuration.
    pub fn get_rate_limit(env: Env, category: Symbol) -> RateLimitConfig {
        Self::require_initialized(&env);
        Self::get_rate_limit_config_internal(&env, &category)
    }

    /// Alias for `get_rate_limit` with an explicit configuration-oriented
    /// name for SDK consumers.
    pub fn get_rate_limit_config(env: Env, category: Symbol) -> RateLimitConfig {
        Self::require_initialized(&env);
        Self::get_rate_limit_config_internal(&env, &category)
    }

    /// Returns the global usage bucket for a rate-limit category.
    ///
    /// A never-used bucket is returned as `{ window_start: 0, calls: 0 }`.
    /// This is a read-only monitoring helper and is not itself rate-limited.
    pub fn get_global_rate_limit_state(env: Env, category: Symbol) -> RateLimitState {
        Self::require_initialized(&env);
        Self::read_rate_limit_state(&env, &category, None)
    }

    /// Returns a user's usage bucket for a rate-limit category.
    ///
    /// The bucket is stored in instance storage so its reset cannot be caused
    /// by persistent-entry TTL expiry. A never-used bucket is returned as
    /// `{ window_start: 0, calls: 0 }`.
    pub fn get_user_rate_limit_state(env: Env, user: Address, category: Symbol) -> RateLimitState {
        Self::require_initialized(&env);
        Self::read_rate_limit_state(&env, &category, Some(&user))
    }

    /// Sets the maximum number of entries accepted by `batch_deposit`.
    ///
    /// A value of `0` disables the batch-size guard. The separate
    /// `RATE_LIMIT_BATCH_DEPOSIT` call allowance remains active, so disabling
    /// the size guard does not disable frequency protection.
    ///
    /// # Events
    ///
    /// Emits `BatchSizeLimitUpdatedEvent`.
    pub fn set_max_batch_size(env: Env, max_entries: u32) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        let old_max_entries: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MaxBatchSize)
            .unwrap_or(DEFAULT_MAX_BATCH_SIZE);
        env.storage()
            .instance()
            .set(&DataKey::MaxBatchSize, &max_entries);

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        env.events().publish(
            (TOPIC_BATCH_SIZE_LIMIT_UPDATED,),
            BatchSizeLimitUpdatedEvent {
                old_max_entries,
                new_max_entries: max_entries,
                owner,
            },
        );
    }

    /// Returns the maximum number of entries accepted by `batch_deposit`.
    ///
    /// Returns `0` when the owner explicitly disabled the size guard.
    pub fn get_max_batch_size(env: Env) -> u32 {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::MaxBatchSize)
            .unwrap_or(DEFAULT_MAX_BATCH_SIZE)
    }

    /// Alias for `set_max_batch_size`.
    pub fn set_batch_size_limit(env: Env, max_entries: u32) {
        Self::set_max_batch_size(env, max_entries);
    }

    /// Alias for `get_max_batch_size`.
    pub fn get_batch_size_limit(env: Env) -> u32 {
        Self::get_max_batch_size(env)
    }

    /// Sets the minimum number of ledgers a user must hold their deposit before
    /// they can withdraw. Passing `0` disables the holding period (default).
    ///
    /// Only callable by the vault owner. Used to mitigate flash-loan attacks
    /// that manipulate share prices by depositing and immediately withdrawing
    /// in the same or adjacent transactions (#659).
    ///
    /// A ledger on Stellar closes approximately every 5 seconds. A holding
    /// period of 120 ledgers (~10 minutes) prevents same-block deposit/withdraw
    /// cycles while keeping withdrawal UX acceptable for normal users.
    ///
    /// # Panics
    /// - [`VaultError::CallerIsNotOwner`] if caller is not the owner.
    pub fn set_min_holding_period(env: Env, ledgers: u32) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        if ledgers == 0 {
            env.storage().instance().remove(&DataKey::MinHoldingPeriod);
        } else {
            env.storage()
                .instance()
                .set(&DataKey::MinHoldingPeriod, &ledgers);
        }
    }

    /// Returns the configured minimum holding period in ledgers, or `0` if
    /// no holding period has been set (flash-loan protection disabled).
    pub fn get_min_holding_period(env: Env) -> u32 {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::MinHoldingPeriod)
            .unwrap_or(0)
    }

    /// Records a suspected MEV extraction event after a rebalance (#658).
    ///
    /// Called by the off-chain agent when post-execution analysis suggests a
    /// sandwich attack occurred (e.g., actual received amount was less than
    /// expected even though `min_out` was satisfied). The agent computes the
    /// estimated loss by comparing the simulated vs. actual fill price.
    ///
    /// Emits `MevExtractionSuspectedEvent`. If the reported loss exceeds the
    /// owner-configured `MaxAcceptableMevLoss` threshold, the event payload
    /// signals indexers/monitoring to alert the team.
    ///
    /// # Arguments
    /// * `protocol` — Protocol on which MEV was detected ("blend" or "dex").
    /// * `estimated_loss_stroops` — Agent-estimated stroops lost to MEV.
    /// * `min_out_used` — The `min_out` parameter passed to `rebalance`.
    ///
    /// # Panics
    /// - The caller must be the authorized agent.
    /// - If `estimated_loss_stroops` is negative.
    pub fn submit_mev_report(
        env: Env,
        protocol: Symbol,
        estimated_loss_stroops: i128,
        min_out_used: i128,
    ) {
        Self::require_initialized(&env);
        Self::require_is_agent(&env);

        assert!(
            estimated_loss_stroops >= 0,
            "estimated_loss_stroops must be non-negative"
        );

        let cumulative: i128 = env
            .storage()
            .instance()
            .get(&DataKey::CumulativeMevLoss)
            .unwrap_or(0_i128);
        let new_cumulative = cumulative
            .checked_add(estimated_loss_stroops)
            .expect("vault: mev cumulative overflow");

        let incident_count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MevIncidentCount)
            .unwrap_or(0_u32);
        let new_count = incident_count.saturating_add(1);

        env.storage()
            .instance()
            .set(&DataKey::CumulativeMevLoss, &new_cumulative);
        env.storage()
            .instance()
            .set(&DataKey::MevIncidentCount, &new_count);

        env.events().publish(
            (symbol_short!("mev_alert"),),
            MevExtractionSuspectedEvent {
                protocol,
                estimated_loss_stroops,
                min_out_used,
                cumulative_loss_stroops: new_cumulative,
                incident_count: new_count,
            },
        );
    }

    /// Returns `(cumulative_mev_loss_stroops, incident_count)` for monitoring (#658).
    pub fn get_mev_stats(env: Env) -> (i128, u32) {
        Self::require_initialized(&env);
        let loss: i128 = env
            .storage()
            .instance()
            .get(&DataKey::CumulativeMevLoss)
            .unwrap_or(0);
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::MevIncidentCount)
            .unwrap_or(0);
        (loss, count)
    }

    /// Sets the maximum acceptable MEV loss per rebalance in stroops (#658).
    /// A value of `0` disables the threshold check.
    ///
    /// Only callable by the vault owner.
    pub fn set_max_acceptable_mev_loss(env: Env, max_loss_stroops: i128) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);
        assert!(
            max_loss_stroops >= 0,
            "max_loss_stroops must be non-negative"
        );
        env.storage()
            .instance()
            .set(&DataKey::MaxAcceptableMevLoss, &max_loss_stroops);
    }

    /// Stores an ML model APY prediction on-chain for a given protocol (#650).
    ///
    /// Called by the off-chain agent after running inference on the LSTM /
    /// Prophet model. Predictions are stored per-protocol and overwrite the
    /// previous forecast. The `rebalance()` call can then read the latest
    /// prediction via `get_apy_prediction` to make a more informed decision.
    ///
    /// # Arguments
    /// * `prediction` — Populated `ApyPrediction` struct from the agent.
    ///
    /// # Panics
    /// - The caller must be the authorized agent.
    pub fn submit_apy_prediction(env: Env, prediction: ApyPrediction) {
        Self::require_initialized(&env);
        Self::require_is_agent(&env);

        let key = DataKey::ApyPrediction(prediction.protocol.clone());
        let record = ApyPrediction {
            submitted_at_ledger: env.ledger().sequence(),
            ..prediction
        };
        env.storage().persistent().set(&key, &record);
    }

    /// Returns the most recent ML model APY prediction for a given protocol,
    /// or `None` if no prediction has been submitted yet (#650).
    ///
    /// The `rebalance` caller (agent) should check this value to decide the
    /// `expected_apy` argument for the next rebalance call. A `None` result
    /// means the agent should fall back to the current observed APY.
    pub fn get_apy_prediction(env: Env, protocol: Symbol) -> Option<ApyPrediction> {
        Self::require_initialized(&env);
        env.storage()
            .persistent()
            .get(&DataKey::ApyPrediction(protocol))
    }

    /// Returns the ledger sequence number of the most recent successful
    /// rebalance() call, or `0` if rebalance has never been called.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    /// The ledger of the last rebalance, or `0`.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    pub fn get_last_rebalance_ledger(env: Env) -> u32 {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::LastRebalanceLedger)
            .unwrap_or(0)
    }

    /// Sets the circuit-breaker threshold: the number of **consecutive** failed
    /// rebalances that trips an automatic emergency pause (Issue #439).
    ///
    /// Only the owner can call this. The counter increments on every rebalance
    /// that completes with a `"failed"` status and resets to zero on any
    /// `"success"`. Once the counter reaches `threshold`, `rebalance` pauses the
    /// vault (reusing the emergency-pause flag) and emits an
    /// [`EmergencyPausedEvent`]. The owner must `unpause` to resume operations.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `threshold` - Consecutive failures that trip the breaker. Must be `>= 1`.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - [`MaxConsecutiveFailuresUpdatedEvent`] with the previous effective
    ///   threshold and the newly configured one, so off-chain monitoring has a
    ///   full audit trail of circuit-breaker sensitivity changes.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - [`VaultError::CallerIsNotOwner`] if the caller is not the stored owner.
    /// - [`VaultError::InvalidStrategy`] if `threshold` is `0`. The error enum is
    ///   at Soroban's 50-variant limit, so `InvalidStrategy` is reused for
    ///   invalid configuration input (mirroring `rebalance`).
    pub fn set_max_consecutive_failures(env: Env, threshold: u32) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);
        Self::require(&env, threshold >= 1, VaultError::InvalidStrategy);

        let old_threshold = Self::effective_max_consecutive_failures(&env);

        env.storage()
            .instance()
            .set(&DataKey::MaxConsecutiveFailures, &threshold);

        env.events().publish(
            (TOPIC_MAX_FAILURES_UPDATED,),
            MaxConsecutiveFailuresUpdatedEvent {
                old_threshold,
                new_threshold: threshold,
            },
        );
    }

    /// Returns the configured circuit-breaker threshold (Issue #439), or
    /// the default value when the owner has not set one.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    /// The consecutive-failure threshold that trips the auto-pause.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    pub fn get_max_consecutive_failures(env: Env) -> u32 {
        Self::require_initialized(&env);
        Self::effective_max_consecutive_failures(&env)
    }

    /// Returns the current count of consecutive failed rebalances (Issue #439).
    ///
    /// Resets to `0` after any successful rebalance. Useful for monitoring how
    /// close the vault is to the auto-pause threshold.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    /// The number of consecutive failed rebalances since the last success.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    pub fn get_consecutive_failures(env: Env) -> u32 {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::ConsecutiveFailures)
            .unwrap_or(0)
    }

    /// Sets the shared lifetime, in ledgers, of the token approvals the vault
    /// grants to external protocols.
    ///
    /// The TTL applies to **both** the Blend and DEX integrations: before each
    /// supply leg the vault approves the pool to spend USDC until
    /// `current_ledger + ttl`. A short TTL limits the blast radius of a
    /// compromised pool; a long TTL avoids re-approving on every rebalance.
    ///
    /// Only the owner can call this function. The value is clamped to
    /// `[1_000, 500_000]` ledgers (roughly 1.4 to 29 days at ~5 s per ledger).
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `ttl` - Number of ledgers added to the current ledger when approving.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `ApprovalTtlUpdatedEvent`
    ///
    /// # Errors
    ///
    /// None. Failures panic rather than returning a `Result`.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - [`VaultError::CallerIsNotOwner`] if the caller is not the stored owner.
    /// - [`VaultError::ApprovalTtlTooLow`] if `ttl < 1_000`.
    /// - [`VaultError::ApprovalTtlTooHigh`] if `ttl > 500_000`.
    ///
    /// # Storage
    ///
    /// Writes [`DataKey::ApprovalTtl`]. The legacy
    /// [`DataKey::BlendApprovalTtl`] key is only read as a fallback for vaults
    /// initialized before the shared TTL existed, and is never written here.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Shorten approvals to ~1 day (17,280 ledgers x ~5 s).
    /// vault_client.set_approval_ttl(&17_280);
    /// assert_eq!(vault_client.get_approval_ttl(), 17_280);
    /// ```
    pub fn set_approval_ttl(env: Env, ttl: u32) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        if ttl < MIN_APPROVAL_TTL {
            panic_with_error!(&env, VaultError::ApprovalTtlTooLow);
        }
        if ttl > MAX_APPROVAL_TTL {
            panic_with_error!(&env, VaultError::ApprovalTtlTooHigh);
        }

        let old_ttl = Self::get_approval_ttl_internal(&env);

        env.storage().instance().set(&DataKey::ApprovalTtl, &ttl);

        env.events().publish(
            (TOPIC_APPROVAL_TTL_UPDATED,),
            ApprovalTtlUpdatedEvent {
                old_ttl,
                new_ttl: ttl,
            },
        );
    }

    /// Returns the shared protocol approval TTL in ledgers.
    ///
    /// Resolution order: [`DataKey::ApprovalTtl`], then the legacy
    /// [`DataKey::BlendApprovalTtl`] for vaults initialized before the shared
    /// key existed, then the `100_000`-ledger default (~5.7 days).
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Number of ledgers added to the current ledger when approving Blend or
    /// DEX pools to spend the vault's USDC.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    pub fn get_approval_ttl(env: Env) -> u32 {
        Self::require_initialized(&env);
        Self::get_approval_ttl_internal(&env)
    }

    // ==========================================================================
    // ADMINISTRATIVE - CONFIGURATION
    // ==========================================================================

    /// Returns the current TVL cap.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The current TVL cap in USDC units (7 decimal places), or 0 if no cap
    pub fn get_tvl_cap(env: Env) -> i128 {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::TvLCap)
            .unwrap_or(0_i128)
    }

    /// Returns the current per-user deposit cap.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The current per-user deposit cap in USDC units (7 decimal places), or 0 if no cap
    pub fn get_user_deposit_cap(env: Env) -> i128 {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::UserDepositCap)
            .unwrap_or(0_i128)
    }

    /// Returns the current minimum deposit limit.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The current minimum deposit limit in USDC units (7 decimal places)
    pub fn get_min_deposit(env: Env) -> i128 {
        Self::require_initialized(&env);
        Self::get_min_deposit_internal(&env)
    }

    /// Returns the current maximum deposit limit.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The current maximum deposit limit in USDC units (7 decimal places)
    pub fn get_max_deposit(env: Env) -> i128 {
        Self::require_initialized(&env);
        Self::get_max_deposit_internal(&env)
    }

    // ==========================================================================
    // USER STRATEGY PREFERENCE
    // ==========================================================================

    /// Sets the caller's investment strategy preference.
    ///
    /// Only the user themselves can set their own strategy (`require_auth`);
    /// neither the owner nor the agent can set it on their behalf. The
    /// preference is advisory: it is stored on-chain for the AI agent to read
    /// when choosing where to deploy funds, and does not by itself move any
    /// assets or restrict which protocol `rebalance` may target.
    ///
    /// Setting the same strategy twice is allowed and re-emits the event with
    /// `old_strategy == new_strategy`.
    ///
    /// **Storage-only — no on-chain effect on fund deployment.**
    /// `rebalance()` and `deposit()` never read `DataKey::UserStrategy`; the vault
    /// deploys funds pooled to a single `CurrentProtocol` regardless of any
    /// individual user's selection. The off-chain AI agent is expected to consume
    /// this preference when deciding yield allocation. A user's chosen strategy can
    /// therefore diverge from where their share of the pooled funds is actually
    /// deployed.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address; must authorize the call.
    /// * `strategy` - One of `"conservative"`, `"balanced"`, or `"growth"`.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits [`UserStrategyUpdatedEvent`] with topics
    /// `("usr_strat", user)`. `old_strategy` is the empty symbol `""` the first
    /// time a user sets a preference.
    ///
    /// # Errors
    ///
    /// None. Failures panic rather than returning a `Result`.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If `user` does not authorize the call.
    /// - [`VaultError::InvalidStrategy`] if `strategy` is not one of the three
    ///   accepted symbols.
    ///
    /// # Storage
    ///
    /// Writes [`DataKey::UserStrategy`] in persistent storage, keyed by `user`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// vault_client.set_user_strategy(&user, &Symbol::new(&env, "growth"));
    /// assert_eq!(
    ///     vault_client.get_user_strategy(&user),
    ///     Symbol::new(&env, "growth"),
    /// );
    /// ```
    pub fn set_user_strategy(env: Env, user: Address, strategy: Symbol) {
        Self::require_initialized(&env);
        user.require_auth();

        let valid = strategy == Symbol::new(&env, "conservative")
            || strategy == Symbol::new(&env, "balanced")
            || strategy == Symbol::new(&env, "growth");

        Self::require(&env, valid, VaultError::InvalidStrategy);

        let old_strategy: Symbol = env
            .storage()
            .persistent()
            .get(&DataKey::UserStrategy(user.clone()))
            .unwrap_or(Symbol::new(&env, ""));

        env.storage()
            .persistent()
            .set(&DataKey::UserStrategy(user.clone()), &strategy);

        env.events().publish(
            (TOPIC_USER_STRATEGY_UPDATED, user.clone()),
            UserStrategyUpdatedEvent {
                user,
                old_strategy,
                new_strategy: strategy,
            },
        );
    }

    /// Returns a user's investment strategy preference.
    ///
    /// Read-only and unauthenticated: any caller may query any address. Users
    /// who have never called [`set_user_strategy`](crate::NeuroWealthVault::set_user_strategy) are reported as
    /// `"balanced"`, so callers cannot distinguish "never set" from
    /// "explicitly set to balanced" through this function alone.
    ///
    /// **Storage-only — no on-chain effect on fund deployment.**
    /// This value is a per-user preference stored for the off-chain AI agent to
    /// read. `rebalance()` and `deposit()` do not consult it; the vault pools all
    /// funds to a single `CurrentProtocol`. See `set_user_strategy` for details.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address to query.
    ///
    /// # Returns
    ///
    /// The strategy symbol: `"conservative"`, `"balanced"`, or `"growth"`.
    /// Defaults to `"balanced"` when unset.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // A user who has never expressed a preference reads back as "balanced".
    /// assert_eq!(
    ///     vault_client.get_user_strategy(&fresh_user),
    ///     Symbol::new(&env, "balanced"),
    /// );
    /// ```
    pub fn get_user_strategy(env: Env, user: Address) -> Symbol {
        Self::require_initialized(&env);
        env.storage()
            .persistent()
            .get(&DataKey::UserStrategy(user))
            .unwrap_or(Symbol::new(&env, "balanced"))
    }

    /// Proposes an agent update with a 24-hour timelock (step 1 of 2). (#317)
    ///
    /// Records the new agent as pending and sets an expiry ledger after which
    /// `confirm_agent_update()` may be called. During the delay, operators and
    /// users can observe the proposal on-chain and react before the change takes
    /// effect. Only one pending proposal is allowed at a time.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `new_agent` - The proposed new AI agent address.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `AgentUpdateProposedEvent`
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If a pending agent update already exists (`TimelockAlreadyPending`).
    pub fn update_agent(env: Env, new_agent: Address) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        Self::require(
            &env,
            !env.storage().instance().has(&DataKey::PendingAgent),
            VaultError::TimelockAlreadyPending,
        );

        let old_agent: Address = env.storage().instance().get(&DataKey::Agent).unwrap();
        let effective_ledger = env
            .ledger()
            .sequence()
            .saturating_add(AGENT_TIMELOCK_LEDGERS);

        env.storage()
            .instance()
            .set(&DataKey::PendingAgent, &new_agent);
        env.storage()
            .instance()
            .set(&DataKey::AgentTimelockExpiry, &effective_ledger);

        env.events().publish(
            (TOPIC_AGENT_UPDATE_PROPOSED,),
            AgentUpdateProposedEvent {
                old_agent,
                new_agent,
                effective_ledger,
            },
        );
    }

    /// Confirms a pending agent update after the timelock has elapsed (step 2 of 2). (#317)
    ///
    /// Can only be called once `env.ledger().sequence() >= AgentTimelockExpiry`.
    /// On success the pending agent becomes the active agent and the proposal is cleared.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `AgentUpdateConfirmedEvent`
    /// - `AgentUpdatedEvent` (for backward-compatible indexers)
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If no pending proposal exists (`NoTimelockPending`).
    /// - If the timelock delay has not yet elapsed (`TimelockNotExpired`).
    pub fn confirm_agent_update(env: Env) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        Self::require(
            &env,
            env.storage().instance().has(&DataKey::PendingAgent),
            VaultError::NoTimelockPending,
        );

        let expiry: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AgentTimelockExpiry)
            .unwrap_or(0);

        Self::require(
            &env,
            env.ledger().sequence() >= expiry,
            VaultError::TimelockNotExpired,
        );

        let old_agent: Address = env.storage().instance().get(&DataKey::Agent).unwrap();
        let new_agent: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAgent)
            .unwrap();

        env.storage().instance().set(&DataKey::Agent, &new_agent);
        env.storage().instance().remove(&DataKey::PendingAgent);
        env.storage()
            .instance()
            .remove(&DataKey::AgentTimelockExpiry);

        env.events().publish(
            (TOPIC_AGENT_UPDATE_CONFIRMED,),
            AgentUpdateConfirmedEvent {
                old_agent: old_agent.clone(),
                new_agent: new_agent.clone(),
            },
        );

        // Emit backward-compatible event so existing indexers tracking TOPIC_AGENT_UPDATED see the change.
        env.events().publish(
            (TOPIC_AGENT_UPDATED,),
            AgentUpdatedEvent {
                old_agent,
                new_agent,
            },
        );
    }

    /// Cancels a pending agent update before it can be confirmed. (#317)
    ///
    /// Only the owner may cancel. Clears the pending proposal so a new one can
    /// be proposed. Safe to call at any point during the timelock window.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `AgentUpdateCancelledEvent`
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If no pending proposal exists (`NoTimelockPending`).
    pub fn cancel_agent_update(env: Env) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        Self::require(
            &env,
            env.storage().instance().has(&DataKey::PendingAgent),
            VaultError::NoTimelockPending,
        );

        let old_agent: Address = env.storage().instance().get(&DataKey::Agent).unwrap();
        let proposed_new_agent: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAgent)
            .unwrap();

        env.storage().instance().remove(&DataKey::PendingAgent);
        env.storage()
            .instance()
            .remove(&DataKey::AgentTimelockExpiry);

        env.events().publish(
            (TOPIC_AGENT_UPDATE_CANCELLED,),
            AgentUpdateCancelledEvent {
                old_agent,
                proposed_new_agent,
            },
        );
    }

    /// Returns the pending agent proposal, if one is active. (#317)
    ///
    /// Lets operators and indexers observe a proposed agent change during the
    /// 24-hour timelock window opened by [`update_agent`](crate::NeuroWealthVault::update_agent), and decide
    /// whether to let it proceed or call [`cancel_agent_update`](crate::NeuroWealthVault::cancel_agent_update).
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// * `Some((new_agent, effective_ledger))` while a proposal is pending,
    ///   where `effective_ledger` is the first ledger at which
    ///   [`confirm_agent_update`](crate::NeuroWealthVault::confirm_agent_update) may be called.
    /// * `None` when no proposal is pending — either none was made, or it was
    ///   already confirmed or cancelled.
    ///
    /// Compare `effective_ledger` against `env.ledger().sequence()` to tell a
    /// still-waiting proposal from a ready-to-confirm one. The currently active
    /// agent is unchanged until confirmation; read it with [`get_agent`](crate::NeuroWealthVault::get_agent).
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    ///
    /// # Storage
    ///
    /// Reads [`DataKey::PendingAgent`] and [`DataKey::AgentTimelockExpiry`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// match vault_client.get_pending_agent_update() {
    ///     None => { /* no agent change in flight */ }
    ///     Some((new_agent, effective_ledger)) => {
    ///         if env.ledger().sequence() >= effective_ledger {
    ///             vault_client.confirm_agent_update();
    ///         } else {
    ///             // Still inside the timelock window; cancel if unexpected.
    ///             let _ = new_agent;
    ///         }
    ///     }
    /// }
    /// ```
    pub fn get_pending_agent_update(env: Env) -> Option<(Address, u32)> {
        Self::require_initialized(&env);
        let pending: Option<Address> = env.storage().instance().get(&DataKey::PendingAgent);
        pending.map(|addr| {
            let expiry: u32 = env
                .storage()
                .instance()
                .get(&DataKey::AgentTimelockExpiry)
                .unwrap_or(0);
            (addr, expiry)
        })
    }

    /// Sets the Blend pool contract address for on-chain integration.
    ///
    /// Only the owner can set the Blend pool address. This must be called
    /// before the vault can interact with Blend for yield generation.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `owner` - The owner address (must authorize this call).
    /// * `pool_address` - The Blend pool contract address.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `BlendPoolConfiguredEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If the provided pool_address is not a valid Blend pool contract.
    pub fn set_blend_pool(env: Env, owner: Address, pool_address: Address) {
        Self::require_initialized(&env);
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(
            &env,
            owner == stored_owner,
            VaultError::OnlyOwnerCanConfigurePool,
        );

        // Idempotent no-op guard (Issue #438): if the Blend pool is already set
        // to this exact address, skip the interface probe, the storage write, and
        // the event so a redundant re-configuration emits nothing at all.
        let old_pool: Option<Address> = env.storage().instance().get(&DataKey::BlendPool);
        if old_pool.as_ref() == Some(&pool_address) {
            return;
        }

        // Validate pool interface by probing the `balance` function (Issue #148).
        // If the address is not a valid Blend pool contract the invocation will
        // panic here, rejecting the registration before the address is stored.
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let vault_address = env.current_contract_address();
        BlendPoolClient::get_balance(&env, &pool_address, &usdc_token, &vault_address);

        env.storage()
            .instance()
            .set(&DataKey::BlendPool, &pool_address);

        // Initialize CurrentProtocol to "none" if not set
        if !env.storage().instance().has(&DataKey::CurrentProtocol) {
            env.storage()
                .instance()
                .set(&DataKey::CurrentProtocol, &symbol_short!("none"));
        }

        env.events().publish(
            (TOPIC_BLEND_POOL_CONFIGURED,),
            BlendPoolConfiguredEvent {
                old_pool,
                new_pool: pool_address.clone(),
                owner: owner.clone(),
            },
        );
    }

    /// Configures the DEX liquidity pool contract address (owner only).
    ///
    /// Mirrors [`set_blend_pool`](crate::NeuroWealthVault::set_blend_pool). The pool interface is validated by probing
    /// its `balance` entrypoint before the address is stored, so an invalid pool
    /// address is rejected at configuration time. `CurrentProtocol` is initialized
    /// to `"none"` when not already set.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `owner` - The owner address (must authorize this call).
    /// * `pool_address` - The DEX liquidity pool contract address.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `DexPoolConfiguredEvent`
    ///
    /// # Panics
    ///
    /// - If the caller is not the owner.
    /// - If the provided `pool_address` is not a valid DEX pool contract.
    pub fn set_dex_pool(env: Env, owner: Address, pool_address: Address) {
        Self::require_initialized(&env);
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(
            &env,
            owner == stored_owner,
            VaultError::OnlyOwnerCanConfigurePool,
        );

        // Idempotent no-op guard (Issue #438): if the DEX pool is already set to
        // this exact address, skip the interface probe, the storage write, and the
        // event so a redundant re-configuration emits nothing at all.
        let old_pool: Option<Address> = env.storage().instance().get(&DataKey::DexPool);
        if old_pool.as_ref() == Some(&pool_address) {
            return;
        }

        // Validate the pool interface by probing the `balance` function. If the
        // address is not a valid DEX pool contract the invocation panics here,
        // rejecting the registration before the address is stored.
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let vault_address = env.current_contract_address();
        DexPoolClient::get_balance(&env, &pool_address, &usdc_token, &vault_address);

        env.storage()
            .instance()
            .set(&DataKey::DexPool, &pool_address);

        // Initialize CurrentProtocol to "none" if not set
        if !env.storage().instance().has(&DataKey::CurrentProtocol) {
            env.storage()
                .instance()
                .set(&DataKey::CurrentProtocol, &symbol_short!("none"));
        }

        env.events().publish(
            (TOPIC_DEX_POOL_CONFIGURED,),
            DexPoolConfiguredEvent {
                old_pool,
                new_pool: pool_address.clone(),
                owner: owner.clone(),
            },
        );
    }

    /// Updates the shared ledger TTL used when approving protocol token spend.
    ///
    /// The approval expiration ledger is computed as:
    /// `env.ledger().sequence() + blend_approval_ttl`
    ///
    /// # Events
    ///
    /// Emits:
    /// - [`ApprovalTtlUpdatedEvent`] (same topic as `set_approval_ttl`, since
    ///   both mutate the shared [`DataKey::ApprovalTtl`]), so indexers can
    ///   watch a single topic for every approval-TTL change.
    pub fn set_blend_approval_ttl(env: Env, owner: Address, blend_approval_ttl: u32) {
        Self::require_initialized(&env);
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::CallerIsNotOwner);

        let old_ttl = Self::get_approval_ttl_internal(&env);

        env.storage()
            .instance()
            .set(&DataKey::ApprovalTtl, &blend_approval_ttl);

        env.events().publish(
            (TOPIC_APPROVAL_TTL_UPDATED,),
            ApprovalTtlUpdatedEvent {
                old_ttl,
                new_ttl: blend_approval_ttl,
            },
        );
    }

    // ==========================================================================
    // ADMINISTRATIVE - OWNERSHIP TRANSFER
    // ==========================================================================

    /// Initiates ownership transfer to a new owner (step 1 of 2).
    ///
    /// This implements a two-step ownership transfer pattern for safety.
    /// The current owner proposes a new owner, and the new owner must
    /// explicitly accept ownership to complete the transfer.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `new_owner` - The proposed new owner address.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `OwnershipTransferInitiatedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the current owner.
    pub fn transfer_ownership(env: Env, new_owner: Address) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        let current_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();

        env.storage()
            .instance()
            .set(&DataKey::PendingOwner, &new_owner);

        env.events().publish(
            (TOPIC_OWNERSHIP_INITIATED,),
            OwnershipTransferInitiatedEvent {
                current_owner,
                pending_owner: new_owner,
            },
        );
    }

    /// Accepts ownership transfer (step 2 of 2).
    ///
    /// The pending owner must call this function to complete the ownership
    /// transfer. This ensures the new owner has access to their keys and
    /// prevents accidental transfers to wrong addresses.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `new_owner` - The new owner address (must match pending owner).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `OwnershipTransferredEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If there is no pending owner.
    /// - If the caller is not the pending owner.
    pub fn accept_ownership(env: Env, new_owner: Address) {
        Self::require_initialized(&env);
        new_owner.require_auth();

        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingOwner)
            .unwrap_or_else(|| panic_with_error!(&env, VaultError::CallerIsNotPendingOwner));

        Self::require(
            &env,
            new_owner == pending,
            VaultError::CallerIsNotPendingOwner,
        );

        let old_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();

        env.storage().instance().set(&DataKey::Owner, &new_owner);
        env.storage().instance().remove(&DataKey::PendingOwner);

        env.events().publish(
            (TOPIC_OWNERSHIP_TRANSFERRED,),
            OwnershipTransferredEvent {
                old_owner,
                new_owner,
            },
        );
    }

    /// Cancels a pending ownership transfer.
    ///
    /// Allows the current owner to cancel a pending ownership transfer
    /// if they change their mind or made a mistake.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `OwnershipTransferCancelledEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the caller is not the current owner.
    /// - If there is no pending ownership transfer.
    pub fn cancel_ownership_transfer(env: Env) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingOwner)
            .unwrap_or_else(|| panic_with_error!(&env, VaultError::CallerIsNotPendingOwner));

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();

        env.storage().instance().remove(&DataKey::PendingOwner);

        env.events().publish(
            (TOPIC_OWNERSHIP_CANCELLED,),
            OwnershipTransferCancelledEvent {
                owner,
                cancelled_pending: pending,
            },
        );
    }

    /// Returns the pending owner address, if any.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The pending owner address, or None if no transfer is pending
    pub fn get_pending_owner(env: Env) -> Option<Address> {
        Self::require_initialized(&env);
        env.storage().instance().get(&DataKey::PendingOwner)
    }

    /// Returns the pending ownership information, if any.
    ///
    /// Provides both the pending owner address and timelock expiry in a single
    /// struct, making it easier for off-chain monitors to display pending
    /// ownership transfers.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// `Some(PendingOwnershipInfo)` if a transfer is pending, `None` otherwise
    pub fn get_pending_ownership(env: Env) -> Option<PendingOwnershipInfo> {
        Self::require_initialized(&env);
        let pending_owner: Option<Address> = env.storage().instance().get(&DataKey::PendingOwner);
        pending_owner.map(|owner| PendingOwnershipInfo {
            pending_owner: owner,
            timelock_expiry: 0,
        })
    }

    /// Updates the total assets tracked by the vault.
    ///
    /// The agent calls this to reflect realized yield (increase) or a confirmed
    /// strategy loss / bad-debt write-down (decrease).
    ///
    /// ## Decrease policy
    ///
    /// Decreases are permitted only when **all** of the following hold:
    ///
    /// 1. `allow_decrease` is `true` — the caller explicitly opts in.
    /// 2. The **owner** has co-signed this transaction (`owner.require_auth()`).
    ///    A rogue agent cannot unilaterally slash user value; the loss must be
    ///    countersigned by the vault operator.
    /// 3. The decrease does not exceed `max_decrease_bps` basis points of the
    ///    current total (minimum floor: 100 bps = 1%). This caps the worst-case
    ///    loss any single call can commit, limiting damage from a compromised key.
    ///
    /// Typical values: `allow_decrease = true`, `max_decrease_bps = 1000` (10 %).
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `agent` - The authorized AI agent address.
    /// * `new_total` - The new total asset amount (principal + yield).
    /// * `allow_decrease` - If true, permits the reported assets to decrease.
    /// * `max_decrease_bps` - Maximum allowed decrease in basis points (e.g., 500 = 5%).
    ///
    /// # Returns
    ///
    /// None.
    ///
    /// # Events
    ///
    /// Emits:
    /// - `AssetsUpdatedEvent`
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If called by anyone other than the authorized agent.
    /// - If new_total is negative.
    /// - If there are assets but total_shares is zero.
    /// - If the vault does not hold enough USDC to back the new_total.
    /// - If a decrease is attempted but not allowed.
    /// - If a decrease exceeds the maximum basis points.
    /// - If a decrease lacks the vault owner's authorization.
    pub fn update_total_assets(
        env: Env,
        agent: Address,
        new_total: i128,
        allow_decrease: bool,
        max_decrease_bps: u32,
    ) {
        Self::require_initialized(&env);
        let stored_agent: Address = env.storage().instance().get(&DataKey::Agent).unwrap();
        Self::require(
            &env,
            agent == stored_agent,
            VaultError::OnlyAgentCanUpdateTotalAssets,
        );
        agent.require_auth();

        let old_total = Self::get_total_assets_internal(&env);

        if new_total < old_total {
            Self::require(
                &env,
                allow_decrease,
                VaultError::TotalAssetsDecreaseNotAllowed,
            );

            // Owner must co-sign any loss report. A single compromised key
            // cannot unilaterally reduce user asset values.
            Self::require_is_owner(&env);

            // Cap the per-call decrease (minimum floor: 100 bps = 1 %).
            let effective_cap_bps = max_decrease_bps.max(100);
            let max_decrease = old_total
                .checked_mul(effective_cap_bps as i128)
                .expect("vault: total available overflow")
                .checked_div(10_000)
                .expect("vault: total available overflow");
            let actual_decrease = old_total
                .checked_sub(new_total)
                .expect("vault: decrease underflow");

            Self::require(
                &env,
                actual_decrease <= max_decrease,
                VaultError::DecreaseExceedsMaximumAllowedBps,
            );
        }

        // CRITICAL SECURITY CHECK: Verify vault actually holds sufficient USDC
        // This prevents the agent from inflating total_assets beyond what the vault can pay out
        // We must include both idle funds in vault AND funds deployed to Blend
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        let vault_balance = token_client.balance(&env.current_contract_address());

        let mut total_available = vault_balance;

        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        if current_protocol == symbol_short!("blend")
            && env.storage().instance().has(&DataKey::BlendPool)
        {
            let blend_pool: Address = env.storage().instance().get(&DataKey::BlendPool).unwrap();
            let deployed_balance = BlendPoolClient::get_balance(
                &env,
                &blend_pool,
                &usdc_token,
                &env.current_contract_address(),
            );
            total_available = total_available
                .checked_add(deployed_balance)
                .expect("vault: total available overflow");
        }

        if current_protocol == symbol_short!("dex")
            && env.storage().instance().has(&DataKey::DexPool)
        {
            let dex_pool: Address = env.storage().instance().get(&DataKey::DexPool).unwrap();
            let deployed_balance = DexPoolClient::get_balance(
                &env,
                &dex_pool,
                &usdc_token,
                &env.current_contract_address(),
            );
            total_available = total_available
                .checked_add(deployed_balance)
                .expect("vault: total available overflow");
        }

        Self::require(
            &env,
            total_available >= new_total,
            VaultError::InsufficientBalanceForAssets,
        );

        env.storage()
            .instance()
            .set(&DataKey::TotalAssets, &new_total);

        env.events().publish(
            (TOPIC_ASSETS_UPDATED,),
            AssetsUpdatedEvent {
                old_total,
                new_total,
            },
        );
    }

    // ==========================================================================
    // ADMINISTRATIVE - UPGRADES
    // ==========================================================================

    /// Schedules a contract upgrade behind a timelock (step 1 of 2). (#316)
    ///
    /// Records `new_wasm_hash` as the pending upgrade and sets an expiry ledger
    /// after which `execute_upgrade()` may be called. The delay
    /// (`UPGRADE_TIMELOCK_LEDGERS`, ≈24 h) gives users and operators a recovery
    /// window to observe the proposal on-chain and react — including calling
    /// `cancel_upgrade()` — before new WASM takes effect. This closes the
    /// "compromised owner key swaps WASM instantly" gap. Only one pending
    /// upgrade is allowed at a time.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `owner` - The owner address (must authorize).
    /// * `new_wasm_hash` - Hash of the new WASM binary (32 bytes).
    ///
    /// # Events
    ///
    /// Emits:
    /// - `UpgradeScheduledEvent`
    ///
    /// # Panics
    ///
    /// - If the vault is paused.
    /// - If the caller is not the stored owner.
    /// - If an upgrade is already pending (`TimelockAlreadyPending`).
    pub fn schedule_upgrade(env: Env, owner: Address, new_wasm_hash: BytesN<32>) {
        Self::require_initialized(&env);
        owner.require_auth();
        Self::require_not_paused(&env);

        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::CallerIsNotOwner);

        Self::require(
            &env,
            !env.storage().instance().has(&DataKey::PendingUpgradeHash),
            VaultError::TimelockAlreadyPending,
        );

        Self::require(
            &env,
            new_wasm_hash != BytesN::from_array(&env, &[0u8; 32]),
            VaultError::InvalidWasmHash,
        );

        let effective_ledger = env
            .ledger()
            .sequence()
            .saturating_add(UPGRADE_TIMELOCK_LEDGERS);

        env.storage()
            .instance()
            .set(&DataKey::PendingUpgradeHash, &new_wasm_hash);
        env.storage()
            .instance()
            .set(&DataKey::UpgradeTimelockExpiry, &effective_ledger);

        env.events().publish(
            (TOPIC_UPGRADE_SCHEDULED,),
            UpgradeScheduledEvent {
                new_wasm_hash,
                effective_ledger,
            },
        );
    }

    /// Executes a scheduled upgrade after the timelock has elapsed (step 2 of 2). (#316)
    ///
    /// Can only be called once `env.ledger().sequence() >= UpgradeTimelockExpiry`.
    /// On success the pending WASM hash is activated, the contract `Version` is
    /// incremented, and the pending proposal is cleared. All storage state (user
    /// balances, configuration, owner, agent) is preserved across the upgrade.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `owner` - The owner address (must authorize).
    ///
    /// # Events
    ///
    /// Emits:
    /// - `UpgradedEvent`
    ///
    /// # Panics
    ///
    /// - If the vault is paused.
    /// - If the caller is not the stored owner.
    /// - If no pending upgrade exists (`NoTimelockPending`).
    /// - If the timelock delay has not yet elapsed (`TimelockNotExpired`).
    /// - If the pending hash does not correspond to an uploaded WASM binary.
    pub fn execute_upgrade(env: Env, owner: Address) {
        Self::require_initialized(&env);
        owner.require_auth();
        Self::require_not_paused(&env);

        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::CallerIsNotOwner);

        Self::require(
            &env,
            env.storage().instance().has(&DataKey::PendingUpgradeHash),
            VaultError::NoTimelockPending,
        );

        let expiry: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UpgradeTimelockExpiry)
            .unwrap_or(0);

        Self::require(
            &env,
            env.ledger().sequence() >= expiry,
            VaultError::TimelockNotExpired,
        );

        let new_wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgradeHash)
            .unwrap();

        // Clear the pending proposal before applying the upgrade so a fresh
        // proposal can be scheduled afterwards.
        env.storage()
            .instance()
            .remove(&DataKey::PendingUpgradeHash);
        env.storage()
            .instance()
            .remove(&DataKey::UpgradeTimelockExpiry);

        // Soroban will trap/panic here if the hash is not installed on the network.
        env.deployer().update_current_contract_wasm(new_wasm_hash);

        let old_version: u32 = env.storage().instance().get(&DataKey::Version).unwrap_or(1);
        let new_version = old_version.checked_add(1).expect("vault: version overflow");
        env.storage()
            .instance()
            .set(&DataKey::Version, &new_version);

        env.events().publish(
            (TOPIC_UPGRADED,),
            UpgradedEvent {
                old_version,
                new_version,
            },
        );
    }

    /// Cancels a pending upgrade before it can be executed. (#316)
    ///
    /// Only the owner may cancel. Clears the pending proposal so a new one can
    /// be scheduled. Safe to call at any point during the timelock window — this
    /// is the recovery path if a malicious or mistaken upgrade was scheduled.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `owner` - The owner address (must authorize).
    ///
    /// # Events
    ///
    /// Emits:
    /// - `UpgradeCancelledEvent`
    ///
    /// # Panics
    ///
    /// - If the caller is not the stored owner.
    /// - If no pending upgrade exists (`NoTimelockPending`).
    pub fn cancel_upgrade(env: Env, owner: Address) {
        Self::require_initialized(&env);
        owner.require_auth();

        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::CallerIsNotOwner);

        Self::require(
            &env,
            env.storage().instance().has(&DataKey::PendingUpgradeHash),
            VaultError::NoTimelockPending,
        );

        let cancelled_wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::PendingUpgradeHash)
            .unwrap();

        env.storage()
            .instance()
            .remove(&DataKey::PendingUpgradeHash);
        env.storage()
            .instance()
            .remove(&DataKey::UpgradeTimelockExpiry);

        env.events().publish(
            (TOPIC_UPGRADE_CANCELLED,),
            UpgradeCancelledEvent {
                cancelled_wasm_hash,
            },
        );
    }

    /// Returns the pending upgrade proposal, if one is active. (#316)
    ///
    /// This is the public monitoring hook for the two-step timelocked upgrade.
    /// Users and watchtowers should poll it (or subscribe to
    /// [`UpgradeScheduledEvent`]) to learn about a scheduled code change while
    /// there is still time to exit the vault or for the owner to call
    /// [`cancel_upgrade`](crate::NeuroWealthVault::cancel_upgrade).
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// * `Some((new_wasm_hash, effective_ledger))` while an upgrade is pending,
    ///   where `new_wasm_hash` is the WASM that [`execute_upgrade`](crate::NeuroWealthVault::execute_upgrade) will
    ///   activate and `effective_ledger` is the first ledger at which it may be
    ///   executed.
    /// * `None` when no upgrade is pending — either none was scheduled, or it
    ///   was already executed or cancelled.
    ///
    /// A `Some(..)` result does **not** mean the contract code has changed; the
    /// running code is only replaced by [`execute_upgrade`](crate::NeuroWealthVault::execute_upgrade). Compare the
    /// returned hash against the WASM you expect before allowing the timelock
    /// to elapse.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    ///
    /// # Storage
    ///
    /// Reads [`DataKey::PendingUpgradeHash`] and
    /// [`DataKey::UpgradeTimelockExpiry`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// if let Some((wasm_hash, effective_ledger)) = vault_client.get_pending_upgrade() {
    ///     let ledgers_remaining = effective_ledger.saturating_sub(env.ledger().sequence());
    ///     if wasm_hash != expected_wasm_hash {
    ///         // Unexpected code scheduled - cancel within the timelock window.
    ///         vault_client.cancel_upgrade(&owner);
    ///     }
    ///     let _ = ledgers_remaining;
    /// }
    /// ```
    pub fn get_pending_upgrade(env: Env) -> Option<(BytesN<32>, u32)> {
        Self::require_initialized(&env);
        let pending: Option<BytesN<32>> =
            env.storage().instance().get(&DataKey::PendingUpgradeHash);
        pending.map(|hash| {
            let expiry: u32 = env
                .storage()
                .instance()
                .get(&DataKey::UpgradeTimelockExpiry)
                .unwrap_or(0);
            (hash, expiry)
        })
    }

    // ==========================================================================
    // READ FUNCTIONS
    // ==========================================================================

    /// Reads a user's share balance from persistent storage (no TTL side effects).
    fn read_shares(env: &Env, user: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128)
    }

    /// Records `user` in the active-share index the first time they hold
    /// non-zero shares (Issue #440).
    ///
    /// The index is an append-only `Vec<Address>` in instance storage. Entries
    /// are never removed, so a user who fully withdraws keeps their slot; the
    /// `contains` guard keeps the index duplicate-free when such a user later
    /// re-deposits. `get_users_with_shares` filters the stale zero-share slots
    /// out at read time. See that function for the pagination trade-off.
    fn add_to_user_index(env: &Env, user: &Address) {
        let mut index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::UserSharesIndex)
            .unwrap_or_else(|| Vec::new(env));
        if !index.contains(user) {
            index.push_back(user.clone());
            env.storage()
                .instance()
                .set(&DataKey::UserSharesIndex, &index);
        }
    }

    /// Returns the effective circuit-breaker threshold (Issue #439), falling
    /// back to [`DEFAULT_MAX_CONSECUTIVE_FAILURES`] for instances initialized
    /// before the circuit breaker existed.
    fn effective_max_consecutive_failures(env: &Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MaxConsecutiveFailures)
            .unwrap_or(DEFAULT_MAX_CONSECUTIVE_FAILURES)
    }

    /// Folds a rebalance outcome into the consecutive-failure circuit breaker
    /// (Issue #439).
    ///
    /// A `"failed"` status increments the counter; a `"success"` status resets
    /// it to zero. `"partial"` and `"noop"` outcomes leave the counter
    /// unchanged. When the counter reaches
    /// [`DataKey::MaxConsecutiveFailures`] the vault auto-pauses by setting the
    /// shared [`DataKey::Paused`] flag and emitting the existing
    /// [`EmergencyPausedEvent`]. The pause is applied at most once per trip: if
    /// the vault is already paused no duplicate event is emitted.
    fn record_rebalance_outcome(env: &Env, status: &Symbol) {
        if *status == symbol_short!("failed") {
            let failures = env
                .storage()
                .instance()
                .get::<DataKey, u32>(&DataKey::ConsecutiveFailures)
                .unwrap_or(0)
                .saturating_add(1);
            env.storage()
                .instance()
                .set(&DataKey::ConsecutiveFailures, &failures);

            if failures >= Self::effective_max_consecutive_failures(env) {
                let already_paused: bool = env
                    .storage()
                    .instance()
                    .get(&DataKey::Paused)
                    .unwrap_or(false);
                if !already_paused {
                    env.storage().instance().set(&DataKey::Paused, &true);
                    let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
                    env.events()
                        .publish((TOPIC_EMERGENCY_PAUSED,), EmergencyPausedEvent { owner });
                }
            }
        } else if *status == symbol_short!("success") {
            env.storage()
                .instance()
                .set(&DataKey::ConsecutiveFailures, &0_u32);
        }
    }

    /// Extends the persistent TTL for a user's `Shares` entry when it exists.
    fn extend_user_shares_ttl(env: &Env, user: &Address) {
        let shares_key = DataKey::Shares(user.clone());
        if env.storage().persistent().has(&shares_key) {
            env.storage().persistent().extend_ttl(
                &shares_key,
                USER_SHARES_TTL_THRESHOLD,
                USER_SHARES_TTL_EXTEND_TO,
            );
        }
    }

    /// Reduces `TotalDeposits` by `amount`, flooring at zero.
    ///
    /// `TotalDeposits` tracks principal only. On withdrawal the returned amount
    /// may include accrued yield, so the subtraction is saturating rather than
    /// exact to avoid underflow.
    fn reduce_total_deposits_on_withdraw(env: &Env, amount: i128) {
        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0_i128);
        env.storage().instance().set(
            &DataKey::TotalDeposits,
            &total.saturating_sub(amount).max(0_i128),
        );
    }

    /// Returns the USDC balance of a specific user.
    ///
    /// This is the user's claim on the vault's total managed assets, based
    /// on their share balance. It includes any yield that has been accrued
    /// and reflected in `TotalAssets`.
    ///
    /// This is a pure read and does not extend persistent storage TTL. See
    /// `touch_user_ttl` for explicit TTL maintenance.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address to query.
    ///
    /// # Returns
    ///
    /// Returns the user's USDC-equivalent balance in raw units (7 decimal places).
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_balance(env: Env, user: Address) -> i128 {
        Self::require_initialized(&env);

        let shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);
        if shares == 0 {
            return 0;
        }

        let total_shares = Self::get_total_shares_internal(&env);
        let total_assets = Self::get_total_assets_internal(&env);

        if total_shares == 0 || total_assets == 0 {
            0
        } else {
            // User's pro-rata claim: (user_shares / total_shares) * total_assets
            shares
                .checked_mul(total_assets)
                .expect("vault: share to asset conversion overflow")
                .checked_div(total_shares)
                .expect("vault: conversion div error")
        }
    }

    /// Returns the total USDC principal deposited in the vault (issue #299).
    ///
    /// **Relationship between `TotalDeposits`, `TotalAssets`, and shares:**
    ///
    /// | Value | Includes yield? | Used for |
    /// |---|---|---|
    /// | `TotalDeposits` | No  | Principal bookkeeping and reporting only |
    /// | `TotalAssets`   | Yes | Share pricing, TVL cap guard, user balances |
    ///
    /// After `update_total_assets()` is called to reflect external yield,
    /// `TotalAssets >= TotalDeposits`.  All economic quantities — share minting,
    /// user redemption amounts, and the TVL cap check — use `TotalAssets`, never
    /// `TotalDeposits`.  `TotalDeposits` is intentionally not synced on yield
    /// updates; it is a principal-only counter useful for reporting.
    ///
    /// See also: `get_total_assets()`, ARCHITECTURE.md §"TotalDeposits vs TotalAssets".
    ///
    /// # Returns
    ///
    /// Returns total USDC principal deposits in raw units (7 decimal places).
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_total_deposits(env: Env) -> i128 {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0_i128)
    }

    /// Returns the total managed assets of the vault (principal + yield).
    ///
    /// This value is used for share pricing and reflects the full value
    /// backing all outstanding shares.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the total managed assets.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_total_assets(env: Env) -> i128 {
        Self::require_initialized(&env);
        Self::get_total_assets_internal(&env)
    }

    /// Returns the total number of shares in circulation.
    ///
    /// This is the sum of all user shares and represents proportional ownership
    /// of the vault's total assets.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the total number of shares.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_total_shares(env: Env) -> i128 {
        Self::require_initialized(&env);
        Self::get_total_shares_internal(&env)
    }

    /// Returns the share balance of a specific user.
    ///
    /// This is the number of vault shares the user owns.
    ///
    /// This is a pure read and does not extend persistent storage TTL. Call
    /// `touch_user_ttl` when an off-chain maintainer or indexer needs to
    /// refresh rent for a user's `Shares` entry.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address.
    ///
    /// # Returns
    ///
    /// Returns the number of shares the user owns.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_shares(env: Env, user: Address) -> i128 {
        Self::require_initialized(&env);
        Self::read_shares(&env, &user)
    }

    /// Returns a paginated slice of `(user, shares)` pairs for holders with a
    /// positive share balance, for off-chain indexer support (Issue #440).
    ///
    /// The vault maintains an append-only index of every address that has ever
    /// held non-zero shares (populated on deposit).
    ///
    /// # Pagination and the stale-entry trade-off
    ///
    /// Pagination is applied over the **raw index positions** `[start, start +
    /// limit)`, and the `shares > 0` filter is applied *within* that window. The
    /// index is never pruned on withdrawal, so a fully-withdrawn holder keeps its
    /// slot as a zero-share entry that is filtered out here. This keeps writes
    /// cheap (deposits only ever append, and only for genuinely new holders) at
    /// the cost of two documented read-time behaviours:
    ///
    /// - A page may return **fewer than `limit`** entries (or even be empty) while
    ///   later pages still hold results, because stale slots occupy positions.
    /// - Callers walk pages by advancing `start` by `limit` until an empty index
    ///   window is returned (i.e. `start >= total index length`), not until a
    ///   short page is seen.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `start` - Zero-based index offset to begin the page at.
    /// * `limit` - Maximum number of index slots to scan for this page.
    ///
    /// # Returns
    ///
    /// A `Vec<(Address, i128)>` of holders in `[start, start + limit)` whose
    /// share balance is strictly positive. Empty when `limit == 0` or `start` is
    /// beyond the end of the index.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    pub fn get_users_with_shares(env: Env, start: u32, limit: u32) -> Vec<(Address, i128)> {
        Self::require_initialized(&env);

        let mut result: Vec<(Address, i128)> = Vec::new(&env);
        if limit == 0 {
            return result;
        }

        let index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::UserSharesIndex)
            .unwrap_or_else(|| Vec::new(&env));

        let len = index.len();
        if start >= len {
            return result;
        }
        let end = core::cmp::min(start.saturating_add(limit), len);

        for i in start..end {
            let user = index.get(i).unwrap();
            let shares = Self::read_shares(&env, &user);
            if shares > 0 {
                result.push_back((user, shares));
            }
        }

        result
    }

    /// Extends the persistent TTL for a user's `Shares` entry.
    ///
    /// Off-chain indexers and maintenance jobs should call this instead of relying
    /// on read-only getters (`get_balance`, `get_shares`) to keep user share data
    /// from expiring. State-changing calls such as `deposit` and `withdraw` already
    /// rewrite `Shares` and refresh TTL as part of normal ledger writes.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user whose share entry TTL should be extended.
    ///
    /// # Returns
    ///
    /// Returns `true` if a `Shares` entry existed and TTL was extended; `false` otherwise.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - [`VaultError::RateLimitExceeded`] if this user's TTL bucket is exhausted.
    ///
    /// # Storage
    ///
    /// Extends the TTL of [`DataKey::Shares`] in persistent storage when the
    /// entry exists. Never creates an entry, so calling this for an address
    /// that has never deposited is a cheap no-op returning `false`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Keeper job: refresh every tracked holder, skipping addresses that
    /// // hold no shares.
    /// for user in tracked_users {
    ///     if !vault_client.touch_user_ttl(&user) {
    ///         // No Shares entry - drop the address from the keeper set.
    ///     }
    /// }
    /// ```
    pub fn touch_user_ttl(env: Env, user: Address) -> bool {
        Self::require_initialized(&env);
        // Count maintenance attempts even when the requested entry is absent;
        // otherwise an attacker could use missing-user probes as a cheap DoS.
        Self::enforce_user_rate_limit(&env, &user, RATE_LIMIT_TOUCH_TTL);
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Shares(user.clone()))
        {
            return false;
        }
        Self::extend_user_shares_ttl(&env, &user);
        true
    }

    /// Returns both the principal balance and share balance for a user.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `user` - The user address.
    ///
    /// # Returns
    ///
    /// Returns `UserInfo` containing principal and shares.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_user_info(env: Env, user: Address) -> UserInfo {
        Self::require_initialized(&env);
        let shares = Self::read_shares(&env, &user);
        let principal = Self::convert_to_assets_internal(&env, shares);

        UserInfo { principal, shares }
    }

    /// Previews the number of shares that would be minted for a given deposit.
    ///
    /// Uses **floor** rounding, matching `deposit`: the caller may receive
    /// fractionally fewer shares than exact division would give, and the
    /// remainder accrues to the vault. This is the ERC-4626 convention.
    ///
    /// Read-only, but the preview is only valid for the current ledger state —
    /// a deposit, withdrawal, or `update_total_assets` landing in between can
    /// move the share price. Frontends should re-read immediately before
    /// submitting.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `assets` - The amount of USDC to deposit, in raw units (7 decimals).
    ///
    /// # Returns
    ///
    /// The number of shares that would be minted:
    /// `floor(assets * total_shares / total_assets)`, or `assets` in the
    /// bootstrap case where `total_shares == 0` or `total_assets == 0`.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If the intermediate `assets * total_shares` product overflows `i128`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Vault holding 1,100 USDC against 1,000 shares (10% yield accrued).
    /// // Depositing 110 USDC mints ~100 shares.
    /// let shares = vault_client.preview_deposit_to_shares(&1_100_000_000);
    /// ```
    pub fn preview_deposit_to_shares(env: Env, assets: i128) -> i128 {
        Self::require_initialized(&env);
        Self::enforce_global_rate_limit(&env, RATE_LIMIT_PREVIEW);
        Self::convert_to_shares_internal(&env, assets)
    }

    /// Previews the USDC value of a given number of shares.
    ///
    /// Uses **floor** rounding, matching the redemption path. Use this to show
    /// a holder's current position value; use [`preview_withdraw`](crate::NeuroWealthVault::preview_withdraw) to
    /// show how many shares a *target withdrawal amount* would burn.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `shares` - The number of vault shares to value.
    ///
    /// # Returns
    ///
    /// `floor(shares * total_assets / total_shares)` in USDC raw units
    /// (7 decimals), or `0` when the vault holds no shares or no assets.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If the intermediate `shares * total_assets` product overflows `i128`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Display a user's position in USDC.
    /// let shares = vault_client.get_shares(&user);
    /// let value = vault_client.preview_shares_to_assets(&shares);
    /// ```
    pub fn preview_shares_to_assets(env: Env, shares: i128) -> i128 {
        Self::require_initialized(&env);
        Self::enforce_global_rate_limit(&env, RATE_LIMIT_PREVIEW);
        Self::convert_to_assets_internal(&env, shares)
    }

    /// Previews the number of shares that would be burned for a given asset withdrawal.
    ///
    /// Unlike `preview_deposit_to_shares` (which uses floor), this function uses
    /// ceiling division to match the actual `withdraw` behavior (ceil burn).
    /// This ensures frontends can accurately display expected share burn before
    /// a user submits a withdrawal transaction.
    ///
    /// NOTE: In partial liquidity scenarios (when Blend returns less than requested),
    /// the actual shares burned may differ from this preview. This preview always
    /// assumes full liquidity is available.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `assets` - The amount of USDC to withdraw (7 decimal places).
    ///
    /// # Returns
    ///
    /// The number of shares that would be burned:
    /// `ceil(assets * total_shares / total_assets)`. Always at least `1` when
    /// `assets > 0`, which is what makes dust withdrawals non-free.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If the intermediate `assets * total_shares` product overflows `i128`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Show the user the share cost of a withdrawal before they confirm.
    /// let shares_burned = vault_client.preview_withdraw(&amount);
    /// assert!(shares_burned <= vault_client.get_shares(&user));
    ///
    /// // Ceil burn: preview_withdraw is never smaller than the floor-rounded
    /// // conversion of the same amount.
    /// assert!(shares_burned >= vault_client.convert_to_shares(&amount));
    /// ```
    pub fn preview_withdraw(env: Env, assets: i128) -> i128 {
        Self::require_initialized(&env);
        Self::enforce_global_rate_limit(&env, RATE_LIMIT_PREVIEW);
        Self::convert_to_shares_internal_ceil(&env, assets)
    }

    /// Converts a USDC amount to shares at the current share price.
    ///
    /// The ERC-4626 `convertToShares` equivalent: an idealised, fee-free
    /// conversion for accounting and display. It is identical to
    /// [`preview_deposit_to_shares`](crate::NeuroWealthVault::preview_deposit_to_shares) in this vault because deposits carry
    /// no fee, and both round **down**. When previewing a *withdrawal*, use
    /// [`preview_withdraw`](crate::NeuroWealthVault::preview_withdraw) instead — that path rounds up.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `assets` - The USDC amount in raw units (7 decimals).
    ///
    /// # Returns
    ///
    /// `floor(assets * total_shares / total_assets)`, or `assets` in the
    /// bootstrap case where `total_shares == 0` or `total_assets == 0`.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If the intermediate `assets * total_shares` product overflows `i128`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Round-tripping is lossy in the vault's favour, never the user's.
    /// let shares = vault_client.convert_to_shares(&assets);
    /// assert!(vault_client.convert_to_assets(&shares) <= assets);
    /// ```
    pub fn convert_to_shares(env: Env, assets: i128) -> i128 {
        Self::require_initialized(&env);
        Self::enforce_global_rate_limit(&env, RATE_LIMIT_PREVIEW);
        Self::convert_to_shares_internal(&env, assets)
    }

    /// Converts a share amount to USDC at the current share price.
    ///
    /// The ERC-4626 `convertToAssets` equivalent, identical to
    /// [`preview_shares_to_assets`](crate::NeuroWealthVault::preview_shares_to_assets) in this vault. Rounds **down**.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    /// * `shares` - The number of vault shares.
    ///
    /// # Returns
    ///
    /// `floor(shares * total_assets / total_shares)` in USDC raw units
    /// (7 decimals), or `0` when the vault holds no shares or no assets.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If the intermediate `shares * total_assets` product overflows `i128`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Value the whole supply: equals total_assets up to floor rounding.
    /// let all = vault_client.convert_to_assets(&vault_client.get_total_shares());
    /// assert!(all <= vault_client.get_total_assets());
    /// ```
    pub fn convert_to_assets(env: Env, shares: i128) -> i128 {
        Self::require_initialized(&env);
        Self::enforce_global_rate_limit(&env, RATE_LIMIT_PREVIEW);
        Self::convert_to_assets_internal(&env, shares)
    }

    /// Returns the authorized AI agent address.
    ///
    /// This is the only address that can call rebalance() to move funds
    /// between yield strategies.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the agent's Address.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_agent(env: Env) -> Address {
        Self::require_initialized(&env);
        env.storage().instance().get(&DataKey::Agent).unwrap()
    }

    /// Returns the contract owner address.
    ///
    /// The owner can pause/unpause the vault, set limits, and upgrade the contract.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the owner's Address.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_owner(env: Env) -> Address {
        Self::require_initialized(&env);
        env.storage().instance().get(&DataKey::Owner).unwrap()
    }

    /// Returns whether the vault is currently paused.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns true if paused, false otherwise.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn is_paused(env: Env) -> bool {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Returns the contract version.
    ///
    /// Used to track upgrades and ensure compatibility with external systems.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the current contract version (u32).
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_version(env: Env) -> u32 {
        Self::require_initialized(&env);
        env.storage().instance().get(&DataKey::Version).unwrap_or(1)
    }

    /// Returns the USDC token address.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the USDC token contract address.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_usdc_token(env: Env) -> Address {
        Self::require_initialized(&env);
        env.storage().instance().get(&DataKey::UsdcToken).unwrap()
    }

    /// Returns the current protocol where funds are deployed.
    ///
    /// This getter enables tests to verify storage state changes after rebalance()
    /// instead of relying solely on event assertions.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the current protocol symbol (e.g., "blend", "none").
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_current_protocol(env: Env) -> Symbol {
        Self::require_initialized(&env);
        env.storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"))
    }

    /// Returns the Blend pool contract address, if configured.
    ///
    /// This getter enables tests to verify storage state changes for the Blend
    /// pool configuration.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the Blend pool contract address, or None if not configured.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// None.
    pub fn get_blend_pool(env: Env) -> Option<Address> {
        Self::require_initialized(&env);
        env.storage().instance().get(&DataKey::BlendPool)
    }

    /// Returns the DEX liquidity pool contract address, if configured.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the DEX pool contract address, or None if not configured.
    pub fn get_dex_pool(env: Env) -> Option<Address> {
        Self::require_initialized(&env);
        env.storage().instance().get(&DataKey::DexPool)
    }

    /// Returns the shared ledger TTL used when approving Blend token spend.
    pub fn get_blend_approval_ttl(env: Env) -> u32 {
        Self::require_initialized(&env);
        Self::get_approval_ttl_internal(&env)
    }

    /// Returns the current exchange rate: assets per share, scaled by `EXCHANGE_RATE_SCALAR`.
    ///
    /// ## Formula
    ///
    /// ```text
    /// exchange_rate = (total_assets * EXCHANGE_RATE_SCALAR) / total_shares
    /// ```
    ///
    /// Where `EXCHANGE_RATE_SCALAR = 10_000_000` (7 decimal places, matching USDC
    /// precision on Stellar).
    ///
    /// ### Bootstrap / Empty-vault case
    ///
    /// When `total_shares == 0` or `total_assets == 0` (i.e. the vault has never
    /// had a deposit, or all funds have been withdrawn), the function returns
    /// `EXCHANGE_RATE_SCALAR` (i.e. `1.0000000`), representing parity between one
    /// share and one asset unit.  This prevents a division-by-zero panic and gives
    /// external callers a well-defined initial price.
    ///
    /// ### Rounding
    ///
    /// Integer division truncates toward zero (floor rounding).  The result is
    /// therefore always <= the true rational value, which is the conservative
    /// direction for a vault: it never over-reports the share price.
    ///
    /// ### Interpretation
    ///
    /// A return value of `10_500_000` means each share is currently worth
    /// `1.05` USDC (5% yield accrued since inception).
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// Returns the exchange rate as a scaled `i128`. Divide by `10_000_000` (7 decimal
    /// places) to obtain the human-readable assets-per-share ratio.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - If the vault has not been initialized yet.
    ///
    /// ## Example (off-chain pseudo-code)
    /// ```text
    /// let rate = vault.get_exchange_rate();           // e.g. 10_500_000
    /// let human_rate = rate as f64 / 10_000_000.0;   // → 1.05
    /// let user_assets = user_shares as f64 * human_rate;
    /// ```
    pub fn get_exchange_rate(env: Env) -> i128 {
        Self::require_initialized(&env);

        /// Scalar used to preserve 7 decimal places of precision in the
        /// integer result (matches USDC's 7-decimal precision on Stellar).
        const EXCHANGE_RATE_SCALAR: i128 = 10_000_000;

        let total_shares = Self::get_total_shares_internal(&env);
        let total_assets = Self::get_total_assets_internal(&env);

        // Bootstrap / empty-vault: return 1:1 parity (no division-by-zero).
        if total_shares == 0 || total_assets == 0 {
            return EXCHANGE_RATE_SCALAR;
        }

        // exchange_rate = (total_assets * SCALAR) / total_shares
        // Floor-rounded (conservative – never over-reports share price).
        total_assets
            .checked_mul(EXCHANGE_RATE_SCALAR)
            .expect("vault: exchange rate product overflow")
            .checked_div(total_shares)
            .expect("vault: exchange rate div overflow")
    }

    /// Returns the vault's idle USDC balance (funds sitting in the vault, not deployed).
    ///
    /// Idle funds are USDC held directly by the vault contract that have not yet
    /// been deployed to an external yield protocol via `rebalance()`. This value
    /// reflects the vault's on-chain token balance and decreases when the agent
    /// deploys funds (e.g., to Blend) and increases after protocol withdrawals.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// The idle USDC balance in raw units (7 decimals), read live from the USDC
    /// token contract.
    ///
    /// This is the token balance, not an accounting figure: it is not the same
    /// as `get_total_assets() - get_deployed_assets()`, and it is the amount a
    /// withdrawal can be served from without exiting a protocol position.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If the USDC token contract traps on the `balance` call.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Can this withdrawal be served without touching the protocol position?
    /// let instant = vault_client.get_idle_balance() >= amount;
    /// ```
    pub fn get_idle_balance(env: Env) -> i128 {
        Self::require_initialized(&env);
        let usdc: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        token::Client::new(&env, &usdc).balance(&env.current_contract_address())
    }

    /// Returns the amount of USDC currently deployed to an external yield protocol.
    ///
    /// Deployed assets are funds that have been supplied to an external protocol
    /// (e.g., Blend, DEX) via `rebalance()`. When `CurrentProtocol` is `"none"`,
    /// no funds are deployed and this function returns `0`. The value is queried
    /// live from the protocol's `balance` entrypoint.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// The deployed USDC amount in raw units (7 decimals), queried live from
    /// the active protocol's `balance` entrypoint.
    ///
    /// Returns `0` when [`DataKey::CurrentProtocol`] is `"none"`, when it names
    /// a protocol the vault does not integrate, or when the matching pool
    /// address ([`DataKey::BlendPool`] / [`DataKey::DexPool`]) is unset — an
    /// unconfigured pool reads as zero rather than panicking.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If the configured pool contract traps on the cross-contract `balance`
    ///   call. Because this is a cross-contract read, the call also costs more
    ///   than a plain storage getter.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Yield to date, ignoring rounding, once funds are deployed.
    /// let deployed = vault_client.get_deployed_assets();
    /// let idle = vault_client.get_idle_balance();
    /// let unreported = (idle + deployed) - vault_client.get_total_assets();
    /// ```
    pub fn get_deployed_assets(env: Env) -> i128 {
        Self::require_initialized(&env);
        let protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));
        Self::get_protocol_balance(&env, &protocol)
    }

    /// Returns the vault's asset breakdown as `(idle, deployed)`.
    ///
    /// Combines [`get_idle_balance`](crate::NeuroWealthVault::get_idle_balance) and [`get_deployed_assets`](crate::NeuroWealthVault::get_deployed_assets) into
    /// a single call for convenience. Useful for dashboards and AI agents that need
    /// both values atomically in one RPC round-trip.
    ///
    /// - `idle`:     USDC held directly by the vault contract (not in any protocol).
    /// - `deployed`: USDC currently supplied to an external yield protocol.
    ///
    /// # Arguments
    ///
    /// * `env` - The Soroban environment.
    ///
    /// # Returns
    ///
    /// `(idle, deployed)`, both in USDC raw units (7 decimals). The two values
    /// are read within one invocation, so they are consistent with each other —
    /// unlike calling the two getters separately, which can straddle a
    /// rebalance.
    ///
    /// Their sum is the vault's economic position and need not equal
    /// [`get_total_assets`](crate::NeuroWealthVault::get_total_assets), which only moves when the agent reports it
    /// via `update_total_assets`. A persistent gap means unreported yield.
    ///
    /// # Events
    ///
    /// None.
    ///
    /// # Errors
    ///
    /// None.
    ///
    /// # Panics
    ///
    /// - [`VaultError::NotInitialized`] if the vault has not been initialized.
    /// - If the USDC token or the configured pool contract traps on its
    ///   `balance` call.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let (idle, deployed) = vault_client.get_asset_breakdown();
    /// let deployed_pct = if idle + deployed == 0 {
    ///     0
    /// } else {
    ///     deployed * 100 / (idle + deployed)
    /// };
    /// ```
    pub fn get_asset_breakdown(env: Env) -> (i128, i128) {
        Self::require_initialized(&env);
        let usdc: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let idle = token::Client::new(&env, &usdc).balance(&env.current_contract_address());
        let protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));
        let deployed = Self::get_protocol_balance(&env, &protocol);
        (idle, deployed)
    }

    // ==========================================================================
    // INTERNAL HELPERS
    // ==========================================================================

    /// Writes the default rate-limit policy during initialization.
    ///
    /// These defaults are intentionally generous enough for normal batching
    /// and preview composition while still placing an on-chain ceiling on
    /// high-frequency activity. The owner can tighten or disable every bucket
    /// with `set_rate_limit`.
    #[inline]
    fn initialize_rate_limit_defaults(env: &Env) {
        env.storage().instance().set(
            &DataKey::RateLimitConfig(RATE_LIMIT_DEPOSIT),
            &RateLimitConfig {
                max_calls: DEFAULT_DEPOSIT_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_DEPOSIT_RATE_LIMIT_WINDOW,
            },
        );
        env.storage().instance().set(
            &DataKey::RateLimitConfig(RATE_LIMIT_WITHDRAW),
            &RateLimitConfig {
                max_calls: DEFAULT_WITHDRAW_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_WITHDRAW_RATE_LIMIT_WINDOW,
            },
        );
        env.storage().instance().set(
            &DataKey::RateLimitConfig(RATE_LIMIT_REBALANCE),
            &RateLimitConfig {
                max_calls: DEFAULT_REBALANCE_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_REBALANCE_RATE_LIMIT_WINDOW,
            },
        );
        env.storage().instance().set(
            &DataKey::RateLimitConfig(RATE_LIMIT_TOUCH_TTL),
            &RateLimitConfig {
                max_calls: DEFAULT_TOUCH_TTL_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_TOUCH_TTL_RATE_LIMIT_WINDOW,
            },
        );
        env.storage().instance().set(
            &DataKey::RateLimitConfig(RATE_LIMIT_PREVIEW),
            &RateLimitConfig {
                max_calls: DEFAULT_PREVIEW_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_PREVIEW_RATE_LIMIT_WINDOW,
            },
        );
        env.storage().instance().set(
            &DataKey::RateLimitConfig(RATE_LIMIT_BATCH_DEPOSIT),
            &RateLimitConfig {
                max_calls: DEFAULT_BATCH_DEPOSIT_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_BATCH_DEPOSIT_RATE_LIMIT_WINDOW,
            },
        );
        env.storage()
            .instance()
            .set(&DataKey::MaxBatchSize, &DEFAULT_MAX_BATCH_SIZE);
    }

    /// Returns whether `category` is one of the supported rate-limit buckets.
    #[inline]
    fn is_valid_rate_limit_category(category: &Symbol) -> bool {
        category == &RATE_LIMIT_DEPOSIT
            || category == &RATE_LIMIT_WITHDRAW
            || category == &RATE_LIMIT_REBALANCE
            || category == &RATE_LIMIT_TOUCH_TTL
            || category == &RATE_LIMIT_PREVIEW
            || category == &RATE_LIMIT_BATCH_DEPOSIT
    }

    /// Rejects unknown category symbols before they can create arbitrary
    /// storage keys. This bounds the policy surface and prevents a caller from
    /// accidentally configuring a bucket that no entrypoint consumes.
    #[inline]
    fn require_valid_rate_limit_category(env: &Env, category: &Symbol) {
        Self::require(
            env,
            Self::is_valid_rate_limit_category(category),
            VaultError::InvalidRateLimitCategory,
        );
    }

    /// Returns the deployment default for a validated category.
    #[inline]
    fn default_rate_limit_config(category: &Symbol) -> RateLimitConfig {
        if category == &RATE_LIMIT_DEPOSIT {
            RateLimitConfig {
                max_calls: DEFAULT_DEPOSIT_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_DEPOSIT_RATE_LIMIT_WINDOW,
            }
        } else if category == &RATE_LIMIT_WITHDRAW {
            RateLimitConfig {
                max_calls: DEFAULT_WITHDRAW_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_WITHDRAW_RATE_LIMIT_WINDOW,
            }
        } else if category == &RATE_LIMIT_REBALANCE {
            RateLimitConfig {
                max_calls: DEFAULT_REBALANCE_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_REBALANCE_RATE_LIMIT_WINDOW,
            }
        } else if category == &RATE_LIMIT_TOUCH_TTL {
            RateLimitConfig {
                max_calls: DEFAULT_TOUCH_TTL_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_TOUCH_TTL_RATE_LIMIT_WINDOW,
            }
        } else if category == &RATE_LIMIT_PREVIEW {
            RateLimitConfig {
                max_calls: DEFAULT_PREVIEW_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_PREVIEW_RATE_LIMIT_WINDOW,
            }
        } else {
            RateLimitConfig {
                max_calls: DEFAULT_BATCH_DEPOSIT_RATE_LIMIT_MAX_CALLS,
                window_ledgers: DEFAULT_BATCH_DEPOSIT_RATE_LIMIT_WINDOW,
            }
        }
    }

    /// Reads one rate-limit configuration with a legacy-deployment fallback.
    #[inline]
    fn get_rate_limit_config_internal(env: &Env, category: &Symbol) -> RateLimitConfig {
        Self::require_valid_rate_limit_category(env, category);
        env.storage()
            .instance()
            .get(&DataKey::RateLimitConfig(category.clone()))
            .unwrap_or_else(|| Self::default_rate_limit_config(category))
    }

    /// Reads a bucket without writing it. If its window has elapsed, returns a
    /// fresh empty bucket; the next accepted call persists that reset.
    #[inline]
    fn read_rate_limit_state(
        env: &Env,
        category: &Symbol,
        user: Option<&Address>,
    ) -> RateLimitState {
        let config = Self::get_rate_limit_config_internal(env, category);
        if config.max_calls == 0 {
            return RateLimitState {
                window_start: 0,
                calls: 0,
            };
        }

        let state: Option<RateLimitState> = if let Some(user) = user {
            env.storage()
                .instance()
                .get(&DataKey::RateLimitUserState(user.clone(), category.clone()))
        } else {
            env.storage()
                .instance()
                .get(&DataKey::RateLimitGlobalState(category.clone()))
        };
        let current_ledger = env.ledger().sequence();
        match state {
            Some(state)
                if current_ledger.saturating_sub(state.window_start) < config.window_ledgers =>
            {
                state
            }
            Some(_) => RateLimitState {
                window_start: current_ledger,
                calls: 0,
            },
            None => RateLimitState {
                window_start: 0,
                calls: 0,
            },
        }
    }

    /// Checks and records one accepted call in a fixed-window bucket.
    ///
    /// The configuration is read once and the bucket is read once, then the
    /// bucket is written once on success. User and global keys are both kept in
    /// instance storage: this avoids TTL-expiry reset bypasses and means a
    /// window reset overwrites an existing key rather than creating history.
    #[inline]
    fn enforce_rate_limit(env: &Env, category: Symbol, user: Option<&Address>) {
        let config = Self::get_rate_limit_config_internal(env, &category);
        if config.max_calls == 0 {
            return;
        }

        let key = if let Some(user) = user {
            DataKey::RateLimitUserState(user.clone(), category.clone())
        } else {
            DataKey::RateLimitGlobalState(category.clone())
        };
        let current_ledger = env.ledger().sequence();
        let stored_state: Option<RateLimitState> = env.storage().instance().get(&key);
        let mut state = match stored_state {
            Some(state)
                if current_ledger.saturating_sub(state.window_start) < config.window_ledgers =>
            {
                state
            }
            _ => RateLimitState {
                window_start: current_ledger,
                calls: 0,
            },
        };

        if state.calls >= config.max_calls {
            env.events().publish(
                (TOPIC_RATE_LIMIT_HIT,),
                RateLimitExceededEvent {
                    category,
                    user: user.cloned(),
                    current_ledger,
                    window_start: state.window_start,
                    max_calls: config.max_calls,
                    calls: state.calls,
                },
            );
            panic_with_error!(env, VaultError::RateLimitExceeded);
        }

        state.calls = state.calls.saturating_add(1);
        env.storage().instance().set(&key, &state);
    }

    /// Enforces a per-user call allowance.
    #[inline]
    fn enforce_user_rate_limit(env: &Env, user: &Address, category: Symbol) {
        Self::enforce_rate_limit(env, category, Some(user));
    }

    /// Enforces a global call allowance.
    #[inline]
    fn enforce_global_rate_limit(env: &Env, category: Symbol) {
        Self::enforce_rate_limit(env, category, None);
    }

    /// Returns the configured maximum batch size, falling back for upgraded
    /// instances that predate the key.
    #[inline]
    fn get_max_batch_size_internal(env: &Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::MaxBatchSize)
            .unwrap_or(DEFAULT_MAX_BATCH_SIZE)
    }

    /// Rejects a batch that exceeds the owner-configured entry count.
    #[inline]
    fn require_batch_size(env: &Env, entries: u32) {
        let max_entries = Self::get_max_batch_size_internal(env);
        Self::require(
            env,
            max_entries == 0 || entries <= max_entries,
            VaultError::BatchSizeExceeded,
        );
    }

    /// Validates that the vault is not paused.
    ///
    /// # Panics
    /// - If the vault is paused
    #[inline]
    fn require_not_paused(env: &Env) {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        Self::require(env, !paused, VaultError::Paused);
    }

    /// Validates that the vault has been initialized.
    ///
    /// # Panics
    /// - If the vault has not been initialized yet
    #[inline]
    fn require_initialized(env: &Env) {
        Self::require(
            env,
            env.storage().instance().has(&DataKey::Agent)
                && env.storage().instance().has(&DataKey::UsdcToken)
                && env.storage().instance().has(&DataKey::Owner),
            VaultError::NotInitialized,
        );
    }

    /// Validates that the caller is the contract owner.
    ///
    /// # Panics
    /// - If the caller is not the owner
    #[inline]
    fn require_is_owner(env: &Env) {
        Self::require_initialized(env);
        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        owner.require_auth();
    }

    /// Validates that the caller is the AI agent.
    ///
    /// # Panics
    /// - If the caller is not the agent
    #[inline]
    fn require_is_agent(env: &Env) {
        Self::require_initialized(env);
        let agent: Address = env.storage().instance().get(&DataKey::Agent).unwrap();
        agent.require_auth();
    }

    /// Validates that an amount is positive.
    ///
    /// # Panics
    /// - If amount is <= 0
    #[inline]
    fn require_positive_amount(env: &Env, amount: i128) {
        Self::require(env, amount > 0, VaultError::AmountMustBePositive);
    }

    /// Validates that a deposit meets the minimum requirement.
    ///
    /// Minimum deposit is read from storage (default 1 USDC).
    ///
    /// # Panics
    /// - If amount < minimum deposit
    #[inline]
    fn require_minimum_deposit(env: &Env, amount: i128) {
        let min_deposit: i128 = Self::get_min_deposit_internal(env);
        Self::require(env, amount >= min_deposit, VaultError::BelowMinimumDeposit);
    }

    #[inline]
    fn get_min_deposit_internal(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinDeposit)
            .unwrap_or(DEFAULT_MIN_DEPOSIT)
    }

    /// Validates that a deposit is within the maximum limit.
    ///
    /// Maximum deposit is read from storage (default 10,000 USDC).
    ///
    /// # Panics
    /// - If amount > maximum deposit
    #[inline]
    fn require_maximum_deposit(env: &Env, amount: i128) {
        let max_deposit: i128 = Self::get_max_deposit_internal(env);
        Self::require(
            env,
            amount <= max_deposit,
            VaultError::MaximumDepositExceeded,
        );
    }

    #[inline]
    fn get_max_deposit_internal(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MaxDeposit)
            .unwrap_or(DEFAULT_MAX_DEPOSIT)
    }

    #[inline]
    fn get_approval_ttl_internal(env: &Env) -> u32 {
        let _ = env.ledger().sequence();
        env.storage()
            .instance()
            .get(&DataKey::ApprovalTtl)
            .or_else(|| env.storage().instance().get(&DataKey::BlendApprovalTtl))
            .unwrap_or(DEFAULT_APPROVAL_TTL)
    }

    /// Validates that a deposit is within the user's cap.
    ///
    /// The cap is enforced against the user's current **asset value** (shares ×
    /// share price, which includes accrued yield), not just deposited principal.
    /// This makes the per-user cap a true exposure limit: once yield pushes a
    /// user's position to or above the cap, further deposits are rejected.
    ///
    /// # Panics
    /// - If user's new asset value (current assets + deposit amount) would exceed the cap
    #[inline]
    fn require_within_deposit_cap(env: &Env, user: &Address, amount: i128) {
        let cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::UserDepositCap)
            .unwrap_or(0_i128);
        if cap > 0 {
            let user_shares = Self::read_shares(env, user);
            let user_usdc = Self::convert_to_assets_internal(env, user_shares);
            let new_user_usdc = user_usdc
                .checked_add(amount)
                .expect("vault: total available overflow");
            if new_user_usdc > cap {
                panic_with_error!(env, VaultError::ExceedsUserDepositCap);
            }
        }
    }

    /// Validates that a deposit is within the TVL cap.
    ///
    /// Uses `TotalAssets` (principal + yield) so the cap reflects actual vault TVL
    /// rather than just principal. After yield accrual, `TotalAssets` can exceed
    /// `TotalDeposits`, and the cap check correctly accounts for that.
    ///
    /// # Panics
    /// - If total assets plus the new deposit would exceed the TVL cap
    #[inline]
    fn require_within_tvl_cap(env: &Env, amount: i128) {
        let cap: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TvLCap)
            .unwrap_or(0_i128);
        if cap > 0 {
            let total = Self::get_total_assets_internal(env);
            let new_total = total
                .checked_add(amount)
                .expect("vault: total available overflow");
            if new_total > cap {
                panic_with_error!(env, VaultError::ExceedsTvlCap);
            }
        }
    }

    /// Returns the current total shares in circulation.
    #[inline]
    fn get_total_shares_internal(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalShares)
            .unwrap_or(0_i128)
    }

    /// Returns the current total managed assets (principal + yield).
    ///
    /// If `TotalAssets` has not been explicitly set yet (e.g., right after
    /// upgrade from a principal-only model), this falls back to `TotalDeposits`
    /// to preserve continuity.
    #[inline]
    fn get_total_assets_internal(env: &Env) -> i128 {
        match env.storage().instance().get(&DataKey::TotalAssets) {
            Some(v) => v,
            None => env
                .storage()
                .instance()
                .get(&DataKey::TotalDeposits)
                .unwrap_or(0_i128),
        }
    }

    /// Internal helper: convert assets (USDC) to shares using current totals.
    /// Uses floor division - safe for deposits (user gets fewer shares, vault benefits).
    ///
    /// # Inflation-attack note
    /// Pricing reads the stored `TotalAssets` (see [`get_total_assets_internal`](crate::NeuroWealthVault::get_total_assets_internal)),
    /// NOT the vault's live token balance. Direct "donations" (token transfers to
    /// the vault that bypass `deposit`) therefore do not move the share price, so
    /// the classic first-depositor/donation inflation attack does not apply here.
    /// Virtual-share / dead-share offsets (common mitigations for balance-based
    /// vaults) are unnecessary as a result. The `deposit` entrypoint additionally
    /// rejects zero-share mints and enforces a minimum deposit; see [`deposit`](crate::NeuroWealthVault::deposit)
    /// for the full mitigation rationale.
    #[inline]
    fn convert_to_shares_internal(env: &Env, assets: i128) -> i128 {
        if assets == 0 {
            return 0;
        }

        let total_shares = Self::get_total_shares_internal(env);
        let total_assets = Self::get_total_assets_internal(env);

        if total_shares == 0 || total_assets == 0 {
            // Bootstrap: 1:1 mapping between assets and shares
            assets
        } else {
            assets
                .checked_mul(total_shares)
                .expect("vault: share conversion overflow")
                .checked_div(total_assets)
                .expect("vault: conversion div error")
        }
    }

    /// Internal helper: convert assets (USDC) to shares using current totals.
    /// Uses ceiling division - safe for withdrawals (user burns more shares, vault benefits).
    /// Prevents dust attacks where floor division could result in 0 shares burned.
    #[inline]
    fn convert_to_shares_internal_ceil(env: &Env, assets: i128) -> i128 {
        if assets == 0 {
            return 0;
        }

        let total_shares = Self::get_total_shares_internal(env);
        let total_assets = Self::get_total_assets_internal(env);

        if total_shares == 0 || total_assets == 0 {
            // Bootstrap: 1:1 mapping between assets and shares
            // Ceiling of assets is just assets (assets >= 1)
            assets
        } else {
            // Ceiling division: (a + b - 1) / b
            // shares = ceil(assets * total_shares / total_assets)
            let product = assets
                .checked_mul(total_shares)
                .expect("vault: conversion mul overflow");
            // total_assets >= 1 in this branch, so the subtraction cannot underflow;
            // use checked ops throughout for a consistent, explicit failure mode.
            let numerator = product
                .checked_add(
                    total_assets
                        .checked_sub(1)
                        .expect("vault: conversion sub underflow"),
                )
                .expect("vault: conversion add overflow");
            numerator
                .checked_div(total_assets)
                .expect("vault: conversion div error")
        }
    }

    /// Internal helper: convert shares to assets (USDC) using current totals.
    #[inline]
    fn convert_to_assets_internal(env: &Env, shares: i128) -> i128 {
        if shares == 0 {
            return 0;
        }

        let total_shares = Self::get_total_shares_internal(env);
        let total_assets = Self::get_total_assets_internal(env);

        if total_shares == 0 || total_assets == 0 {
            0
        } else {
            shares
                .checked_mul(total_assets)
                .expect("vault: share to asset conversion overflow")
                .checked_div(total_shares)
                .expect("vault: conversion div error")
        }
    }

    /// Updates [`DataKey::CurrentProtocol`] and emits [`ProtocolChangedEvent`] on change.
    fn set_current_protocol(env: &Env, new_protocol: Symbol) {
        let old_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        if old_protocol == new_protocol {
            return;
        }

        env.storage()
            .instance()
            .set(&DataKey::CurrentProtocol, &new_protocol);

        env.events().publish(
            (TOPIC_PROTOCOL_CHANGED,),
            ProtocolChangedEvent {
                old_protocol,
                new_protocol,
            },
        );
    }

    /// Panics when `min_out > 0` and fewer assets were received than required.
    fn require_min_out(env: &Env, actual: i128, min_out: i128, leg: &str) {
        if min_out > 0 && actual < min_out {
            let _ = leg;
            panic_with_error!(env, VaultError::MinOutNotMet);
        }
    }

    /// Internal helper: Supplies USDC to the Blend pool.
    ///
    /// This function handles the cross-contract call to Blend's supply function.
    /// It also approves the Blend pool to spend USDC from the vault before supplying.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `amount` - Amount of USDC to supply
    /// * `min_out` - Minimum amount that must be supplied (0 = no check)
    ///
    /// # Returns
    /// The amount actually supplied (may be less than requested)
    ///
    /// # Error Handling
    /// - Returns 0 if amount <= 0
    /// - Panics if Blend pool address is not configured
    /// - Emits BlendSupplyEvent with success status
    /// - Uses the shared approval TTL configuration from instance storage to set the approval expiry
    fn supply_to_blend(env: &Env, amount: i128, min_out: i128) -> i128 {
        if amount <= 0 {
            return 0;
        }

        let pool_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::BlendPool)
            .unwrap_or_else(|| panic_with_error!(env, VaultError::BlendPoolNotConfigured));

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let vault_address = env.current_contract_address();
        let approval_ledger = env
            .ledger()
            .sequence()
            .saturating_add(Self::get_approval_ttl_internal(env));

        // Prepare authorization for token approval and Blend supply
        let approval_args: Vec<Val> = vec![
            env,
            vault_address.clone().into_val(env),
            pool_address.clone().into_val(env),
            amount.into_val(env),
            approval_ledger.into_val(env),
        ];
        let submit_args: Vec<Val> = vec![
            env,
            vault_address.clone().into_val(env),
            vault_address.clone().into_val(env),
            vault_address.clone().into_val(env),
            vec![
                env,
                BlendRequest {
                    request_type: BLEND_REQUEST_TYPE_SUPPLY,
                    address: usdc_token.clone(),
                    amount,
                },
            ]
            .into_val(env),
        ];
        let transfer_from_args: Vec<Val> = vec![
            env,
            pool_address.clone().into_val(env),
            vault_address.clone().into_val(env),
            pool_address.clone().into_val(env),
            amount.into_val(env),
        ];

        // Approve Blend pool to spend USDC
        let token_client = token::Client::new(env, &usdc_token);
        env.authorize_as_current_contract(vec![
            env,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: usdc_token.clone(),
                    fn_name: Symbol::new(env, "approve"),
                    args: approval_args,
                },
                sub_invocations: vec![env],
            }),
        ]);
        token_client.approve(&vault_address, &pool_address, &amount, &approval_ledger);

        // Authorize and execute Blend supply
        env.authorize_as_current_contract(vec![
            env,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: pool_address.clone(),
                    fn_name: Symbol::new(env, "submit_with_allowance"),
                    args: submit_args.clone(),
                },
                sub_invocations: vec![
                    env,
                    InvokerContractAuthEntry::Contract(SubContractInvocation {
                        context: ContractContext {
                            contract: usdc_token.clone(),
                            fn_name: Symbol::new(env, "transfer_from"),
                            args: transfer_from_args,
                        },
                        sub_invocations: vec![env],
                    }),
                ],
            }),
        ]);

        // Call Blend supply function
        let supplied =
            BlendPoolClient::supply(env, &pool_address, &usdc_token, amount, &vault_address);

        Self::require_min_out(env, supplied, min_out, "blend supply");

        if supplied > 0 {
            Self::set_current_protocol(env, symbol_short!("blend"));
        }

        // Emit event for supply
        env.events().publish(
            (TOPIC_BLEND_SUPPLY,),
            BlendSupplyEvent {
                asset: usdc_token,
                amount_actual: supplied,
                success: supplied > 0,
            },
        );

        supplied
    }

    /// Internal helper: Withdraws USDC from the Blend pool.
    ///
    /// This function handles the cross-contract call to Blend's withdraw function.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `amount` - Amount of USDC to withdraw (0 = withdraw all)
    /// * `min_out` - Minimum amount that must be withdrawn (0 = no check)
    ///
    /// # Returns
    /// The amount actually withdrawn
    ///
    /// # Error Handling
    /// - Returns 0 if amount_to_withdraw <= 0
    /// - Panics if Blend pool address is not configured
    /// - Emits BlendWithdrawEvent with success status and actual amount received
    fn withdraw_from_blend(env: &Env, amount: i128, min_out: i128) -> i128 {
        let pool_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::BlendPool)
            .unwrap_or_else(|| panic_with_error!(env, VaultError::BlendPoolNotConfigured));

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let vault_address = env.current_contract_address();

        // Withdraw from Blend pool
        // If amount is 0, we attempt to withdraw the full balance
        let amount_to_withdraw = if amount == 0 {
            // Get the current balance in Blend
            BlendPoolClient::get_balance(env, &pool_address, &usdc_token, &vault_address)
        } else {
            amount
        };

        if amount_to_withdraw <= 0 {
            return 0;
        }

        // Call Blend withdraw function
        let withdrawn = BlendPoolClient::withdraw(
            env,
            &pool_address,
            &usdc_token,
            amount_to_withdraw,
            &vault_address,
        );

        Self::require_min_out(env, withdrawn, min_out, "blend withdraw");

        if withdrawn > 0 {
            let remaining =
                BlendPoolClient::get_balance(env, &pool_address, &usdc_token, &vault_address);
            if remaining == 0 {
                Self::set_current_protocol(env, symbol_short!("none"));
            }
        }

        // Emit event for withdrawal
        env.events().publish(
            (TOPIC_BLEND_WITHDRAW,),
            BlendWithdrawEvent {
                asset: usdc_token,
                amount_actual: withdrawn,
                success: withdrawn > 0,
            },
        );

        withdrawn
    }

    /// Internal helper: Supplies USDC to the DEX liquidity pool.
    ///
    /// Mirrors [`supply_to_blend`](crate::NeuroWealthVault::supply_to_blend): approves the pool to pull USDC, authorizes
    /// the cross-contract `add_liquidity` call (with its `transfer_from`
    /// sub-invocation), then supplies. The `min_out` floor is enforced both by
    /// forwarding it to the pool and by [`require_min_out`](crate::NeuroWealthVault::require_min_out) on the realized
    /// amount, giving slippage protection on the DEX leg.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `amount` - Amount of USDC to supply
    /// * `min_out` - Minimum amount that must be supplied (0 = no check)
    ///
    /// # Returns
    /// The amount actually supplied (may be less than requested).
    ///
    /// # Error Handling
    /// - Returns 0 if amount <= 0
    /// - Panics if the DEX pool address is not configured
    /// - Panics with `MinOutNotMet` if the realized amount is below `min_out`
    /// - Emits `DexSupplyEvent` with success status
    fn supply_to_dex(env: &Env, amount: i128, min_out: i128) -> i128 {
        if amount <= 0 {
            return 0;
        }

        let pool_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::DexPool)
            .unwrap_or_else(|| panic_with_error!(env, VaultError::DexPoolNotConfigured));

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let vault_address = env.current_contract_address();
        let approval_ledger = env
            .ledger()
            .sequence()
            .saturating_add(Self::get_approval_ttl_internal(env));

        let approval_args: Vec<Val> = vec![
            env,
            vault_address.clone().into_val(env),
            pool_address.clone().into_val(env),
            amount.into_val(env),
            approval_ledger.into_val(env),
        ];
        let add_liquidity_args: Vec<Val> = vec![
            env,
            vault_address.clone().into_val(env),
            usdc_token.clone().into_val(env),
            amount.into_val(env),
            min_out.into_val(env),
        ];
        let transfer_from_args: Vec<Val> = vec![
            env,
            pool_address.clone().into_val(env),
            vault_address.clone().into_val(env),
            pool_address.clone().into_val(env),
            amount.into_val(env),
        ];

        // Approve the DEX pool to spend USDC.
        let token_client = token::Client::new(env, &usdc_token);
        env.authorize_as_current_contract(vec![
            env,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: usdc_token.clone(),
                    fn_name: Symbol::new(env, "approve"),
                    args: approval_args,
                },
                sub_invocations: vec![env],
            }),
        ]);
        token_client.approve(&vault_address, &pool_address, &amount, &approval_ledger);

        // Authorize and execute the DEX add_liquidity (pulls USDC via transfer_from).
        env.authorize_as_current_contract(vec![
            env,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: pool_address.clone(),
                    fn_name: Symbol::new(env, "add_liquidity"),
                    args: add_liquidity_args.clone(),
                },
                sub_invocations: vec![
                    env,
                    InvokerContractAuthEntry::Contract(SubContractInvocation {
                        context: ContractContext {
                            contract: usdc_token.clone(),
                            fn_name: Symbol::new(env, "transfer_from"),
                            args: transfer_from_args,
                        },
                        sub_invocations: vec![env],
                    }),
                ],
            }),
        ]);

        let supplied = DexPoolClient::supply(
            env,
            &pool_address,
            &usdc_token,
            amount,
            min_out,
            &vault_address,
        );

        Self::require_min_out(env, supplied, min_out, "dex supply");

        if supplied > 0 {
            Self::set_current_protocol(env, symbol_short!("dex"));
        }

        env.events().publish(
            (TOPIC_DEX_SUPPLY,),
            DexSupplyEvent {
                asset: usdc_token,
                amount_actual: supplied,
                success: supplied > 0,
            },
        );

        supplied
    }

    /// Internal helper: Withdraws USDC from the DEX liquidity pool.
    ///
    /// Mirrors [`withdraw_from_blend`](crate::NeuroWealthVault::withdraw_from_blend). When `amount == 0` the full deployed
    /// position is withdrawn.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `amount` - Amount of USDC to withdraw (0 = withdraw all)
    /// * `min_out` - Minimum amount that must be withdrawn (0 = no check)
    ///
    /// # Returns
    /// The amount actually withdrawn.
    ///
    /// # Error Handling
    /// - Returns 0 if there is nothing to withdraw
    /// - Panics if the DEX pool address is not configured
    /// - Panics with `MinOutNotMet` if the realized amount is below `min_out`
    /// - Emits `DexWithdrawEvent` with success status and actual amount received
    fn withdraw_from_dex(env: &Env, amount: i128, min_out: i128) -> i128 {
        let pool_address: Address = env
            .storage()
            .instance()
            .get(&DataKey::DexPool)
            .unwrap_or_else(|| panic_with_error!(env, VaultError::DexPoolNotConfigured));

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let vault_address = env.current_contract_address();

        // If amount is 0, withdraw the full deployed position.
        let amount_to_withdraw = if amount == 0 {
            DexPoolClient::get_balance(env, &pool_address, &usdc_token, &vault_address)
        } else {
            amount
        };

        if amount_to_withdraw <= 0 {
            return 0;
        }

        let withdrawn = DexPoolClient::withdraw(
            env,
            &pool_address,
            &usdc_token,
            amount_to_withdraw,
            min_out,
            &vault_address,
        );

        Self::require_min_out(env, withdrawn, min_out, "dex withdraw");

        if withdrawn > 0 {
            let remaining =
                DexPoolClient::get_balance(env, &pool_address, &usdc_token, &vault_address);
            if remaining == 0 {
                Self::set_current_protocol(env, symbol_short!("none"));
            }
        }

        env.events().publish(
            (TOPIC_DEX_WITHDRAW,),
            DexWithdrawEvent {
                asset: usdc_token,
                amount_actual: withdrawn,
                success: withdrawn > 0,
            },
        );

        withdrawn
    }

    /// Internal helper: Withdraws from the current protocol if funds are deployed.
    ///
    /// This function checks the current protocol and withdraws funds if necessary.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `protocol` - The protocol symbol to withdraw from
    ///
    /// # Returns
    /// The amount withdrawn, or 0 if no funds were deployed to that protocol
    fn withdraw_from_protocol(env: &Env, protocol: &Symbol, min_out: i128) -> i128 {
        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        if current_protocol == *protocol && *protocol == symbol_short!("blend") {
            Self::withdraw_from_blend(env, 0, min_out)
        } else if current_protocol == *protocol && *protocol == symbol_short!("dex") {
            Self::withdraw_from_dex(env, 0, min_out)
        } else {
            0
        }
    }

    /// Internal helper: Withdraws a specific `amount` from the active protocol.
    ///
    /// Used by user-facing `withdraw`/`withdraw_all` to pull only the liquidity
    /// needed to satisfy a redemption (as opposed to [`withdraw_from_protocol`](crate::NeuroWealthVault::withdraw_from_protocol),
    /// which exits the full position). Dispatches to the protocol-specific helper.
    ///
    /// # Returns
    /// The amount actually withdrawn, or 0 if `protocol` holds no funds.
    fn withdraw_amount_from_protocol(
        env: &Env,
        protocol: &Symbol,
        amount: i128,
        min_out: i128,
    ) -> i128 {
        if *protocol == symbol_short!("blend") {
            Self::withdraw_from_blend(env, amount, min_out)
        } else if *protocol == symbol_short!("dex") {
            Self::withdraw_from_dex(env, amount, min_out)
        } else {
            0
        }
    }

    /// Internal helper: Gets the balance deployed to a specific protocol.
    ///
    /// Used to verify complete protocol exit during rebalancing.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `protocol` - The protocol symbol to check
    ///
    /// # Returns
    /// The amount deployed to the protocol, or 0 if not deployed
    fn get_protocol_balance(env: &Env, protocol: &Symbol) -> i128 {
        if *protocol == symbol_short!("blend") {
            let pool_address: Option<Address> = env.storage().instance().get(&DataKey::BlendPool);
            if let Some(pool) = pool_address {
                let usdc_token: Address =
                    env.storage().instance().get(&DataKey::UsdcToken).unwrap();
                let vault_address = env.current_contract_address();
                BlendPoolClient::get_balance(env, &pool, &usdc_token, &vault_address)
            } else {
                0
            }
        } else if *protocol == symbol_short!("dex") {
            let pool_address: Option<Address> = env.storage().instance().get(&DataKey::DexPool);
            if let Some(pool) = pool_address {
                let usdc_token: Address =
                    env.storage().instance().get(&DataKey::UsdcToken).unwrap();
                let vault_address = env.current_contract_address();
                DexPoolClient::get_balance(env, &pool, &usdc_token, &vault_address)
            } else {
                0
            }
        } else {
            0
        }
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod comprehensive_tests;
