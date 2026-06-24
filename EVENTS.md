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

Canonical topics are declared in [neurowealth-vault/contracts/vault/src/lib.rs](neurowealth-vault/contracts/vault/src/lib.rs) as `TOPIC_*` constants and should be used as the single source of truth by emit sites and tests.

## Core Events

### 1. VaultInitializedEvent
**Topic:** `"init"`

Emitted when the vault is initialized with core configuration.

```rust
pub struct VaultInitializedEvent {
    pub agent: Address,        // Authorized AI agent address
    pub usdc_token: Address,   // USDC token contract address
    pub tvl_cap: i128,        // Initial TVL cap (7 decimals)
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
**Topic:** `"rebalance"`

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
**Topic:** `"proto_chg"`

Emitted when `CurrentProtocol` storage changes (supply to Blend, full withdraw, or explicit transition to `"none"`).

```rust
pub struct ProtocolChangedEvent {
    pub old_protocol: Symbol,
    pub new_protocol: Symbol,
}
```

**Usage:**
- Indexers record explicit protocol state transitions without inferring from rebalance events alone

## Administrative Events

### 5. VaultPausedEvent
**Topic:** `"paused"`

Emitted when the vault is paused by the owner.

```rust
pub struct VaultPausedEvent {
    pub owner: Address,   // Owner who triggered the pause
}
```

### 6. VaultUnpausedEvent
**Topic:** `"unpaused"`

Emitted when the vault is unpaused by the owner.

```rust
pub struct VaultUnpausedEvent {
    pub owner: Address,   // Owner who triggered the unpause
}
```

### 7. EmergencyPausedEvent
**Topic:** `"emergency"`

Emitted when the vault is emergency paused by the agent.

```rust
pub struct EmergencyPausedEvent {
    pub owner: Address,   // Agent who triggered emergency pause
}
```

### 8. LimitsUpdatedEvent
**Topic:** `"limits"`

Emitted when per-transaction deposit limits are updated.

> [!IMPORTANT]
> **Indexer Migration Note:**
> Previously, `LimitsUpdatedEvent` was also used for TVL and User caps (via the deprecated `set_limits` function). This usage is now discouraged. Indexers should transition to monitoring `TvlCapUpdatedEvent` (`"tvl_cap"`), `UserDepositCapUpdatedEvent` (`"user_cap"`), and `CapsUpdatedEvent` (`"caps_upd"`) for all cap-related updates. The field names `min`/`max` in `LimitsUpdatedEvent` should only be interpreted as per-transaction deposit limits moving forward.

```rust
pub struct LimitsUpdatedEvent {
    pub old_min: i128,    // Previous minimum deposit limit
    pub new_min: i128,    // New minimum deposit limit
    pub old_max: i128,    // Previous maximum deposit limit
    pub new_max: i128,    // New maximum deposit limit
}
```

### 8a. TvlCapUpdatedEvent
**Topic:** `"tvl_cap"`

Emitted when the vault's total TVL cap is updated.

```rust
pub struct TvlCapUpdatedEvent {
    pub old_cap: i128,    // Previous TVL cap
    pub new_cap: i128,    // New TVL cap
}
```

### 8b. UserDepositCapUpdatedEvent
**Topic:** `"user_cap"`

Emitted when the per-user deposit cap is updated.

```rust
pub struct UserDepositCapUpdatedEvent {
    pub old_cap: i128,    // Previous per-user cap
    pub new_cap: i128,    // New per-user cap
}
```

### 8c. CapsUpdatedEvent
**Topic:** `"caps_upd"`


Emitted when user deposit and TVL caps are updated in a single transaction via `set_caps`.

```rust
pub struct CapsUpdatedEvent {
    pub old_user_cap: i128,  // Previous per-user deposit cap (7 decimals)
    pub new_user_cap: i128,  // New per-user deposit cap (7 decimals)
    pub old_tvl_cap: i128,   // Previous TVL cap (7 decimals)
    pub new_tvl_cap: i128,   // New TVL cap (7 decimals)
}
```

### 9. UserStrategyUpdatedEvent

Emitted when a user updates their on-chain strategy preference.

```rust
pub struct UserStrategyUpdatedEvent {
    pub user: Address,
    pub old_strategy: Symbol,
    pub new_strategy: Symbol,
}
```

**Topics**: `SymbolShort("user_strat")`

### 9. AgentUpdatedEvent
**Topic:** `"agent"`

Emitted when the AI agent address is updated.

```rust
pub struct AgentUpdatedEvent {
    pub old_agent: Address,  // Previous agent address
    pub new_agent: Address,  // New agent address
}
```

### 10. AssetsUpdatedEvent
**Topic:** `"assets"`

Emitted when total assets are updated (yield accrual).

```rust
pub struct AssetsUpdatedEvent {
    pub old_total: i128,   // Previous total assets
    pub new_total: i128,   // New total assets
}
```

## Ownership Transfer Events

### 11. OwnershipTransferInitiatedEvent
**Topic:** `"own_init"`

Emitted when ownership transfer is initiated.

```rust
pub struct OwnershipTransferInitiatedEvent {
    pub current_owner: Address,  // Current owner address
    pub pending_owner: Address,  // Pending owner address
}
```

### 12. OwnershipTransferredEvent
**Topic:** `"own_xfer"`

Emitted when ownership transfer is completed.

```rust
pub struct OwnershipTransferredEvent {
    pub old_owner: Address,   // Previous owner address
    pub new_owner: Address,   // New owner address
}
```

### 13. OwnershipTransferCancelledEvent
**Topic:** `"own_cncl"`

Emitted when ownership transfer is cancelled.

```rust
pub struct OwnershipTransferCancelledEvent {
    pub owner: Address,              // Current owner address
    pub cancelled_pending: Address,  // Cancelled pending owner
}
```

## Protocol Integration Events

### 14. BlendSupplyEvent
**Topic:** `"blend_sup"`

Emitted when assets are supplied to Blend protocol.

```rust
pub struct BlendSupplyEvent {
    pub asset: Address,   // Asset address (USDC)
    pub amount: i128,     // Amount supplied
    pub success: bool,    // Whether supply succeeded
}
```

### 15. BlendWithdrawEvent
**Topic:** `"blend_wd"`

Emitted when assets are withdrawn from Blend protocol.

```rust
pub struct BlendWithdrawEvent {
    pub asset: Address,           // Asset address (USDC)
    pub requested_amount: i128,   // Amount requested to withdraw
    pub amount_received: i128,    // Amount actually received
    pub success: bool,            // Whether withdrawal succeeded
}
```

### 15a. BlendPoolConfiguredEvent
**Topic:** `"blend_cfg"`

Emitted after `set_blend_pool` updates the configured Blend pool address.

```rust
pub struct BlendPoolConfiguredEvent {
    pub old_pool: Option<Address>, // Previous pool address, or None on first configuration
    pub new_pool: Address,         // Newly configured pool address
    pub owner: Address,            // Owner/admin who triggered the change
}
```

### 15b. DexSupplyEvent
**Topic:** `"dex_sup"`

Emitted when assets are supplied to a DEX liquidity pool (Issue #228).

```rust
pub struct DexSupplyEvent {
    pub asset: Address,        // Asset address (USDC)
    pub amount_actual: i128,   // Amount actually supplied (balance-delta measured)
    pub success: bool,         // Whether supply succeeded
}
```

### 15c. DexWithdrawEvent
**Topic:** `"dex_wd"`

Emitted when assets are withdrawn from a DEX liquidity pool (Issue #228).

```rust
pub struct DexWithdrawEvent {
    pub asset: Address,        // Asset address (USDC)
    pub amount_actual: i128,   // Amount actually received (balance-delta measured)
    pub success: bool,         // Whether withdrawal succeeded
}
```

### 15d. DexPoolConfiguredEvent
**Topic:** `"dex_cfg"`

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
**Topic:** `"upgraded"`

Emitted when the contract is upgraded to a new WASM implementation.

```rust
pub struct UpgradedEvent {
    pub old_version: u32,   // Previous contract version
    pub new_version: u32,   // New contract version
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
