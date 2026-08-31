# NeuroWealth Vault Events

This document provides a comprehensive reference for all events emitted by the NeuroWealth Vault contract, including their topics, payload schemas, and usage patterns.

## Event Design Philosophy

Events are emitted for all state-changing operations to enable:
- AI agent to detect deposits/withdrawals and react accordingly
- Frontend applications to track user balances in real-time
- External indexers to build transaction histories
- Security auditors to verify contract behavior

## Event Topics Convention

All events use short symbol topics (max 9 characters) for efficiency:
- Topics are prefixed with abbreviated identifiers
- Payload contains detailed event data
- Events are published from the vault contract address

Canonical topics are declared in [neurowealth-vault/contracts/vault/src/topics.rs](neurowealth-vault/contracts/vault/src/topics.rs) as `TOPIC_*` constants and should be used as the single source of truth by emit sites and tests. `lib.rs` imports them rather than redefining its own copies, so the on-chain symbols, that module, and this document cannot drift apart.

Every event below lists its `TOPIC_*` constant alongside the literal symbol. Three events publish an additional indexed `Address` as topic 1 so indexers can filter per user without scanning payloads: `DepositEvent`, `WithdrawEvent`, and `UserStrategyUpdatedEvent`.

### Field description convention

Every field in every payload below carries a one-line description matching the
NatDoc on the corresponding Rust struct in `lib.rs`. Amounts are in **USDC raw
units with 7 decimals** unless stated otherwise (1 USDC = `10_000_000`), and
`old_*` / `new_*` pairs are the values immediately before and after the state
change that triggered the event.

## Core Events

### 1. VaultInitializedEvent
**Topic:** `"init"` (`TOPIC_INIT`)

Emitted exactly once, by `initialize`, when the vault is set up with its core
configuration.

```rust
pub struct VaultInitializedEvent {
    pub owner: Address,        // Initial owner; authorized for every administrative entrypoint (pause, caps, pool configuration, upgrades, ownership transfer)
    pub agent: Address,        // Authorized AI agent; the only address allowed to call rebalance and update_total_assets
    pub usdc_token: Address,   // USDC token contract address; the only token the vault accepts
    pub tvl_cap: i128,         // TVL cap applied at initialization, in USDC raw units (7 decimals)
}
```

**Usage:**
- AI agents use this to discover vault configuration
- Frontend verifies initialization parameters
- Indexers record vault deployment details

### 2. DepositEvent
**Topics:** `("deposit", <user: Address>)`

Emitted when a user deposits USDC into the vault.

The user address is published as a second **indexed topic** so that indexers
and AI agents can filter deposit events by user without scanning full payloads.

```rust
pub struct DepositEvent {
    pub user: Address,    // Depositing user address
    pub amount: i128,     // Amount deposited (7 decimals)
    pub shares: i128,     // Number of shares minted
}
```

**Topic tuple (position → value):**
| Position | Type    | Value                  |
|----------|---------|------------------------|
| 0        | Symbol  | `"deposit"`            |
| 1        | Address | depositing user address |

**Usage:**
- AI agents detect new deposits to deploy yield strategies
- Frontend updates user balances in real-time
- Indexers filter deposit history by user via topic[1]

### 3. WithdrawEvent
**Topics:** `("withdraw", <user: Address>)`

Emitted when a user withdraws USDC from the vault (both `withdraw` and `withdraw_all`).

The user address is published as a second **indexed topic** so that indexers
and AI agents can filter withdrawal events by user without scanning full payloads.

```rust
pub struct WithdrawEvent {
    pub user: Address,    // Withdrawing user address
    pub amount: i128,     // Amount withdrawn (7 decimals)
    pub shares: i128,     // Number of shares burned
}
```

**Topic tuple (position → value):**
| Position | Type    | Value                   |
|----------|---------|-------------------------|
| 0        | Symbol  | `"withdraw"`            |
| 1        | Address | withdrawing user address |

**Usage:**
- AI agents update internal records after withdrawals
- Frontend updates user balances
- Indexers filter withdrawal history by user via topic[1]

### 4. RebalanceEvent
**Topic:** `"rebalance"` (`TOPIC_REBALANCE`)

