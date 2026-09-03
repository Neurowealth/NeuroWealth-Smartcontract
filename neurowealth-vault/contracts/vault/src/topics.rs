//! Event topic constants for the NeuroWealth Vault contract.
//
// This module is the single source of truth for every event topic emitted
// by the vault. `lib.rs` imports these constants directly rather than
// redefining its own copies, so the on-chain symbols, this file, and
// `EVENTS.md` cannot drift apart. Symbols are limited to 9 characters
// (the `symbol_short!` limit).
//
// Most events publish a single-element topic tuple, `(TOPIC_X,)`, Three
// events additionally publish an indexed `Address` as topic 1 so indexers can
// filter per user without scanning payloads: `TOPIC_DEPOSIT`,
// `TOPIC_WITHDRAW`, and `TOPIC_USER_STRATEGY_UPDATED`.

#![warn(missing_docs)]

use soroban_sdk::{symbol_short, Symbol};

/// Topic for `VaultInitializedEvent`, published once by `initialize`.
pub const TOPIC_INIT: Symbol = symbol_short!("init");
/// Topic 0 for `DepositEvent`; topic 1 is the depositing user's `Address`.
pub const TOPIC_DEPOSIT: Symbol = symbol_short!("deposit");
/// Topic 0 for `WithdrawEvent`; topic 1 is the withdrawing user's `Address`.
///
/// Published by both `withdraw` and `withdraw_all`.
pub const TOPIC_WITHDRAW: Symbol = symbol_short!("withdraw");
/// Topic for `RebalanceEvent`, published by every `rebalance` outcome
/// (including `"noop"`).
pub const TOPIC_REBALANCE: Symbol = symbol_short!("rebalance");
/// Topic for `VaultPausedEvent`, published by `pause`.
pub const TOPIC_PAUSED: Symbol = symbol_short!("paused");
/// Topic for `VaultUnpausedEvent`, published by `unpause`.
pub const TOPIC_UNPAUSED: Symbol = symbol_short!("unpaused");
/// Topic for `EmergencyPausedEvent`, published by `emergency_pause`.
pub const TOPIC_EMERGENCY_PAUSED: Symbol = symbol_short!("emerg");
/// Topic for `TvlCapUpdatedEvent`, published by `set_tvl_cap`.
pub const TOPIC_TVL_CAP_UPDATED: Symbol = symbol_short!("tvl_cap");
/// Topic for `UserDepositCapUpdatedEvent`, published by `set_user_deposit_cap`.
pub const TOPIC_USER_CAP_UPDATED: Symbol = symbol_short!("user_cap");
/// Topic for `LimitsUpdatedEvent`, published by the deprecated `set_limits`.
///
/// Prefer `TOPIC_DEPOSIT_LIMITS_UPDATED` for new indexers.
pub const TOPIC_LIMITS_UPDATED: Symbol = symbol_short!("l_upd");
/// Topic for `DepositLimitsUpdatedEvent`, published by `set_deposit_limits`.
pub const TOPIC_DEPOSIT_LIMITS_UPDATED: Symbol = symbol_short!("dep_lim");
/// Topic for `CapsUpdatedEvent`, published by `set_caps`.
pub const TOPIC_CAPS_UPDATED: Symbol = symbol_short!("caps_upd");
/// Topic for `AgentUpdatedEvent`, published by `confirm_agent_update` alongside
/// `TOPIC_AGENT_UPDATE_CONFIRMED` for legacy indexer compatibility.
pub const TOPIC_AGENT_UPDATED: Symbol = symbol_short!("agent");
/// Topic for `OwnershipTransferInitiatedEvent`, published by `transfer_ownership`.
pub const TOPIC_OWNERSHIP_INITIATED: Symbol = symbol_short!("own_init");
/// Topic for `OwnershipTransferedEvent`, published by `accept_ownership`.
pub const TOPIC_OWNERSHIP_TRANSFERRED: Symbol = symbol_short!("own_xfer");
/// Topic for `OwnershipTransferCancelledEvent`, published by `cancel_ownership_transfer`.
pub const TOPIC_OWNERSHIP_CANCELLED: Symbol = symbol_short!("own_cncl");
/// Topic for `AssetsUpdatedEvent`, published by `update_total_assets`.
pub const TOPIC_ASSETS_UPDATED: Symbol = symbol_short!("assets");
/// Topic for `UpgradedEvent`, published by `execute_upgrade`.
pub const TOPIC_UPGRADED: Symbol = symbol_short!("upgraded");
/// Topic for `BlendSupplyEvent`, published when a rebalance supplies USDC to Blend.
pub const TOPIC_BLEND_SUPPLY: Symbol = symbol_short!("blend_sup");
/// Topic for `BlendWithdrawEvent`, published when a rebalance exits a Blend position.
pub const TOPIC_BLEND_WITHDRAW: Symbol = symbol_short!("blend_wd");
/// Topic for `BlendPoolConfiguredEvent`, published by `set_blend_pool`.
pub const TOPIC_BLEND_POOL_CONFIGURED: Symbol = symbol_short!("blend_cfg");
/// Topic for `DexSupplyEvent`, published when a rebalance adds DEX liquidity.
pub const TOPIC_DEX_SUPPLY: Symbol = symbol_short!("dex_sup");
/// Topic for `DexWithdrawEvent`, published when a rebalance removes DEX liquidity.
pub const TOPIC_DEX_WITHDRAW: Symbol = symbol_short!("dex_wd");
/// Topic for `DexPoolConfiguredEvent`, published by `set_dex_pool`.
pub const TOPIC_DEX_POOL_CONFIGURED: Symbol = symbol_short!("dex_cfg");
/// Topic for `ProtocolChangedEvent`, the authoritative signal that
/// `DataKey::CurrentProtocol` changed.
pub const TOPIC_PROTOCOL_CHANGED: Symbol = symbol_short!("proto_chg");
/// Topic 0 for `UserStrategyUpdatedEvent`; topic 1 is the user's `Address`.
pub const TOPIC_USER_STRATEGY_UPDATED: Symbol = symbol_short!("usr_strat");
/// Topic for `RebalanceFailedEvent`, published when a protocol exit leg leaves
/// a non-zero balance behind and the rebalance aborts without reverting.
pub const TOPIC_REBALANCE_FAILED: Symbol = symbol_short!("reb_fail");
/// Topic for `AgentUpdateProposedEvent`, published by `update_agent` (timelock step 1).
pub const TOPIC_AGENT_UPDATE_PROPOSED: Symbol = symbol_short!("agt_prop");
/// Topic for `AgentUpdateConfirmedEvent`, published by `confirm_agent_update` (timelock step 2).
pub const TOPIC_AGENT_UPDATE_CONFIRMED: Symbol = symbol_short!("agt_conf");
/// Topic for `AgentUpdateCancelledEvent`, published by `cancel_agent_update`.
pub const TOPIC_AGENT_UPDATE_CANCELLED: Symbol = symbol_short!("agt_cncl");
/// Topic for `UpgradeScheduledEvent`, published by `schedule_upgrade` (timelock step 1).
pub const TOPIC_UPGRADE_SCHEDULED: Symbol = symbol_short!("upg_sched");
/// Topic for `UpgradeCancelledEvent`, published by `cancel_upgrade`.
pub const TOPIC_UPGRADE_CANCELLED: Symbol = symbol_short!("upg_cncl");
/// Topic for `RebalanceCooldownUpdatedEvent`, published by `set_rebalance_cooldown`.
pub const TOPIC_REBALANCE_COOLDOWN_UPDATED: Symbol = symbol_short!("reb_cd");
/// Topic for `ApprovalTtlUpdatedEvent`, published by `set_approval_ttl`.
pub const TOPIC_APPROVAL_TTL_UPDATED: Symbol = symbol_short!("ttl_upd");
/// Topic for `HarvestEvent`, published when accrued yield is harvested and compounded.
pub const TOPIC_HARVEST: Symbol = symbol_short!("harvest");
/// Topic for `EmergencyHarvestEvent`, published when the owner triggers an
/// emergency harvest fallback during an agent-key outage or rotation.
pub const TOPIC_EMERGENCY_HARVEST: Symbol = symbol_short!("em_harv");
/// Topic 0 for `SharesMigratedEvent`; topic 1 is the migrating user's `Address`.
pub const TOPIC_MIGRATE: Symbol = symbol_short!("migrate");
/// Topic for `MigrationTargetUpdatedEvent`, published when the owner sets/updates migration target.
pub const TOPIC_MIGRATION_TARGET_UPDATED: Symbol = symbol_short!("mig_tgt");
/// Topic for `MigrationPausedEvent`, published when migration is paused/unpaused.
pub const TOPIC_MIGRATION_PAUSED: Symbol = symbol_short!("mig_pse");
/// Topic 0 for `SharesLockedEvent`; topic 1 is the user's `Address`.
pub const TOPIC_SHARES_LOCKED: Symbol = symbol_short!("lock");
/// Topic 0 for `SharesUnlockedEvent`; topic 1 is the user's `Address`.
pub const TOPIC_SHARES_UNLOCKED: Symbol = symbol_short!("unlock");
/// Topic 0 for `EmergencyWithdrawalEvent`; topic 1 is the withdrawing user's `Address`.
pub const TOPIC_EMERGENCY_WITHDRAWAL: Symbol = symbol_short!("em_wd");
/// Topic for `CircuitBreakerTriggeredEvent`.
pub const TOPIC_CIRCUIT_BREAKER_TRIGGERED: Symbol = symbol_short!("cb_trig");
/// Topic for `CircuitBreakerResetEvent`.
pub const TOPIC_CIRCUIT_BREAKER_RESET: Symbol = symbol_short!("cb_reset");
/// Topic for `InsuranceFundUpdatedEvent`.
pub const TOPIC_INSURANCE_FUND_UPDATED: Symbol = symbol_short!("ins_fund");

