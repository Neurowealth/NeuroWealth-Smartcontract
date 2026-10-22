# NeuroWealth Contract Upgrade & Storage Migration Guide

This document provides comprehensive guidelines for upgrading the NeuroWealth smart contract and managing its storage schema safely. It serves as a reference for contributors and maintainers to ensure data integrity during contract evolutions.

## 1. Overview

### 1.1 User-Initiated Vault Migration (#637)

In addition to the traditional contract upgrade process described below, the NeuroWealth vault now supports user-initiated migration of shares from an old vault to a new vault. This provides a trustless way for users to move their positions during contract upgrades without requiring owner intervention.

**Key Features:**
- Users can migrate their shares to a new vault contract set by the owner
- Share value is preserved through exchange rate conversion
- Migration can be paused independently of the main vault pause
- Comprehensive event logging for migration tracking

**Migration Flow:**
1. Owner schedules the migration target via `schedule_migration_target(owner, new_vault_address)`, waits out the timelock (~24 h, same as upgrades), then calls `execute_migration_target(owner)`
2. Users call `migrate_shares(user)` to move their positions
3. Old vault burns shares and transfers equivalent assets to new vault
4. New vault mints shares for the user based on the transferred assets

**Security Considerations:**
- Migration target must be owner-set to prevent malicious contracts
- Owner can pause migration independently as a safety measure
- Exchange rate is calculated at migration time to preserve value
- Full event logging enables audit trails

**Detailed Migration Process:**
See section 12 below for complete details on user-initiated vault migration.

In the Soroban smart contract environment, a contract upgrade involves replacing the underlying WebAssembly (WASM) code of a contract while its data (storage) remains attached to the same contract ID.

**What is preserved during an upgrade:**

- **Persistent Storage**: Data meant to outlive the transaction and remain available indefinitely (e.g., user balances, shares, config).
- **Temporary Storage**: Short-lived data, but still persists across the WASM swap until its TTL expires.
- **Instance Storage**: Contract-level global state (e.g., admin addresses, token IDs).
- **The Contract ID**: The address of the contract remains exactly the same.

**What is replaced during an upgrade:**

- **Contract Code (WASM)**: All logic, entrypoints, and type definitions are completely replaced by the new WASM binary.

**Upgrade vs. Migration:**

- **Code Upgrade**: Swapping the executable WASM file. If the storage schema (the structure of saved data) has not changed, an upgrade requires no further action.
- **Storage Migration**: Re-structuring the existing data stored on the ledger to match new type definitions in the upgraded code. This typically requires a dedicated migration entrypoint to transition old data formats to new ones.

---

## 2. Upgrade Safety Principles

Soroban storage keys (`DataKey`s) and values are heavily tied to their Rust serialized representations (XDR). Altering these types requires extreme care.

### Safe Changes

- Adding new functions or entrypoints.
- Adding new events or changing event structures (events are not state).
- Adding new `DataKey` variants at the _end_ of the enum (does not affect existing serialized variants).
- Adding optional struct fields (if standard XDR evolution rules are strictly followed and supported).

### Risky Changes

- **Renaming `DataKey` variants**: Changes the conceptual mapping but technically doesn't break XDR if the variant index and internal types are identical. However, it requires a logical migration if the underlying intent changes.
- **Reordering enum variants**: This alters the discriminant values used in serialization, breaking access to existing storage entries.
- **Changing serialized struct layouts**: Adding or reordering fields in a stored struct breaks deserialization of existing data.
- **Changing stored value types**: E.g., changing `u32` to `u64`.

### Dangerous Changes

**Example of a catastrophic change:**
Before:

```rust
DataKey::UserBalance(Address)
```

Changed to:

```rust
DataKey::Balance(Address)
```

Without a proper migration, the contract will look for `DataKey::Balance` and find nothing, effectively zeroing out all user balances, while the old `DataKey::UserBalance` data becomes permanently orphaned in storage.

---

## 3. Storage Layout Guidelines

To maintain upgrade compatibility, adhere to the following `DataKey` design patterns:

```rust
#[contracttype]
pub enum DataKey {
    Config,
    User(Address),
    Vault(Address),
    Position(u64),
}
```

**Guidelines:**

- **Use typed `DataKey` enums**: Avoid raw `Symbol` or string keys to prevent typos and namespace collisions.
- **Keep variants stable**: Once a variant is used in production, treat it as immutable.
- **Never reorder variants**: Always append new variants to the end of the `DataKey` enum.
- **Namespace logically**: Group related data logically within the enum or nested enums to avoid top-level clutter.