Emitted when the AI agent rebalances funds between yield strategies.

```rust
pub struct RebalanceEvent {
    pub protocol: Symbol,         // Target protocol ("blend", "none")
    pub expected_apy: i128,       // Expected APY in basis points (850 = 8.5%)
    pub status: Symbol,           // "success", "failed", "partial", or "noop"
    pub amount_attempted: i128,   // Amount attempted to be moved
    pub amount_moved: i128,       // Amount actually moved
    pub amount_supplied: i128,    // Amount supplied into the target protocol
    pub amount_withdrawn: i128,   // Amount withdrawn from the prior protocol
}
```

**Agent / indexer notes:**
- `"noop"`: target allocation already satisfied; no supply/withdraw leg ran (e.g. rebalance to Blend with zero idle USDC while already deployed).
- `amount_supplied` captures the deployment size when moving into Blend.
- `amount_withdrawn` captures the exit size when leaving Blend.
- Prefer `ProtocolChangedEvent` for authoritative protocol transitions (see below).

**Usage:**
- AI agents track rebalancing decisions
- Frontend displays current strategy allocation
- Indexers monitor strategy changes for risk analysis

### 4a. ProtocolChangedEvent
**Topic:** `"proto_chg"` (`TOPIC_PROTOCOL_CHANGED`)

Emitted when `CurrentProtocol` storage changes (supply to Blend or a DEX pool, full withdraw, or explicit transition to `"none"`).

```rust
pub struct ProtocolChangedEvent {
    pub old_protocol: Symbol,   // Protocol the vault was deployed to before the change ("blend", "dex", or "none")
    pub new_protocol: Symbol,   // Protocol the vault is deployed to after the change ("blend", "dex", or "none")
}
```

**Usage:**
- Indexers record explicit protocol state transitions without inferring from rebalance events alone

### 4b. RebalanceFailedEvent
**Topic:** `"reb_fail"` (`TOPIC_REBALANCE_FAILED`)

Emitted when a rebalance exits a protocol but the withdrawal leg leaves a non-zero balance behind (incomplete exit).

```rust
pub struct RebalanceFailedEvent {
    pub from_protocol: Symbol,  // The protocol the vault was trying to exit
    pub reason: Symbol,         // Short reason code ("exit_fail" = incomplete withdrawal)
}
```

### 4c. HarvestEvent
**Topic:** `"harvest"` (`TOPIC_HARVEST`)

Emitted when the AI agent compounds accrued yield via `harvest()`. The
function withdraws from the active protocol and immediately re-supplies,
so the vault balance returns to the same position with compounded yield.

```rust
pub struct HarvestEvent {
    pub protocol: Symbol,       // The protocol harvested from ("blend" or "dex")
    pub amount_harvested: i128, // Amount withdrawn and re-deposited
}
```

**Usage:**
- AI agents track compounding frequency and yield accrual
- Indexers distinguish harvests from rebalances for audit trails

### 4d. EmergencyHarvestEvent
**Topic:** `"em_harv"` (`TOPIC_EMERGENCY_HARVEST`)

Emitted when the **owner** triggers an emergency harvest fallback via
`emergency_harvest()`. This is a distinct event from `HarvestEvent` so that
indexers can differentiate agent-initiated harvests from owner-initiated
emergency harvests during agent-key outages or rotations.

```rust
pub struct EmergencyHarvestEvent {
    pub protocol: Symbol,       // The protocol harvested from ("blend" or "dex")
    pub amount_harvested: i128, // Amount withdrawn and re-deposited
}
```

**Usage:**
- Indexers can alert on emergency harvests (potential agent-key issues)
- Audit trail distinguishes owner vs agent compounding actions

## Administrative Events

### 5. VaultPausedEvent
**Topic:** `"paused"` (`TOPIC_PAUSED`)

Emitted by `pause` when the vault is paused by the owner.

```rust
pub struct VaultPausedEvent {
    pub owner: Address,   // Owner address that triggered the pause, read from storage rather than the caller argument
}
```

### 6. VaultUnpausedEvent
**Topic:** `"unpaused"` (`TOPIC_UNPAUSED`)

Emitted by `unpause` when the vault is unpaused by the owner.