/// Topic for `RateLimitConfigUpdatedEvent`, published by `set_rate_limit`.
pub const TOPIC_RATE_LIMIT_CONFIG_UPDATED: Symbol = symbol_short!("rate_cfg");
/// Topic for `BatchSizeLimitUpdatedEvent`, published by `set_max_batch_size`.
pub const TOPIC_BATCH_SIZE_LIMIT_UPDATED: Symbol = symbol_short!("batch_lim");
/// Topic for `RateLimitExceededEvent`, published before a rate-limited call is rejected.
pub const TOPIC_RATE_LIMIT_HIT: Symbol = symbol_short!("rate_hit");

/// Topic for `MaxConsecutiveFailuresUpdatedEvent`, published by
/// `set_max_consecutive_failures` when the circuit-breaker threshold changes.
pub const TOPIC_MAX_FAILURES_UPDATED: Symbol = symbol_short!("maxf_upd");

// ============================================================================
// Multi-protocol adapter events (#656)
// ============================================================================

/// Topic for `ProtocolAdapterUpdatedEvent`, published by `set_protocol_adapter`
/// when the owner registers or replaces a venue adapter contract.
pub const TOPIC_PROTOCOL_ADAPTER_UPDATED: Symbol = symbol_short!("adap_cfg");
/// Topic for `ProtocolWhitelistUpdatedEvent`, published by `set_protocol_whitelisted`.
pub const TOPIC_PROTOCOL_WHITELIST_UPDATED: Symbol = symbol_short!("proto_wl");
/// Topic for `ProtocolSupplyEvent`, published when a rebalance supplies USDC to
/// a whitelisted adapter-backed protocol.
pub const TOPIC_PROTOCOL_SUPPLY: Symbol = symbol_short!("proto_sup");
/// Topic for `ProtocolWithdrawEvent`, published when USDC is withdrawn from a
/// whitelisted adapter-backed protocol.
pub const TOPIC_PROTOCOL_WITHDRAW: Symbol = symbol_short!("proto_wd");