---

## 4. Versioning Strategy

To safely track and orchestrate migrations, the contract must maintain a storage version in its instance or persistent storage.

```rust
pub const STORAGE_VERSION: u32 = 1;
```

When a schema change occurs, increment the version:

```rust
pub const STORAGE_VERSION: u32 = 2;
```

**Versioning Rules:**

- **When to increment**: Increment the `STORAGE_VERSION` constant anytime a structural change is made to stored structs, or when `DataKey` semantics change requiring a migration script.
- **Tracking Migrations**: Store the current migrated version on-chain.
- **Upgrade Scripts**: The migration entrypoint must verify the on-chain version against the expected old version before running, preventing double-migrations.

---

## 5. When Migrations Are Required

### New Storage Key (No Migration Required)

Example: Adding `DataKey::Treasury`.
If you are simply introducing a new key and no existing data needs to be restructured, no migration script is required. The new data will be written on demand.

### Added Struct Field (Migration Required)

Before:

```rust
pub struct Vault {
    pub balance: i128,
}
```

After:

```rust
pub struct Vault {
    pub balance: i128,
    pub reward_rate: u32,
}
```

**Why:** Existing serialized `Vault` values on the ledger lack the `reward_rate` field and cannot automatically deserialize into the new struct. A migration function must read the old bytes/struct, populate the missing field with a default, and write the new struct back.

### Key Rename / Semantic Shift (Migration Required)

Before:

```rust
DataKey::Vault(id)
```

After:

```rust
DataKey::Position(id)
```

**Why:** The data lives under the old serialized key. A migration must read the data from `DataKey::Vault(id)`, write it to `DataKey::Position(id)`, and explicitly delete the old `DataKey::Vault(id)` to free up space and recover storage deposits.

---

## 6. Example Migration Workflow

**Scenario:** Introduce `DataKey::TreasuryBalance` and migrate legacy treasury values.

**Migration Entrypoint:**

```rust
pub fn migrate(env: Env) {
    // 1. Verify admin/owner auth
    env.storage().instance().get::<_, Address>(&DataKey::Admin).unwrap().require_auth();

    // 2. Check current version to prevent double execution
    let current_version: u32 = env.storage().instance().get(&DataKey::Version).unwrap_or(1);
    assert!(current_version == 1, "Migration already executed");

    // 3. Read old values
    let legacy_val: i128 = env.storage().persistent().get(&DataKey::LegacyTreasury).unwrap_or(0);

    // 4. Write new values
    env.storage().persistent().set(&DataKey::TreasuryBalance, &legacy_val);

    // 5. Clean up old storage (crucial for ledger health)
    env.storage().persistent().remove(&DataKey::LegacyTreasury);

    // 6. Update storage version
    env.storage().instance().set(&DataKey::Version, &2u32);
}
```

**Lifecycle:**

1. **Upload new WASM**: Install the compiled contract to the ledger.
2. **Upgrade contract**: Call the Soroban system upgrade functionality to swap the WASM.
3. **Invoke migration entrypoint**: Immediately call `migrate()` before unpausing the contract or allowing user interactions.
4. **Verify storage**: Check state to ensure the migration succeeded.
5. **Remove old migration code**: In a future release (v3), the `migrate` function for v1->v2 can be safely removed to save bytecode size.

---

## 6a. Migrating from the Instant `upgrade()` (Issue #316)

Before Issue #316 the vault exposed a single entrypoint:

```rust
// Removed. Applied the new WASM in one transaction, with no delay.
pub fn upgrade(env: Env, owner: Address, new_wasm_hash: BytesN<32>)
```

That entrypoint **no longer exists**. Any runbook, deploy script, multisig
template, or CI job that still calls `upgrade` will fail at invocation time with
an unknown-function error — not silently. Replace it with the two-step flow:

| Before (instant)        | After (timelocked)                                                                                 |
| ----------------------- | -------------------------------------------------------------------------------------------------- |
| `upgrade(owner, hash)`  | `schedule_upgrade(owner, hash)` → wait ≥ 17,280 ledgers → `execute_upgrade(owner)`                 |
| —                       | `cancel_upgrade(owner)` to abandon a pending proposal                                              |
| —                       | `get_pending_upgrade()` to read `(hash, effective_ledger)`                                         |
| Emitted `UpgradedEvent` | `UpgradeScheduledEvent` on schedule, `UpgradedEvent` on execute, `UpgradeCancelledEvent` on cancel |