```rust
pub struct VaultUnpausedEvent {
    pub owner: Address,   // Owner address that triggered the unpause, read from storage rather than the caller argument
}
```

### 7. EmergencyPausedEvent
**Topic:** `"emerg"` (`TOPIC_EMERGENCY_PAUSED`)

Emitted by `emergency_pause`. Distinguished from `VaultPausedEvent` so
monitoring can alert on emergency halts specifically.

```rust
pub struct EmergencyPausedEvent {
    pub owner: Address,   // Owner address that triggered the emergency pause, read from storage rather than the caller argument
}
```

### 8. LimitsUpdatedEvent
**Topic:** `"l_upd"` (`TOPIC_LIMITS_UPDATED`)

Emitted when per-transaction deposit limits are updated.

> [!IMPORTANT]
> **Indexer Migration Note:**
> Previously, `LimitsUpdatedEvent` was also used for TVL and User caps (via the deprecated `set_limits` function). This usage is now discouraged. Indexers should transition to monitoring `TvlCapUpdatedEvent` (`"tvl_cap"`), `UserDepositCapUpdatedEvent` (`"user_cap"`), and `CapsUpdatedEvent` (`"caps_upd"`) for all cap-related updates. The field names `min`/`max` in `LimitsUpdatedEvent` should only be interpreted as per-transaction deposit limits moving forward.

```rust
pub struct LimitsUpdatedEvent {
    pub old_min: i128,    // Minimum per-transaction deposit before the change (7 decimals)
    pub new_min: i128,    // Minimum per-transaction deposit after the change (7 decimals)
    pub old_max: i128,    // Maximum per-transaction deposit before the change (7 decimals)
    pub new_max: i128,    // Maximum per-transaction deposit after the change (7 decimals)
}
```

### 8a. DepositLimitsUpdatedEvent
**Topic:** `"dep_lim"` (`TOPIC_DEPOSIT_LIMITS_UPDATED`)

Emitted when per-transaction deposit limits are updated via `set_deposit_limits`. This is the current, unambiguous replacement for the legacy `LimitsUpdatedEvent` topic above.

```rust
pub struct DepositLimitsUpdatedEvent {
    pub old_min: i128,    // Minimum per-transaction deposit before the change (7 decimals)
    pub new_min: i128,    // Minimum per-transaction deposit after the change (7 decimals)
    pub old_max: i128,    // Maximum per-transaction deposit before the change (7 decimals)
    pub new_max: i128,    // Maximum per-transaction deposit after the change (7 decimals)
}
```

### 8b. TvlCapUpdatedEvent
**Topic:** `"tvl_cap"` (`TOPIC_TVL_CAP_UPDATED`)

Emitted by `set_tvl_cap` when the vault's total TVL cap is updated.

```rust
pub struct TvlCapUpdatedEvent {
    pub old_cap: i128,    // TVL cap before the change (7 decimals)
    pub new_cap: i128,    // TVL cap after the change (7 decimals)
}
```

### 8c. UserDepositCapUpdatedEvent
**Topic:** `"user_cap"` (`TOPIC_USER_CAP_UPDATED`)

Emitted by `set_user_deposit_cap` when the per-user deposit cap is updated.

```rust
pub struct UserDepositCapUpdatedEvent {
    pub old_cap: i128,    // Per-user deposit cap before the change (7 decimals)
    pub new_cap: i128,    // Per-user deposit cap after the change (7 decimals)
}
```

### 8d. CapsUpdatedEvent
**Topic:** `"caps_upd"` (`TOPIC_CAPS_UPDATED`)

Emitted when user deposit and TVL caps are updated in a single transaction via `set_caps`.

```rust
pub struct CapsUpdatedEvent {
    pub old_user_cap: i128,  // Previous per-user deposit cap (7 decimals)
    pub new_user_cap: i128,  // New per-user deposit cap (7 decimals)
    pub old_tvl_cap: i128,   // Previous TVL cap (7 decimals)
    pub new_tvl_cap: i128,   // New TVL cap (7 decimals)
}
```


### 8e. RebalanceCooldownUpdatedEvent
**Topic:** `"reb_cd"` (`TOPIC_REBALANCE_COOLDOWN_UPDATED`)

