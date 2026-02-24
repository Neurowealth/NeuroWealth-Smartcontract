# Architecture Documentation

This document describes the technical architecture of the NeuroWealth Vault contract, including storage layout, data structures, and integration patterns.

## Overview

The NeuroWealth Vault is a Soroban smart contract that implements a non-custodial yield vault on the Stellar blockchain. Users deposit USDC, and an AI agent automatically deploys those funds across various yield-generating protocols.

## Storage Layout

### Instance Storage

Instance storage is used for contract-wide configuration that is read frequently but changes infrequently.

| Key | Type | Description |
|-----|------|-------------|
| `Agent` | Address | Authorized AI agent that can call rebalance() |
| `UsdcToken` | Address | USDC token contract address |
| `TotalDeposits` | i128 | Total USDC held in vault (excludes deployed yield) |
| `Paused` | bool | Emergency pause state |
| `Owner` | Address | Contract owner for administrative functions |
| `TvLCap` | i128 | Maximum total value locked |
| `UserDepositCap` | i128 | Maximum deposit per user |
| `Version` | u32 | Contract version for upgrade tracking |

### Persistent Storage

Persistent storage is used for per-user data that requires efficient access.

| Key | Type | Description |
|-----|------|-------------|
| `Balance(Address)` | i128 | Individual user USDC balance |

## DataKey Structure

```rust
pub enum DataKey {
    Balance(Address),      // user -> usdc balance
    TotalDeposits,        // total USDC in vault
    Agent,                // authorized AI agent address
    UsdcToken,            // USDC token contract address
    Paused,               // emergency pause state
    Owner,                // contract owner address
    TvLCap,               // maximum TVL
    UserDepositCap,       // per-user deposit limit
    Version,              // contract version
}
```

## Share Accounting Model

### Current Implementation (Phase 1)

The vault currently uses a simple 1:1 asset accounting model:

```
1 deposited USDC = 1 vault balance unit
```

This means:
- Users receive exact balance matching their deposit
- No share tokens are minted
- Yield is tracked separately by the AI agent off-chain

**Limitations**:
- Cannot accurately track user's share of yield earned
- No proportional withdrawal during yield deployment
- Not ERC-4626 compliant

### Future Implementation (Phase 2)

Will upgrade to proper share-based accounting:

```
shares = (assets * total_shares) / total_assets
assets = (shares * total_assets) / total_shares
```

This enables:
- Proportional claim on total vault assets
- Accurate yield distribution
- ERC-4626 compliance
- Proper rebalancing during withdrawals

## Rounding Rules

### Current Implementation

- Deposits: 1:1 (no rounding)
- Withdrawals: 1:1 (no rounding)
- Minimum deposit: 1 USDC (1,000,000 in 7-decimal units)

### Future Implementation (Phase 2)

- Deposits: Round down (favor vault, protect against dust)
- Withdrawals: Round down (favor vault, protect against reentrancy)
- Share minting: Round down

## Event Schema

### DepositEvent

```rust
struct DepositEvent {
    user: Address,    // User who made the deposit
    amount: i128,     // Amount in 7-decimal USDC units
}
```

**Topics**: `SymbolShort("deposit")`

### WithdrawEvent

```rust
struct WithdrawEvent {
    user: Address,    // User who made the withdrawal
    amount: i128,     // Amount in 7-decimal USDC units
}
```

**Topics**: `SymbolShort("withdraw")`

### RebalanceEvent

```rust
struct RebalanceEvent {
    strategy: Symbol,  // Target strategy (e.g., "conservative", "balanced", "growth")
    amount: i128,     // Amount being rebalanced (0 for full rebalance)
}
```

**Topics**: `SymbolShort("rebalance")`

### PauseEvent

```rust
struct PauseEvent {
    paused: bool,    // true = paused, false = unpaused
    caller: Address, // Who triggered the pause
}
```

**Topics**: `SymbolShort("pause")`

## Cross-Contract Integration Flow

### USDC Token Integration

```
Vault Contract → USDC Token Contract (via token::Client)
                  ↑
                  ├── transfer() - receive user funds
                  └── transfer() - return funds to user
```

**Integration Points**:
1. `deposit()`: Calls `token.transfer(user, vault, amount)`
2. `withdraw()`: Calls `token.transfer(vault, user, amount)`

**Assumptions**:
- USDC uses Stellar's Soroban Token interface
- 7 decimal places
- Standard token operations (transfer, balance, etc.)

### AI Agent Integration

```
AI Agent → Vault Contract
           ├── get_balance(user) - monitor positions
           ├── get_total_deposits() - monitor TVL
           └── rebalance(strategy) - signal strategy changes
           ↓
     DepositEvent / WithdrawEvent (via Soroban events)
```

**Event Flow**:
1. User calls `deposit()` or `withdraw()`
2. Contract emits corresponding event
3. AI agent monitors events via RPC/subscription
4. Agent responds by calling `rebalance()` or adjusting off-chain state

### Blend Protocol Integration (Phase 2)

```
Vault Contract → Blend Protocol Contract
                 ↑
                 ├── lend() - deposit USDC for yield
                 ├── redeem() - withdraw from lending
                 └── get_balance() - check yield earned
```