### What operators must change

- **Split the transaction in two.** The upgrade can no longer complete inside a
  single maintenance window. Budget for a ≥ 24-hour gap between scheduling and
  execution, and make sure the signer set that schedules is still available to
  execute.
- **Do not pre-sign `execute_upgrade` at scheduling time** unless your process
  can revoke it. The delay only provides safety if someone is actually watching
  and able to call `cancel_upgrade`.
- **Assign a monitor.** Subscribe to `UpgradeScheduledEvent` (`"upg_sched"`) or
  poll `get_pending_upgrade()` for the duration of the window, and compare the
  pending hash against the WASM you intended to ship.
- **Keep the vault unpaused to schedule and execute.** Both entrypoints are
  pause-gated. `cancel_upgrade` is not, so the escape hatch remains usable
  during an incident.
- **Run `migrate()` after `execute_upgrade`, not after `schedule_upgrade`.**
  Scheduling changes no code; the storage schema is still the old one until
  execution lands.

### New failure modes to expect

| Error                    | Cause                                                                                                                              | Resolution                                                                                                          |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `TimelockAlreadyPending` | `schedule_upgrade` called while a proposal is already pending.                                                                     | `cancel_upgrade(owner)` first, then re-schedule. The 24-hour clock restarts.                                        |
| `NoTimelockPending`      | `execute_upgrade` or `cancel_upgrade` called with nothing scheduled.                                                               | Check `get_pending_upgrade()`; the proposal was already executed or cancelled.                                      |
| `TimelockNotExpired`     | `execute_upgrade` called before `UpgradeTimelockExpiry`.                                                                           | Compare `get_pending_upgrade()`'s `effective_ledger` against the current ledger sequence and retry after it passes. |
| `CallerIsNotOwner`       | The authorizing address is not the stored owner. All three entrypoints take `owner` as an argument _and_ check it against storage. | Confirm the signer matches `get_owner()`.                                                                           |
| `Paused`                 | `schedule_upgrade` or `execute_upgrade` called while the vault is paused.                                                          | Unpause first. `cancel_upgrade` is not pause-gated and stays available.                                             |