Emitted when the minimum rebalance cooldown is updated via `set_rebalance_cooldown`.

```rust
pub struct RebalanceCooldownUpdatedEvent {
    pub old_interval: u32,   // Minimum ledgers between rebalances before the change, or 0 if disabled
    pub new_interval: u32,   // Minimum ledgers between rebalances after the change, or 0 if disabled
}
```

### 8f. ApprovalTtlUpdatedEvent
**Topic:** `"ttl_upd"` (`TOPIC_APPROVAL_TTL_UPDATED`)

Emitted when the shared Blend/DEX approval TTL is updated via `set_approval_ttl`
**or** the legacy `set_blend_approval_ttl` — both mutate the same
`DataKey::ApprovalTtl` storage slot, so indexers only need to watch this one
topic to catch every approval-TTL change.

```rust
pub struct ApprovalTtlUpdatedEvent {
    pub old_ttl: u32,   // Approval TTL in ledgers before the change
    pub new_ttl: u32,   // Approval TTL in ledgers after the change
}
```

### 8g. MaxConsecutiveFailuresUpdatedEvent
**Topic:** `"maxf_upd"` (`TOPIC_MAX_FAILURES_UPDATED`)

Emitted when the owner changes the circuit-breaker threshold via
`set_max_consecutive_failures` (Issue #591 — this setter previously mutated
state silently with no audit trail).

```rust
pub struct MaxConsecutiveFailuresUpdatedEvent {
    pub old_threshold: u32,   // Effective threshold before the change (the default if never configured)
    pub new_threshold: u32,   // Threshold after the change
}
```

### 9. AgentUpdatedEvent
**Topic:** `"agent"` (`TOPIC_AGENT_UPDATED`)

Emitted when the AI agent address is updated. Also emitted alongside `AgentUpdateConfirmedEvent` upon timelock execution for backward compatibility with legacy indexers.

```rust
pub struct AgentUpdatedEvent {
    pub old_agent: Address,  // Agent address that was authorized before the change
    pub new_agent: Address,  // Agent address authorized after the change
}
```

### 9a. AgentUpdateProposedEvent
**Topic:** `"agt_prop"` (`TOPIC_AGENT_UPDATE_PROPOSED`)

Emitted when an agent update proposal is scheduled via `update_agent` — step 1 of the two-step timelocked agent update flow.

```rust
pub struct AgentUpdateProposedEvent {
    pub old_agent: Address,       // Agent currently authorized; stays active for the whole timelock window
    pub new_agent: Address,       // Proposed agent address, activated only by confirm_agent_update
    pub effective_ledger: u32,    // Ledger sequence at which confirm_agent_update becomes callable
}
```

### 9b. AgentUpdateConfirmedEvent
**Topic:** `"agt_conf"` (`TOPIC_AGENT_UPDATE_CONFIRMED`)

Emitted when a pending agent update proposal is executed via `confirm_agent_update` after the timelock window has elapsed — step 2 of the timelocked flow.

```rust
pub struct AgentUpdateConfirmedEvent {
    pub old_agent: Address,       // Agent address that was authorized before confirmation
    pub new_agent: Address,       // Agent address now authorized to call rebalance and update_total_assets
}
```

### 9c. AgentUpdateCancelledEvent
**Topic:** `"agt_cncl"` (`TOPIC_AGENT_UPDATE_CANCELLED`)

Emitted when a pending proposed agent update is cancelled via `cancel_agent_update` before it is executed.

```rust
pub struct AgentUpdateCancelledEvent {
    pub old_agent: Address,              // Agent that stays authorized; cancelling never changes the active agent
    pub proposed_new_agent: Address,     // Proposed agent address that is now discarded
}
```

### 10. AssetsUpdatedEvent
**Topic:** `"assets"` (`TOPIC_ASSETS_UPDATED`)

Emitted by `update_total_assets` when the agent reports new total assets (yield
accrual or loss reporting). Because share price is derived from `TotalAssets`,
this is the authoritative signal that the exchange rate has moved.

