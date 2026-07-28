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
//! ## Withdraw USDC
//! ```ignore
//! vault_client.withdraw(&user, &amount, &None);
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

pub mod topics;

use core::cmp::min;
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contract, contracterror, contractimpl, contracttype, panic_with_error, symbol_short, token,
    vec, Address, BytesN, Env, IntoVal, Symbol, Val, Vec,
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

#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// Supplied min limit is negative.
    NegativeMin = 1,
    /// Supplied max limit is negative.
    NegativeMax = 2,
    /// max must be greater than or equal to min.
    MaxLessThanMin = 3,
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
    /// Minimum deposit is below the allowed floor.
    MinimumDepositTooLow = 26,
    /// Maximum deposit is below the minimum.
    MaximumDepositBelowMinimum = 27,
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
#[allow(dead_code)]
const DEFAULT_TVL_CAP: i128 = 100_000_000_000_i128;
const DEFAULT_USER_DEPOSIT_CAP: i128 = 10_000_000_000_i128;
const DEFAULT_MIN_DEPOSIT: i128 = 1_000_000_i128;
const DEFAULT_MAX_DEPOSIT: i128 = 10_000_000_000_i128;
/// Default Blend token approval lifetime.
/// 100_000 ledgers × ~5s per ledger ≈ 5.7 days on Stellar mainnet.
pub(crate) const DEFAULT_APPROVAL_TTL: u32 = 100_000;
const MIN_APPROVAL_TTL: u32 = 1_000;
const MAX_APPROVAL_TTL: u32 = 500_000;

/// Minimum ledger delay before a proposed agent update can be confirmed (~24 h on Stellar mainnet).
/// 17,280 ledgers × ~5 s per ledger ≈ 86,400 s = 24 h.
const AGENT_TIMELOCK_LEDGERS: u32 = 17_280;

/// Number of ledgers an upgrade must wait between `schedule_upgrade` and
/// `execute_upgrade` (#316). Same 24-hour window as the agent timelock, giving
/// users and operators a recovery window to react to a malicious or mistaken
/// upgrade proposal (and to `cancel_upgrade`) before new WASM takes effect.
const UPGRADE_TIMELOCK_LEDGERS: u32 = 17_280;

/// Minimum ledgers remaining before `touch_user_ttl` extends a user's `Shares` entry.
const USER_SHARES_TTL_THRESHOLD: u32 = 100;
/// Target ledgers to extend a user's `Shares` entry to when maintaining TTL.
const USER_SHARES_TTL_EXTEND_TO: u32 = 100;
/// Default ledgers kept alive for Blend token approvals.
///
/// The approval expiration ledger is calculated as:
    /// `current_ledger_sequence + ApprovalTtl`.
const DEFAULT_BLEND_APPROVAL_TTL: u32 = 100_000;