The first three errors are **shared with the agent timelock** (Issue #317)
because `#[contracterror]` caps the enum at 50 variants. When debugging, confirm
which of the two flows raised the error before assuming it was the upgrade path.

### Storage impact

`schedule_upgrade` writes two new instance keys, `DataKey::PendingUpgradeHash`
and `DataKey::UpgradeTimelockExpiry`. Both are appended `DataKey` variants, so
existing serialized entries are unaffected and **no storage migration is
required** to adopt the timelock.

Both keys are cleared by `execute_upgrade` (before the WASM swap) and by
`cancel_upgrade`. A vault that has never scheduled an upgrade has neither key,
and `get_pending_upgrade()` returns `None`.

### Emergency guidance

The timelock is a safety feature, not an obstacle to route around: there is no
bypass, and none should be added. If a hostile or mistaken upgrade is
scheduled, the response is `cancel_upgrade(owner)` within the window. If owner
keys themselves are compromised, cancelling is not sufficient — pause the
vault, transfer ownership to safe keys via `transfer_ownership` /
`accept_ownership`, and only then cancel the pending proposal.

---

## 7. Upgrade Checklist

Use this practical checklist for every upgrade.

### Before Upgrade

- [ ] All unit and integration tests passing.
- [ ] Storage migration scripts written and rigorously reviewed.
- [ ] `STORAGE_VERSION` constant bumped in code.
- [ ] Testnet deployment and migration fully validated.
- [ ] Production data backup/export completed (if applicable/possible).

### Deployment

- [ ] **Step 1: Install WASM**: Install the compiled WASM binary to the Stellar ledger and obtain its hex hash.
  - **MAINNET GATE (Release #Release#):** Before calling `schedule_upgrade` on mainnet, the WASM hash **must match** a hash published by a CI run on a signed git tag. Verify:
    1. The CI workflow ran on the intended release tag (e.g., `v2.1.0`).
    2. The CI build artifact WASM hash is recorded in the `CHANGELOG.md` under that version.
    3. The hash returned by `stellar contract install` on mainnet **byte-for-byte matches** the CI-published hash.
    4. Record the matching hash and CI job URL in the release ticket for audit trail.
    - _Rationale:_ This gate ensures the exact bytecode deployed to mainnet was built from a tagged, reviewable commit in git and is not a locally-modified or compromised build.
- [ ] **Step 2: Propose / Schedule Upgrade**: Call the `schedule_upgrade(owner, new_wasm_hash)` contract function (emits `UpgradeScheduledEvent`).
- [ ] **Step 3: Monitor Timelock**: Monitor the 24-hour mandatory delay window (17,280 ledgers) for any `UpgradeScheduledEvent` or `UpgradeCancelledEvent` anomalies.
  - If a mistake or key compromise is discovered, the owner must call `cancel_upgrade(owner)` (emits `UpgradeCancelledEvent`) immediately as an escape hatch.
- [ ] **Step 4: Execute Upgrade**: Once the timelock expires (current ledger sequence >= `UpgradeTimelockExpiry`), call `execute_upgrade(owner)` (emits `UpgradedEvent`).
- [ ] **Step 5: Run Migration**: Invoke the `migrate()` entrypoint immediately (if applicable).
- [ ] **Step 6: Verify Version**: Call `get_version()` and verify it returns the incremented version.
- [ ] **Step 7: Validate State**: Validate critical state and balances via RPC queries.

### After Deployment

- [ ] Verify Total Assets, Total Shares, and random User Balances.
- [ ] Verify Vault / Blend position accounting.
- [ ] Verify successful event emission on a small test transaction.
- [ ] Monitor RPC logs for unforeseen deserialization errors.
- [ ] Monitor network dashboards for elevated error rates.

---

## 8. Automated Verification Scripts

The repository includes scripts to verify key invariants before and after upgrades. Run these as part of your CI pipeline or manual upgrade validation.

### Balance Deprecation Check

```bash
bash scripts/check-balance-deprecation.sh
```

Verifies that the deprecated `DataKey::Balance(Address)` variant:

- Exists at discriminant 0 (preserving storage layout)
- Is documented as deprecated
- Is not used in any production code path
- All test/fuzz references use `TokenDataKey::Balance` (mock), not `DataKey::Balance`
- `get_balance` derives values from shares, not from storage

### Access Control Table Check

```bash
bash scripts/check-access-control.sh
```

Cross-references the access control table in `SECURITY.md` against `contract-spec.json` to ensure every state-changing function is documented with the correct access level (owner, agent, user, pending-owner, anyone).

---

## 9. Mainnet Upgrade Procedure

Recommended production flow for upgrading the vault under the timelock architecture:

1. **Step 1: Local Testing**: Deploy and test the upgrade extensively on a local environment using a mainnet state fork.
2. **Step 2: Install WASM**: Upload the compiled new WASM contract to the Testnet network to get the WASM hash.
3. **Step 3: Testnet Scheduling**: Call `schedule_upgrade` on Testnet.
4. **Step 4: Testnet Execution**: After the timelock expires on Testnet, call `execute_upgrade` and run `migrate()`. Verify the flow works end-to-end.
5. **Step 5: Mainnet scheduling announcement**: Schedule the mainnet upgrade and notify stakeholders, detailing the proposed WASM hash and the scheduled execution ledger/time.
6. **Step 6: Install WASM on Mainnet**: Install the WASM bytecode onto Mainnet to acquire the mainnet WASM hash.
7. **Step 7: Schedule Upgrade (Step 1 of Timelock)**: Call `schedule_upgrade(owner, new_wasm_hash)` on the mainnet vault. This initiates the mandatory 24-hour window.
8. **Step 8: Monitoring & Delay**: Monitor the network. Ensure no cancellation events are triggered and check that the correct hash is pending.
9. **Step 9: Execute Upgrade (Step 2 of Timelock)**: Once the timelock sequence is reached, execute the upgrade via `execute_upgrade(owner)`.
10. **Step 10: Run Migration**: Run the migration script and perform post-upgrade validation before resuming normal deposits and withdrawals.

---

## 10. Common Mistakes

**Mistake:** Removing a `DataKey` variant entirely from the enum.
**Result:** Orphaned storage. The data still exists on the ledger, consuming rent/deposits, but the contract completely lacks the type definitions to ever access or delete it.

**Mistake:** Changing struct field order.
**Result:** Deserialization failures. Soroban XDR relies on exact field ordering. The contract will trap/panic whenever it attempts to read the old data.

**Mistake:** Skipping migration version checks in the `migrate` function.
**Result:** Repeated migrations. If a migration is accidentally called twice, it might overwrite valid data with defaults or panic due to missing legacy keys.

---

## 11. Example DataKey Evolution

**Version 1 (Initial):**

```rust
pub enum DataKey {
    Config,
    Vault(u64),
}
```

**Version 2 (Safe Evolution):**

```rust
pub enum DataKey {
    Config,
    Vault(u64),
    Treasury,
}
```

_Why this is safe:_ We appended `Treasury` to the end. The XDR discriminants for `Config` (0) and `Vault` (1) remain unchanged. No migration is required for existing data.

**Version 3 (Unsafe Evolution - Migration Required):**

```rust
pub enum DataKey {
    Config,
    Position(u64),
    Treasury,
}
```

_Why migration is required:_ `Vault(u64)` was renamed to `Position(u64)`. While the XDR discriminant is technically still `1`, if the semantic meaning changed, or if we changed the inner type (e.g., from `u64` to an `Address`), the old data is now inaccessible via `Position`. A migration must be run to pull data from the old layout and restructure it into the new one.

---

## 12. User-Initiated Vault Migration (#637)

### 12.1 Purpose

User-initiated vault migration allows users to trustlessly move their share positions from an old vault contract to a new one during contract upgrades. This provides an alternative to storage migrations and enables users to maintain control over their funds during upgrades.

### 12.2 Architecture

**Components:**
- **Migration Target**: Owner-set address of the new vault contract
- **Migration Pause**: Independent pause state for migration operations
- **Share-to-Asset Conversion**: Preserves user value through exchange rate calculation
- **Cross-Contract Calls**: Transfers assets and calls deposit on new vault

**Storage Keys:**
- `DataKey::MigrationTarget`: Address of the new vault contract
- `DataKey::MigrationPaused`: Boolean flag for migration pause state

**Events:**
- `SharesMigratedEvent`: Emitted when user migrates shares
- `MigrationTargetUpdatedEvent`: Emitted when owner updates migration target
- `MigrationPausedEvent`: Emitted when migration is paused/unpaused

### 12.3 Migration Process

**Step 1: Owner Sets Migration Target**
```rust
vault.schedule_migration_target(owner, new_vault_address);
// wait MIGRATION_TARGET_TIMELOCK_LEDGERS (~24 h)
vault.execute_migration_target(owner);
```
- Only the owner can schedule, execute or cancel (`cancel_migration_target`) a target change
- The timelock gives users time to react before funds can be sent to a new target; `get_pending_migration_target()` shows what is scheduled
- Emits `MigrationTargetScheduledEvent` on schedule, `MigrationTargetUpdatedEvent` on execute and `MigrationTargetCancelledEvent` on cancel

**Step 2: (Optional) Owner Pauses Migration**
```rust
owner.set_migration_paused(true);
```
- Owner can pause migration independently of main vault pause
- Provides granular control during upgrades
- Emits `MigrationPausedEvent`

**Step 3: User Migrates Shares**
```rust
user.migrate_shares(user_address);
```
- User calls migration function on old vault
- Function burns user's shares in old vault
- Calculates asset value using current exchange rate
- Transfers assets to new vault
- Calls deposit on new vault on behalf of user
- Emits `SharesMigratedEvent`

### 12.4 Exchange Rate Preservation

The migration preserves user value through precise exchange rate calculation:

```rust
// In old vault:
let user_shares = get_user_shares(user);
let total_shares = get_total_shares();
let total_assets = get_total_assets();
let assets_to_transfer = (user_shares * total_assets) / total_shares;

// Transfer assets to new vault
transfer_usdc(old_vault, new_vault, assets_to_transfer);

// New vault mints shares based on its own exchange rate
new_vault.deposit(user, assets_to_transfer);
```

**Key Points:**
- Exchange rate is calculated at migration time in old vault
- New vault mints shares based on its own current exchange rate
- This ensures proportional ownership is preserved
- Small rounding differences may occur due to integer division

### 12.5 Security Considerations

**Migration Target Validation:**
- Only owner can set migration target
- Prevents migration to malicious contracts
- Users can verify target before migrating

**Independent Pause Control:**
- Migration can be paused without affecting deposits/withdrawals
- Owner can react to security concerns quickly
- Provides safety during upgrade process

**Authentication Requirements:**
- Migration requires user authentication
- Only users can migrate their own shares
- Prevents unauthorized position transfers

**Liquidity Constraints:**
- Migration requires sufficient liquidity in old vault
- If funds are deployed to protocols, they must be withdrawn first
- Partial migration may occur if liquidity is insufficient

### 12.6 Error Handling

**Common Migration Errors:**

| Error | Cause | Resolution |
|-------|-------|------------|
| `MigrationPaused` | Migration is paused by owner | Wait for owner to unpause or skip migration |
| `InvalidMigrationTarget` | No migration target set | Owner must set target before migration |
| `NoSharesToMigrate` | User has no shares | Deposit shares before attempting migration |
| `InsufficientLiquidity` | Old vault lacks sufficient assets | Owner may need to withdraw from protocols first |

### 12.7 Integration with Traditional Upgrades

User migration can complement traditional storage migrations:

**Scenario 1: Storage Migration Required**
- Use traditional `migrate()` function for storage schema changes
- User migration is optional for users who prefer to move manually

**Scenario 2: No Storage Migration Required**
- Deploy new vault with same storage schema
- Users can migrate their positions without any storage migration
- This is the preferred approach when possible

**Scenario 3: Hybrid Approach**
- Use storage migration for essential state changes
- Allow user migration for optional features
- Provides flexibility for different upgrade scenarios

### 12.8 Testing and Validation

**Pre-Migration Testing:**
1. Deploy new vault on testnet
2. Set migration target on old vault
3. Test migration with various share amounts
4. Verify exchange rate preservation
5. Test edge cases (zero shares, large amounts, etc.)

**Post-Migration Validation:**
1. Verify user shares in new vault
2. Verify asset value preservation
3. Check event logs for migration events
4. Validate total shares and assets in both vaults
5. Test withdrawal from new vault

### 12.9 Rollback Procedures

**If Migration Fails:**
1. Owner can pause migration to prevent further issues
2. Users can continue using old vault
3. Debug the issue and fix new vault
4. Resume migration after fix

**If New Vault Has Issues:**
1. Owner can update migration target to different address
2. Users can migrate to the corrected vault
3. Old vault remains operational as fallback

### 12.10 Monitoring and Alerts

**Key Metrics to Monitor:**
- Migration target changes
- Migration pause state changes
- Number of successful migrations
- Migration failure rates
- Asset transfer amounts

**Alert Triggers:**
- Unexpected migration target changes
- Migration paused without announcement
- High migration failure rates
- Large asset transfers during migration

### 12.11 User Communication

**Pre-Migration Announcements:**
- Inform users of upcoming upgrade
- Explain migration process and benefits
- Provide timeline for migration window
- Share new vault address for verification

**During Migration:**
- Provide real-time status updates
- Share migration statistics
- Address any issues or concerns
- Provide support for migration problems

**Post-Migration:**
- Confirm successful migration completion
- Provide instructions for using new vault
- Share performance metrics
- Archive old vault information

---

## 13. Upgrade Timelock & Governance Architecture (Issues #316, #607, #922)

### 13.1 Two-Step Upgrade Timelock Flow (#316)

Replacing contract code follows a mandatory two-step timelock to protect against unauthorized or accidental WASM upgrades:
1. `schedule_upgrade(owner, new_wasm_hash)`: The owner schedules the proposal, storing `PendingUpgradeHash` and establishing `UpgradeTimelockExpiry` (17,280 ledgers ≈ 24 hours). An `UpgradeScheduledEvent` is emitted.
2. **Review Window**: During this 24-hour delay, monitoring and off-chain watchers verify the proposed WASM hash against reproducible build artifacts. If unauthorized or erroneous, `cancel_upgrade(owner)` can be called to void the proposal.
3. `execute_upgrade(owner)`: Once the expiry ledger is reached and provided the vault is not paused, the owner invokes `execute_upgrade` to apply the new WASM and increment the contract version.

### 13.2 Second Signature & Multi-Sig Governance Decision (#607, #922)

In [`docs/GUARDIAN_KEY_DESIGN.md`](GUARDIAN_KEY_DESIGN.md), an interim smart-contract-level guardian key pattern (`DataKey::Guardian` co-signing `execute_upgrade`) was evaluated alongside native Stellar account multi-sig.

**Recorded Decision:**
- **Smart-contract-level guardian key is formally deferred**: Implementing an interim contract-level guardian introduces duplicate governance flows, storage overhead, and extra error variants while only protecting `execute_upgrade`.
- **Native Stellar Multi-Sig adopted**: Mainnet owner governance relies on Stellar native protocol-level account multi-sig (e.g. 2-of-3 threshold). This guarantees that *every* owner action—including `schedule_upgrade`, `execute_upgrade`, and `pause`—requires M-of-N independent cryptographic signatures at the transaction envelope level.