```rust
pub struct AssetsUpdatedEvent {
    pub old_total: i128,   // Total managed assets before the update (7 decimals)
    pub new_total: i128,   // Total managed assets after the update (7 decimals)
}
```

### 10a. UserStrategyUpdatedEvent
**Topics:** `("usr_strat", <user: Address>)` (`TOPIC_USER_STRATEGY_UPDATED`)

Emitted by `set_user_strategy` when a user updates their investment strategy
preference. The preference is advisory: it tells the AI agent where the user
would like funds deployed, and does not by itself move assets.

The user address is published as a second **indexed topic**, so agents can
subscribe per user.

```rust
pub struct UserStrategyUpdatedEvent {
    pub user: Address,          // The user who updated their strategy
    pub old_strategy: Symbol,   // Strategy before the change: "conservative", "balanced", "growth", or "" the first time a user sets one
    pub new_strategy: Symbol,   // Strategy after the change: "conservative", "balanced", or "growth"
}
```

**Topic tuple (position → value):**
| Position | Type    | Value                        |
|----------|---------|------------------------------|
| 0        | Symbol  | `"usr_strat"`                |
| 1        | Address | user whose strategy changed  |

## Ownership Transfer Events

### 11. OwnershipTransferInitiatedEvent
**Topic:** `"own_init"` (`TOPIC_OWNERSHIP_INITIATED`)

Emitted by `transfer_ownership` — step 1 of the two-step ownership transfer.

```rust
pub struct OwnershipTransferInitiatedEvent {
    pub current_owner: Address,  // Owner that remains in control until the transfer is accepted
    pub pending_owner: Address,  // Proposed owner, which must call accept_ownership to take over
}
```

### 12. OwnershipTransferredEvent
**Topic:** `"own_xfer"` (`TOPIC_OWNERSHIP_TRANSFERRED`)

Emitted by `accept_ownership` — step 2 of the two-step ownership transfer.

```rust
pub struct OwnershipTransferredEvent {
    pub old_owner: Address,   // Owner that held control before the transfer
    pub new_owner: Address,   // Owner now authorized for administrative entrypoints
}
```

### 13. OwnershipTransferCancelledEvent
**Topic:** `"own_cncl"` (`TOPIC_OWNERSHIP_CANCELLED`)

Emitted by `cancel_ownership_transfer` when a pending transfer is discarded.

```rust
pub struct OwnershipTransferCancelledEvent {
    pub owner: Address,              // Owner that stays in control; cancelling never changes the owner
    pub cancelled_pending: Address,  // Pending owner address that was discarded
}
```

## Protocol Integration Events

### 14. BlendSupplyEvent
**Topic:** `"blend_sup"` (`TOPIC_BLEND_SUPPLY`)

Emitted when assets are supplied to Blend protocol.

```rust
pub struct BlendSupplyEvent {
    pub asset: Address,         // Asset address (USDC)
    pub amount_actual: i128,    // Actual amount transferred to Blend (may be less than requested due to pool limits)
    pub success: bool,          // Whether supply succeeded
}
```

### 15. BlendWithdrawEvent
**Topic:** `"blend_wd"` (`TOPIC_BLEND_WITHDRAW`)

Emitted when assets are withdrawn from Blend protocol.

```rust
pub struct BlendWithdrawEvent {
    pub asset: Address,         // Asset address (USDC)
    pub amount_actual: i128,    // Actual amount received from Blend (may be less than requested due to pool liquidity)
    pub success: bool,          // Whether withdrawal succeeded
}
```

### 15a. BlendPoolConfiguredEvent
**Topic:** `"blend_cfg"` (`TOPIC_BLEND_POOL_CONFIGURED`)

Emitted after `set_blend_pool` updates the configured Blend pool address.

```rust
pub struct BlendPoolConfiguredEvent {
    pub old_pool: Option<Address>, // Previous pool address, or None on first configuration
    pub new_pool: Address,         // Newly configured pool address
    pub owner: Address,            // Owner/admin who triggered the change
}
```

### 15b. DexSupplyEvent
**Topic:** `"dex_sup"` (`TOPIC_DEX_SUPPLY`)