**Future Integration**:
- Phase 2 will add direct Blend protocol interactions
- Vault will call Blend's lending functions
- Yield earned will be tracked in total assets

## Asset Flow Diagrams

### Deposit Flow

```
┌─────────┐    ┌─────────────┐    ┌──────────────┐    ┌─────────────┐
│  User   │───▶│ USDC Token  │───▶│ Vault        │───▶│ Event Emit  │
│ Wallet  │    │ (transfer)  │    │ (balance++) │    │ (deposit)   │
└─────────┘    └─────────────┘    └──────────────┘    └─────────────┘
                                                       ↓
                                              ┌─────────────┐
                                              │ AI Agent    │
                                              │ (monitors)  │
                                              └─────────────┘
```

1. User authorizes deposit transaction
2. USDC transferred from user to vault
3. User balance updated in persistent storage
4. Total deposits updated in instance storage
5. DepositEvent emitted
6. AI agent detects event, initiates yield deployment

### Withdraw Flow

```
┌─────────┐    ┌──────────────┐    ┌─────────────┐    ┌─────────────┐
│  User   │───▶│ Vault        │───▶│ Balance     │───▶│ Event Emit  │
│ Wallet  │    │ (auth check) │    │ (balance--) │    │ (withdraw)  │
└─────────┘    └──────────────┘    └─────────────┘    └─────────────┘
                    ↓                                           ↓
            ┌─────────────┐                            ┌─────────────┐
            │ USDC Token  │◀───────────────────────────│ AI Agent    │
            │ (transfer)  │                            │ (monitors)  │
            └─────────────┘                            └─────────────┘
```

1. User authorizes withdrawal transaction
2. Vault verifies user balance
3. User balance updated in persistent storage
4. Total deposits updated in instance storage
5. USDC transferred from vault to user
6. WithdrawEvent emitted
7. AI agent detects event, updates internal state

### Rebalance Flow (AI Agent)

```
┌─────────────┐    ┌──────────────┐    ┌─────────────┐    ┌─────────────┐
│ AI Agent    │───▶│ Vault        │───▶│ Auth Check  │───▶│ Event Emit  │
│ (strategy)  │    │ (rebalance)  │    │ (agent)     │    │ (rebalance) │
└─────────────┘    └──────────────┘    └─────────────┘    └─────────────┘
                                                              ↓
                                                      ┌─────────────┐
                                                      │ External    │
                                                      │ Protocols   │
                                                      │ (Blend/DEX) │
                                                      └─────────────┘
```

1. AI agent evaluates market conditions
2. Agent calls `rebalance(strategy)` on vault
3. Vault verifies caller is agent
4. RebalanceEvent emitted
5. Agent proceeds to execute strategy via external protocols

## Upgrade Model

### Storage Preservation

When upgrading the contract, the following storage keys must be preserved:

- All `Balance(Address)` entries
- `TotalDeposits`
- `Agent`
- `UsdcToken`
- `Paused`
- `Owner`
- `TvLCap`
- `UserDepositCap`
- `Version` (incremented)

### Version History

| Version | Changes |
|---------|---------|
| 1 | Initial implementation with basic deposit/withdraw |
| 2 | (Planned) ERC-4626 share accounting |
| 3 | (Planned) Blend protocol integration |
| 4 | (Planned) Multi-asset support |

### Migration Considerations

When upgrading to share-based accounting (Phase 2):

1. Snapshot all user balances
2. Mint shares 1:1 to existing balances
3. Track total shares = total deposits
4. Future deposits/withdrawals use share math

## Error Handling

### Panic Messages

| Function | Panic Condition |
|----------|----------------|
| `initialize` | "Already initialized" |
| `deposit` | "Vault is paused", "Amount must be positive", "Minimum deposit is 1 USDC", "Exceeds user deposit cap", "Exceeds TVL cap" |
| `withdraw` | "Vault is paused", "Amount must be positive", "Insufficient balance" |
| `rebalance` | "Vault is paused" |
| `pause` | (requires owner auth) |
| `unpause` | "Vault is not paused" |

### Return Values

All read functions return the requested data or 0/default if not set.

## Testing Considerations

### Unit Tests

- Deposit with valid amount
- Deposit with minimum amount (boundary)
- Deposit exceeding cap (should fail)
- Withdraw with sufficient balance
- Withdraw exceeding balance (should fail)
- Pause/unpause by owner
- Pause by non-owner (should fail)

### Integration Tests

- Full deposit → rebalance → withdraw flow
- Multiple users depositing and withdrawing
- TVL cap enforcement
- User deposit cap enforcement
- Emergency pause during active deposits

## Gas Considerations

### Instance Storage Operations

- Read: ~1-2 gas units
- Write: ~2-3 gas units
- Use for: Configuration, totals, flags

### Persistent Storage Operations

- Read: ~1 gas unit
- Write: ~1-2 gas units
- Use for: User balances

### Optimization Strategies

1. Batch reads when possible
2. Use instance storage for frequently accessed globals
3. Use persistent storage for user-specific data
4. Minimize state changes in single transaction