use topics::{
    TOPIC_AGENT_UPDATE_CANCELLED, TOPIC_AGENT_UPDATE_CONFIRMED, TOPIC_AGENT_UPDATE_PROPOSED,
    TOPIC_AGENT_UPDATED, TOPIC_ASSETS_UPDATED, TOPIC_BLEND_POOL_CONFIGURED, TOPIC_BLEND_SUPPLY,
    TOPIC_BLEND_WITHDRAW, TOPIC_CAPS_UPDATED, TOPIC_DEPOSIT, TOPIC_DEPOSIT_LIMITS_UPDATED,
    TOPIC_DEX_POOL_CONFIGURED, TOPIC_DEX_SUPPLY, TOPIC_DEX_WITHDRAW, TOPIC_EMERGENCY_PAUSED,
    TOPIC_INIT, TOPIC_LIMITS_UPDATED, TOPIC_OWNERSHIP_CANCELLED, TOPIC_OWNERSHIP_INITIATED,
    TOPIC_OWNERSHIP_TRANSFERRED, TOPIC_PAUSED, TOPIC_PROTOCOL_CHANGED, TOPIC_REBALANCE,
    TOPIC_REBALANCE_FAILED, TOPIC_TVL_CAP_UPDATED, TOPIC_UNPAUSED, TOPIC_UPGRADE_CANCELLED,
    TOPIC_UPGRADE_SCHEDULED, TOPIC_UPGRADED, TOPIC_USER_CAP_UPDATED, TOPIC_USER_STRATEGY_UPDATED,
    TOPIC_WITHDRAW,
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
    pub fn deposit(env: Env, user: Address, amount: i128) {
        Self::require_initialized(&env);
        user.require_auth();

        Self::require_not_paused(&env);
        Self::require_positive_amount(&env, amount);
        Self::require_minimum_deposit(&env, amount);
        Self::require_maximum_deposit(&env, amount);
        Self::require_within_deposit_cap(&env, &user, amount);
        Self::require_within_tvl_cap(&env, amount);

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&user, &env.current_contract_address(), &amount);

        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0_i128);
        env.storage().instance().set(
            &DataKey::TotalDeposits,
            &(total
                .checked_add(amount)
                .expect("vault: total deposits overflow")),
        );

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
        env.storage().persistent().set(
            &DataKey::Shares(user.clone()),
            &(current_shares
                .checked_add(shares_to_mint)
                .expect("vault: shares overflow")),
        );

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
        env.storage().instance().set(
            &DataKey::TotalShares,
            &(total_shares
                .checked_add(shares_to_mint)
                .expect("vault: total shares overflow")),
        );

        // Update total assets (principal + yield)
        let total_assets = Self::get_total_assets_internal(&env);
        env.storage().instance().set(
            &DataKey::TotalAssets,
            &(total_assets
                .checked_add(amount)
                .expect("vault: total assets overflow")),
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
    /// - If shares to mint rounds down to zero.
    pub fn batch_deposit(env: Env, user: Address, entries: Vec<(Address, i128)>) {
        Self::require_initialized(&env);
        user.require_auth();
        Self::require_not_paused(&env);

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let total_entries = entries.len();

        // First pass: validate every entry before any transfer (fail-fast).
        let mut total_amount: i128 = 0;
        for i in 0..total_entries {
            let (token, amount) = entries.get(i).unwrap();
            // Until multi-asset is enabled, require all entries to use USDC.
            if token != usdc_token {
                panic!("batch_deposit: token {} is not supported; only USDC is accepted", token);
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

        let total_assets = Self::get_total_assets_internal(&env);
        for i in 0..total_entries {
            let (token, amount) = entries.get(i).unwrap();
            env.events().publish(
                (TOPIC_DEPOSIT, user.clone()),
                DepositEvent {
                    user: user.clone(),
                    token,
                    amount,
                    shares: shares_to_mint,
                    total_assets,
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
    /// Withdraws USDC from the vault. Supports optional slippage protection
    /// via `min_amount_out`. If the actual USDC received after pool withdrawals
    /// is less than `min_amount_out`, the transaction reverts.
    ///
    /// Pass `None` for `min_amount_out` to skip the slippage check (current
    /// behavior). Pass `Some(amount)` to require at least that many stroops
    /// returned.
    pub fn withdraw(env: Env, user: Address, amount: i128, min_amount_out: Option<i128>) {
        Self::require_initialized(&env);
        user.require_auth();

        Self::require_not_paused(&env);
        Self::require_positive_amount(&env, amount);

        // Check if funds are deployed in Blend and need to be retrieved
        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);

        let min_out = min_amount_out.unwrap_or(0);

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
                    .expect("vault: math error");

                // Attempt to withdraw from the active protocol (Blend or DEX).
                // The protocol's internal min_out is passed to protect against
                // slippage during the liquidity removal itself.
                let _withdrawn =
                    Self::withdraw_amount_from_protocol(&env, &current_protocol, needed, min_out);

                // RECONCILIATION: Check actual available USDC after the withdrawal.
                // We cap the withdrawal to what the vault actually has available.
                let available_usdc = token_client.balance(&env.current_contract_address());
                actual_to_return = min(amount, available_usdc);
            }
        }

        // Slippage check: if the user specified a minimum, ensure we meet or exceed it.
        if min_out > 0 {
            Self::require(
                &env,
                actual_to_return >= min_out,
                VaultError::MinOutNotMet,
            );
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
        env.storage().persistent().set(
            &DataKey::Shares(user.clone()),
            &(user_shares
                .checked_sub(shares_to_burn)
                .expect("vault: shares underflow")),
        );

        env.storage().instance().set(
            &DataKey::TotalShares,
            &(total_shares
                .checked_sub(shares_to_burn)
                .expect("vault: total shares underflow")),
        );

        // Update total assets (principal + yield)
        env.storage().instance().set(
            &DataKey::TotalAssets,
            &(total_assets
                .checked_sub(usdc_to_return)
                .expect("vault: total assets underflow")),
        );

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
    pub fn withdraw_all(env: Env, user: Address, min_amount_out: Option<i128>) -> i128 {
        Self::require_initialized(&env);
        user.require_auth();

        Self::require_not_paused(&env);

        let min_out = min_amount_out.unwrap_or(0);

        // Check if funds are deployed in Blend and need to be retrieved
        let current_protocol: Symbol = env
            .storage()
            .instance()
            .get(&DataKey::CurrentProtocol)
            .unwrap_or(symbol_short!("none"));

        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);

        let user_shares: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Shares(user.clone()))
            .unwrap_or(0_i128);
        Self::require(&env, user_shares > 0, VaultError::NoSharesToWithdraw);

        let total_shares = Self::get_total_shares_internal(&env);
        let total_assets = Self::get_total_assets_internal(&env);
        Self::require(
            &env,
            total_shares > 0 && total_assets > 0,
            VaultError::NoAssetsToWithdraw,
        );

        // Calculate assets user is entitled to based on their shares
        let entitled_amount = Self::convert_to_assets_internal(&env, user_shares);
        let mut usdc_to_return = entitled_amount;
        let mut shares_to_burn = user_shares;

        if current_protocol == symbol_short!("blend") || current_protocol == symbol_short!("dex") {
            // Check vault's USDC balance
            let vault_balance = token_client.balance(&env.current_contract_address());

            // If vault doesn't have enough USDC, try to withdraw from the active protocol
            if vault_balance < entitled_amount {
                // Attempt to withdraw from the active protocol (Blend or DEX)
                let needed = entitled_amount
                    .checked_sub(vault_balance)
                    .expect("vault: math error");
                let _ = Self::withdraw_amount_from_protocol(&env, &current_protocol, needed, min_out);

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

        // Slippage check: if the user specified a minimum, ensure the vault
        // can satisfy it before updating state or transferring funds.
        if min_out > 0 {
            Self::require(
                &env,
                usdc_to_return >= min_out,
                VaultError::MinOutNotMet,
            );
        }

        Self::require(&env, usdc_to_return > 0, VaultError::NoAssetsToReturn);
        Self::require(&env, shares_to_burn > 0, VaultError::NoSharesToBurn);

        // Update user shares
        env.storage().persistent().set(
            &DataKey::Shares(user.clone()),
            &(user_shares
                .checked_sub(shares_to_burn)
                .expect("vault: shares underflow")),
        );

        // Update total shares
        env.storage().instance().set(
            &DataKey::TotalShares,
            &(total_shares
                .checked_sub(shares_to_burn)
                .expect("vault: total shares underflow")),
        );

        // Update total assets
        env.storage().instance().set(
            &DataKey::TotalAssets,
            &(total_assets
                .checked_sub(usdc_to_return)
                .expect("vault: total assets underflow")),
        );

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
    // CORE LIFECYCLE - REBALANCE
    // ==========================================================================

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
                    status,
                    amount_attempted,
                    amount_moved,
                    amount_supplied,
                    amount_withdrawn,
                },
            );
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
                    status,
                    amount_attempted,
                    amount_moved,
                    amount_supplied,
                    amount_withdrawn,
                },
            );
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
                    status,
                    amount_attempted,
                    amount_moved,
                    amount_supplied,
                    amount_withdrawn,
                },
            );
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

        env.storage().instance().set(&DataKey::Paused, &true);

        let owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        env.events()
            .publish((TOPIC_EMERGENCY_PAUSED,), EmergencyPausedEvent { owner });
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
    pub fn set_deposit_limits(env: Env, min: i128, max: i128) {
        Self::require_initialized(&env);
        Self::require_is_owner(&env);

        // Validate limits
        Self::require(
            &env,
            min >= DEFAULT_MIN_DEPOSIT,
            VaultError::MinimumDepositTooLow,
        );
        Self::require(&env, max >= min, VaultError::MaximumDepositBelowMinimum);

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
    /// None. The cooldown is configuration-only; read the effective value back
    /// with [`get_rebalance_cooldown`](crate::NeuroWealthVault::get_rebalance_cooldown).
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
    /// None.
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

        env.storage().instance().set(&DataKey::ApprovalTtl, &ttl);
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
        env.storage().instance().remove(&DataKey::AgentTimelockExpiry);

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
        env.storage().instance().remove(&DataKey::AgentTimelockExpiry);

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

        // Validate pool interface by probing the `balance` function (Issue #148).
        // If the address is not a valid Blend pool contract the invocation will
        // panic here, rejecting the registration before the address is stored.
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let vault_address = env.current_contract_address();
        BlendPoolClient::get_balance(&env, &pool_address, &usdc_token, &vault_address);

        let old_pool: Option<Address> = env.storage().instance().get(&DataKey::BlendPool);

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

        // Validate the pool interface by probing the `balance` function. If the
        // address is not a valid DEX pool contract the invocation panics here,
        // rejecting the registration before the address is stored.
        let usdc_token: Address = env.storage().instance().get(&DataKey::UsdcToken).unwrap();
        let vault_address = env.current_contract_address();
        DexPoolClient::get_balance(&env, &pool_address, &usdc_token, &vault_address);

        let old_pool: Option<Address> = env.storage().instance().get(&DataKey::DexPool);

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
    pub fn set_blend_approval_ttl(env: Env, owner: Address, blend_approval_ttl: u32) {
        Self::require_initialized(&env);
        owner.require_auth();
        let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
        Self::require(&env, owner == stored_owner, VaultError::CallerIsNotOwner);

        env.storage()
            .instance()
            .set(&DataKey::ApprovalTtl, &blend_approval_ttl);
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
                .expect("vault: max decrease mul overflow")
                .checked_div(10_000)
                .expect("vault: max decrease div overflow");
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
                .expect("vault: conversion mul overflow")
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
            .expect("vault: exchange rate overflow")
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
            if user_usdc
                .checked_add(amount)
                .expect("vault: cap check overflow")
                > cap
            {
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
            if total
                .checked_add(amount)
                .expect("vault: cap check overflow")
                > cap
            {
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
                .expect("vault: conversion mul overflow")
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
                .expect("vault: conversion mul overflow")
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

        let pool_address: Address = env.storage().instance().get(&DataKey::BlendPool).unwrap_or_else(|| {
            panic_with_error!(env, VaultError::BlendPoolNotConfigured);
            unreachable!()
        });

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
        let pool_address: Address = env.storage().instance().get(&DataKey::BlendPool).unwrap_or_else(|| {
            panic_with_error!(env, VaultError::BlendPoolNotConfigured);
            unreachable!()
        });

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

        let pool_address: Address = env.storage().instance().get(&DataKey::DexPool).unwrap_or_else(|| {
            panic_with_error!(env, VaultError::DexPoolNotConfigured);
            unreachable!()
        });

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
        let pool_address: Address = env.storage().instance().get(&DataKey::DexPool).unwrap_or_else(|| {
            panic_with_error!(env, VaultError::DexPoolNotConfigured);
            unreachable!()
        });

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