Emitted when assets are supplied to a DEX liquidity pool (Issue #228).

```rust
pub struct DexSupplyEvent {
    pub asset: Address,        // Asset address (USDC)
    pub amount_actual: i128,   // Amount actually supplied (balance-delta measured)
    pub success: bool,         // Whether supply succeeded
}
```

### 15c. DexWithdrawEvent
**Topic:** `"dex_wd"` (`TOPIC_DEX_WITHDRAW`)

Emitted when assets are withdrawn from a DEX liquidity pool (Issue #228).

```rust
pub struct DexWithdrawEvent {
    pub asset: Address,        // Asset address (USDC)
    pub amount_actual: i128,   // Amount actually received (balance-delta measured)
    pub success: bool,         // Whether withdrawal succeeded
}
```

### 15d. DexPoolConfiguredEvent
**Topic:** `"dex_cfg"` (`TOPIC_DEX_POOL_CONFIGURED`)

Emitted after `set_dex_pool` updates the configured DEX pool address (Issue #228).

```rust
pub struct DexPoolConfiguredEvent {
    pub old_pool: Option<Address>, // Previous pool address, or None on first configuration
    pub new_pool: Address,         // Newly configured pool address
    pub owner: Address,            // Owner/admin who triggered the change
}
```

## Upgrade Events

### 16. UpgradedEvent
**Topic:** `"upgraded"` (`TOPIC_UPGRADED`)

Emitted when the contract is upgraded to a new WASM implementation.

```rust
pub struct UpgradedEvent {
    pub old_version: u32,   // Previous contract version
    pub new_version: u32,   // New contract version
}
```

Emitted by `execute_upgrade` once the upgrade timelock has elapsed (Issue #316).

### 16a. UpgradeScheduledEvent
**Topic:** `"upg_sched"` (`TOPIC_UPGRADE_SCHEDULED`)

Emitted when an upgrade is scheduled via `schedule_upgrade` — step 1 of the
two-step timelocked upgrade (Issue #316). The new WASM hash does not take effect
until `execute_upgrade` is called at or after `effective_ledger`.

```rust
pub struct UpgradeScheduledEvent {
    pub new_wasm_hash: BytesN<32>, // Hash of the WASM to activate after the timelock
    pub effective_ledger: u32,     // Ledger at which execute_upgrade becomes callable
}
```

### 16b. UpgradeCancelledEvent
**Topic:** `"upg_cncl"` (`TOPIC_UPGRADE_CANCELLED`)

Emitted when a pending upgrade is cancelled via `cancel_upgrade` before it is
executed — the recovery path against a malicious or mistaken schedule (Issue #316).

```rust
pub struct UpgradeCancelledEvent {
    pub cancelled_wasm_hash: BytesN<32>, // Hash of the WASM whose pending upgrade was cancelled
}
```

## Admin-Action Event Completeness Matrix

Issue #591 audited every privileged owner/agent setter for a corresponding
emitted event, since a mutation with no event weakens off-chain monitoring
and forensics. Generated by grepping `pub fn set_*` / timelock step functions
in [`lib.rs`](neurowealth-vault/contracts/vault/src/lib.rs) for `.publish(`
calls in their body.

| Setter                          | Has event? | Event / Topic                                                        |
| -------------------------------- | ---------- | ---------------------------------------------------------------------- |
| `pause`                          | ✅         | `VaultPausedEvent` (`paused`)                                          |
| `unpause`                        | ✅         | `VaultUnpausedEvent` (`unpaused`)                                      |
| `emergency_pause`                | ✅         | `EmergencyPausedEvent` (`em_pause`)                                    |
| `set_tvl_cap`                    | ✅         | `TvlCapUpdatedEvent` (`tvl_cap`)                                       |
| `set_user_deposit_cap`           | ✅         | `UserDepositCapUpdatedEvent` (`usr_cap`)                               |
| `set_caps`                       | ✅         | `CapsUpdatedEvent` (`caps_upd`) + both individual cap events           |
| `set_limits`                     | ✅         | `LimitsUpdatedEvent` (`limits`)                                        |
| `set_deposit_limits`             | ✅         | `DepositLimitsUpdatedEvent` (`dep_lim`)                                |
| `set_rebalance_cooldown`         | ✅         | `RebalanceCooldownUpdatedEvent` (`reb_cd`)                             |
| `set_max_consecutive_failures`   | ✅ **new** | `MaxConsecutiveFailuresUpdatedEvent` (`maxf_upd`) — added by #591      |
| `set_approval_ttl`               | ✅         | `ApprovalTtlUpdatedEvent` (`ttl_upd`)                                  |
| `set_blend_approval_ttl` (legacy)| ✅ **new** | `ApprovalTtlUpdatedEvent` (`ttl_upd`), shared with above — added by #591 |
| `set_user_strategy`              | ✅         | `UserStrategyUpdatedEvent` (`usr_strat`)                               |
| `update_agent`                   | ✅         | `AgentUpdateProposedEvent` (`agt_prop`)                                |
| `confirm_agent_update`           | ✅         | `AgentUpdatedEvent` + `AgentUpdateConfirmedEvent`                      |
| `cancel_agent_update`            | ✅         | `AgentUpdateCancelledEvent` (`agt_cncl`)                               |
| `set_blend_pool`                 | ✅         | `BlendPoolConfiguredEvent` (`blend_cfg`)                               |
| `set_dex_pool`                   | ✅         | `DexPoolConfiguredEvent` (`dex_cfg`)                                   |
| `transfer_ownership`             | ✅         | `OwnershipTransferInitiatedEvent` (`own_init`)                         |
| `accept_ownership`               | ✅         | `OwnershipTransferredEvent` (`own_xfer`)                               |
| `cancel_ownership_transfer`      | ✅         | `OwnershipTransferCancelledEvent` (`own_cncl`)                         |
| `schedule_upgrade`               | ✅         | `UpgradeScheduledEvent` (`upg_sched`)                                  |
| `execute_upgrade`                | ✅         | `UpgradedEvent` (`upgraded`)                                           |
| `cancel_upgrade`                 | ✅         | `UpgradeCancelledEvent` (`upg_cncl`)                                   |

Two gaps were found and closed in #591 (marked **new** above):
`set_max_consecutive_failures` and `set_blend_approval_ttl` mutated
contract state with zero emitted events, leaving those changes invisible to
any indexer or alerting rule watching the event stream. Both now emit —
`set_blend_approval_ttl` reuses `ApprovalTtlUpdatedEvent` rather than adding a
second event for the same storage slot, since it and `set_approval_ttl` write
the identical `DataKey::ApprovalTtl`.

Every remaining privileged setter above was already covered. Re-run this
audit whenever a new owner-only setter is added, before considering the
change complete.

## Declared but Never Emitted

Two payload structs are part of the contract's `#[contracttype]` surface — so
they appear in `contract-spec.json` and in generated client bindings — but are
never published by any code path. **Do not subscribe to them**; no topic will
ever fire.

### PauseEvent

A combined pause/unpause payload superseded by the three dedicated events above.
`pause`, `unpause`, and `emergency_pause` publish `VaultPausedEvent`,
`VaultUnpausedEvent`, and `EmergencyPausedEvent` respectively.

```rust
pub struct PauseEvent {
    pub paused: bool,     // true if the vault is now paused, false if now unpaused
    pub caller: Address,  // Address that triggered the pause/unpause transition
}
```

### InitFailedEvent

A failed `initialize` panics with a `VaultError` and the whole transaction is
reverted, so no event survives to be indexed. Observe the transaction result
code instead.

```rust
pub struct InitFailedEvent {
    pub caller: Address,  // Address that attempted the initialization
    pub reason: Symbol,   // Short reason code describing why initialization was rejected
}
```

## Non-Event Payload Types

`UserInfo` is a **return type**, not an event. It is returned by
`get_user_info` and is never published to the event log.

```rust
pub struct UserInfo {
    pub principal: i128,  // Deprecated: now the share-derived asset balance, not a separately stored principal record
    pub shares: i128,     // The user's vault share balance (proportional ownership of TotalAssets)
}
```

## Event Monitoring Guide

### For AI Agents

1. **Monitor DepositEvent**: Trigger yield deployment within 5 seconds
2. **Monitor WithdrawEvent**: Update internal position tracking
3. **Monitor RebalanceEvent**: Log strategy changes for performance tracking

### For Frontend Applications

1. **Monitor DepositEvent/WithdrawEvent**: Update UI balances in real-time
2. **Monitor Pause Events**: Disable deposit/withdraw functionality when paused
3. **Monitor RebalanceEvent**: Display current strategy to users

### For Indexers

1. **All Events**: Store complete event history for analytics
2. **Deposit/Withdraw Events**: Calculate TVL and user activity metrics
3. **Rebalance Events**: Track strategy performance over time

## Frontend Integration: Preview Functions

Frontend applications should use the preview functions to display expected conversion amounts before users submit transactions:

### `preview_deposit_to_shares(assets)`
Predicts shares minted for a deposit. Uses **floor** rounding (user may receive slightly fewer shares than exact division).

### `preview_shares_to_assets(shares)`
Predicts assets returned for a given share amount. Uses **floor** rounding.

### `preview_withdraw(assets)` *(Recommended for withdraw preview)*
Predicts shares burned for a withdrawal. Uses **ceiling** rounding to match actual `withdraw()` behavior. This is the correct function to show users how many shares will be burned before confirming a withdrawal.

**Important:** In partial liquidity scenarios (when Blend protocol returns less than requested), the actual withdrawal amount may be less than expected. The preview functions always assume full liquidity. Frontends should display: *"Amount may vary based on pool liquidity"* when the vault has funds deployed in Blend.

## Event Testing

The contract includes comprehensive tests that verify:
- Each operation emits the correct event topic
- Event payload fields contain expected values
- Event emission is consistent across different scenarios

Tests will fail if:
- Event topics change unexpectedly
- Event payload fields are modified
- Required events are not emitted

## Version Compatibility

Event schemas are versioned to ensure backward compatibility:
- Adding new fields to existing events is allowed
- Removing fields requires a major version bump
- Changing field types requires a major version bump

Current event schema version: **v1**

## Keeping This Document in Sync

Event documentation lives in two places and must agree:

1. The NatDoc on each `#[contracttype]` struct in
   [`lib.rs`](neurowealth-vault/contracts/vault/src/lib.rs) — a `# Topics`
   section naming the literal symbol and its `TOPIC_*` constant, plus a
   one-line `///` description on **every** field.
2. The corresponding section in this document.

When adding an event or a field:

- Declare the topic in
  [`topics.rs`](neurowealth-vault/contracts/vault/src/topics.rs), never inline
  at the emit site. Symbols are capped at 9 characters by `symbol_short!`.
- Document every field on both sides, including units for `i128` amounts.
- Add the event to this document in the section matching its category, keeping
  the `(TOPIC_*)` annotation next to the literal symbol.
- If the event publishes indexed topics beyond position 0, add a topic-tuple
  table like the ones under `DepositEvent` and `WithdrawEvent`.

## Rate-limit events

### `RateLimitConfigUpdatedEvent` (`rate_cfg`)

Emitted when the owner changes a fixed-window rate-limit policy with
`set_rate_limit` (or its `set_rate_limit_config` alias).

```rust
pub struct RateLimitConfigUpdatedEvent {
    pub category: Symbol,
    pub old_max_calls: u32,
    pub old_window_ledgers: u32,
    pub new_max_calls: u32,
    pub new_window_ledgers: u32,
    pub owner: Address,
}
```

### `BatchSizeLimitUpdatedEvent` (`batch_lim`)

Emitted when the owner changes the maximum number of entries accepted by
`batch_deposit`.

```rust
pub struct BatchSizeLimitUpdatedEvent {
    pub old_max_entries: u32,
    pub new_max_entries: u32,
    pub owner: Address,
}
```

### `RateLimitExceededEvent` (`rate_hit`)

Emitted immediately before a call is rejected with
`VaultError::RateLimitExceeded`.

```rust
pub struct RateLimitExceededEvent {
    pub category: Symbol,
    pub user: Option<Address>, // None for a global bucket
    pub current_ledger: u32,
    pub window_start: u32,
    pub max_calls: u32,
    pub calls: u32,
}
```

Correlate the event with the failed transaction's contract error because the
over-limit operation is reverted and event visibility for a failed transaction
depends on the ledger/RPC surface.
