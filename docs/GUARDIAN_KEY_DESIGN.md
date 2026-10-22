# Guardian-Key Design: Second Signature for `execute_upgrade`

> **Issue:** #607, #922  
> **Category:** Audit-Prep / Security Architecture  
> **Status:** Formally Deferred (Native Stellar Multi-Sig Adopted) — Issue #922  
> **Author:** NeuroWealth Security Team  
> **Date:** 2026-08-24 (Updated: 2026-10-16)

---

## 1. Background

A stolen owner key combined with a pending upgrade proposal is the
**highest-blast-radius scenario** in the NeuroWealth threat model. The
current upgrade flow (Issue #316) is:

1. `schedule_upgrade(owner, new_wasm_hash)` — owner proposes a new WASM.
2. Wait `UPGRADE_TIMELOCK_LEDGERS` (17,280 ledgers ≈ 24 hours).
3. `execute_upgrade(owner)` — owner applies the pending WASM.

The 24-hour window allows monitoring to detect a malicious proposal and the
owner to call `cancel_upgrade()` before it takes effect. However, if the
owner key is compromised, the attacker controls **both** the proposal and
the execution — there is no independent check on the execution step.

This document evaluates three approaches to adding a second line of defence
on `execute_upgrade`:

| Approach | Description |
|----------|-------------|
| **Status Quo** | Single owner key, 24 h timelock, `cancel_upgrade()` as the only defence |
| **Guardian Pattern** | A separate `guardian` keypair must co-sign `execute_upgrade` |
| **Owner Multi-Sig** | Owner role held by a Stellar multi-sig account (M-of-N signers required for every owner call) |

---

## 2. Current Flow (Status Quo)

```
Owner key ──► schedule_upgrade(wasm_hash)
               ↓ 24 h timelock
Owner key ──► execute_upgrade()
               ↓
         WASM replaced
```

**Strengths:**
- Simple, already deployed and tested.
- 24-hour window is sufficient for security monitoring to detect and alert.
- `cancel_upgrade()` does not require the owner key to be uncompromised at
  execution time (it requires it, but the monitoring team can act promptly).

**Weaknesses:**
- A single compromised key can both schedule and execute a malicious upgrade
  within the timelock window.
- Requires 24-hour human response SLA. If the team is unreachable, the
  attacker wins.

---

## 3. Guardian Pattern

### 3.1 Concept

A `guardian` address is stored alongside the owner. `execute_upgrade` requires
**both** the owner signature **and** the guardian signature. The guardian key
is stored on separate infrastructure (different cloud account, different HSM).

```
Owner key ──► schedule_upgrade(wasm_hash)
               ↓ 24 h timelock
Owner key  ┐
           ├──► execute_upgrade()   ← both required
Guardian   ┘
               ↓
         WASM replaced
```

### 3.2 Storage Changes

```rust
DataKey::Guardian  // Address; optional — if absent, falls back to owner-only
```

### 3.3 Contract Changes

```rust
pub fn set_guardian(env: Env, new_guardian: Address) {
    // Owner-only; no timelock needed (guardian only gates execute_upgrade)
    Self::require_initialized(&env);
    Self::require_is_owner(&env);
    env.storage().instance().set(&DataKey::Guardian, &new_guardian);
    // emit GuardianUpdatedEvent
}

pub fn execute_upgrade(env: Env, owner: Address) {
    Self::require_initialized(&env);
    owner.require_auth();
    Self::require_not_paused(&env);

    let stored_owner: Address = env.storage().instance().get(&DataKey::Owner).unwrap();
    Self::require(&env, owner == stored_owner, VaultError::CallerIsNotOwner);

    // --- NEW: guardian co-signature requirement ---
    if let Some(guardian) = env.storage().instance().get::<DataKey, Address>(&DataKey::Guardian) {
        guardian.require_auth();
    }
    // --- END NEW ---

    // ... rest of existing execute_upgrade logic ...
}
```

### 3.4 Threat Analysis

| Scenario | Status Quo | Guardian Pattern |
|----------|------------|-----------------|
| Owner key stolen, guardian safe | Attacker can execute upgrade after timelock | Attacker **cannot** execute — guardian co-sign required |
| Guardian key stolen, owner safe | N/A — no guardian today | Attacker can schedule but **cannot** execute (owner co-sign required) |
| Both keys stolen | N/A | Attacker can execute upgrade (no better than status quo, but both keys must be stolen) |
| Owner key lost (legitimate) | Owner cannot execute upgrades | Owner and guardian needed — could lock out legitimate upgrades if both keys are lost |
| Guardian key lost (legitimate) | N/A | Legitimate owner cannot execute upgrades until guardian is rotated |

**Key benefit:** An attacker who steals only the owner key cannot execute
an upgrade. They must also compromise the guardian.

**Key risk:** If the guardian key is lost, legitimate upgrades are blocked
until the guardian is rotated. Guardian rotation itself requires only owner
auth (no timelock), so it is relatively fast.

### 3.5 Migration / Compatibility Notes

- `DataKey::Guardian` is absent on existing instances — the implementation
  must check `if let Some(guardian)` and skip the co-sign when absent.
- This is backward-compatible: existing deployments without a guardian set
  fall back to the current single-owner behavior.
- Guardian can be set in the same deployment transaction as the upgrade that
  adds the guardian feature.
- No changes to `schedule_upgrade` or `cancel_upgrade` are required.

---

## 4. Owner Multi-Sig

### 4.1 Concept

Rather than a contract-level second key, the owner role is held by a Stellar
**multi-sig account** (e.g., 2-of-3 signers). Every owner-gated call requires
M of the N signers to approve the Stellar transaction envelope.

```
Signer A ─┐
Signer B ──┼──► execute_upgrade()   ← 2-of-3 required at Stellar protocol level
Signer C ─┘
               ↓
         WASM replaced
```

### 4.2 Strengths

- **Protocol-level enforcement** — multi-sig is enforced by the Stellar ledger,
  not the contract. No new contract logic needed.
- **Covers all owner actions** — not just `execute_upgrade`. Every owner-only
  call (`set_tvl_cap`, `set_blend_pool`, `pause`, etc.) automatically requires
  M-of-N signers.
- **Rotation without contract upgrade** — adding/removing signers is a Stellar
  account operation, not a contract upgrade.
- **Industry standard** — multi-sig governance is the expected mainnet
  configuration for any serious DeFi protocol.

### 4.3 Weaknesses

- **Coordination overhead** — every owner action (including routine
  `set_rebalance_cooldown`) requires M signers to be online and agree.
- **Operational complexity** — requires a multi-sig coordination tool (e.g.,
  Stellar's `stellar-multisig`, Gnosis Safe equivalent for Stellar).
- **No contract-level awareness** — the contract cannot distinguish a
  multi-sig owner from a single-sig owner; it just sees the owner address.
  Contract-level events do not capture which signers participated.

### 4.4 Interaction with Existing Timelock

Multi-sig owner is fully compatible with the upgrade timelock:

```
Multi-sig account ──► schedule_upgrade(wasm_hash)   ← requires M signers
                       ↓ 24 h timelock
Multi-sig account ──► execute_upgrade()              ← requires M signers again
```

The attacker must compromise M of N signers **twice** (once to schedule, once
to execute), or compromise M signers and wait out the timelock.

---

## 5. Comparison Matrix

| Property | Status Quo | Guardian | Multi-Sig |
|----------|-----------|----------|-----------|
| Protects execute_upgrade from single-key theft | ❌ | ✅ | ✅ |
| Protects ALL owner actions | ❌ | ❌ | ✅ |
| Contract changes required | None | Small | None |
| Operational complexity | Low | Medium | High |
| Backward-compatible (no upgrade) | ✅ | ❌ (needs upgrade) | ✅ (set via account op) |
| Recovery if second key is lost | N/A | Rotate guardian (owner-only) | Add new signer (M signers needed) |
| On-chain transparency | Events per function | Events per function | No signer-level events |
| Mainnet readiness | Acceptable (risk acknowledged) | Good | Best |

---

## 6. Recorded Decision (Issue #922)

### Decision: **FORMALLY DEFER Contract-Level Guardian Key (Phase 1) in Favor of Native Stellar Owner Multi-Sig (Phase 2)**

**Rationale:**

1. **Protocol-Level Protection Across All Operations**:
   Stellar natively enforces multi-signature thresholds directly at the account level (Layer 1). A 2-of-3 multi-sig owner account protects **all** privileged calls (`set_tvl_cap`, `set_blend_pool`, `pause`, `unpause`, `schedule_upgrade`, and `execute_upgrade`), whereas a smart-contract-level guardian key only protects `execute_upgrade`. Gating `execute_upgrade` alone with a contract-level key leaves all other owner-gated administrative powers vulnerable to single-key compromise.

2. **Avoiding Redundant Interim Complexity and Attack Surface**:
   Implementing an interim contract-level guardian pattern would require:
   - Introducing `DataKey::Guardian` in contract instance storage.
   - Adding entrypoints `set_guardian` and `remove_guardian`.
   - Adding new authorization checks and `GuardianUpdatedEvent` emission.
   - Adding extra error handling within the constrained `#[contracterror]` 50-variant SDK budget.
   
   Once the owner account is configured with native Stellar multi-sig, any smart-contract guardian mechanism becomes redundant dead code and a source of conflicting operational workflows.

3. **Existing Timelock and Cancellation Safeties**:
   The existing two-step upgrade flow (`schedule_upgrade` → 17,280 ledgers / ~24 hours → `execute_upgrade`) already provides a robust observation window. Off-chain monitoring alerts immediately upon `UpgradeScheduledEvent`, allowing the owner key (or multi-sig quorum) to call `cancel_upgrade()` or `pause()` the vault before an unauthorized proposal can ever execute.

4. **Consistency with Pre-Mainnet Deferral Precedents**:
   Both [`CAP_CHANGE_TIMELOCK_DESIGN.md`](CAP_CHANGE_TIMELOCK_DESIGN.md) (§4) and [`ROLLING_DECREASE_CAP_DESIGN.md`](ROLLING_DECREASE_CAP_DESIGN.md) (§5) formally deferred bespoke contract logic in favor of prioritizing the native owner multi-sig migration. Deferring the interim contract-level guardian avoids fragmented security paradigms and concentrates engineering resources on native multi-sig operational readiness.

### Conditions for Re-Opening

This decision may be revisited if:
- Stellar protocol changes compromise native account multi-sig capabilities or make them cost-prohibitive.
- An institutional deployer requires an off-chain automated HSM key with a distinct contract-enforced role that cannot participate in the Stellar account signer quorum.
- The Soroban SDK error ceiling is lifted and a unified multi-role governance module is adopted across the protocol.

---

## 7. Deferred Phase 1 Specification (Archival Reference)

The original interim implementation plan is preserved below for reference should the design ever be re-evaluated:

1. Add `DataKey::Guardian` to the storage key enum.
2. Implement `set_guardian(env, new_guardian)` (owner-only, no timelock).
3. Implement `remove_guardian(env)` (owner-only) for emergency key removal.
4. Add `GuardianUpdatedEvent` with topic `guardian`.
5. Update `execute_upgrade` to check for guardian co-sign when `DataKey::Guardian` is present:
   ```rust
   if let Some(guardian) = env.storage().instance().get::<DataKey, Address>(&DataKey::Guardian) {
       guardian.require_auth();
   }
   ```
6. Add `VaultError::GuardianSignatureRequired` if explicit error signaling is required.

### Compatibility Notes (Archival)

- Existing deployed instances without `DataKey::Guardian` continue to work with the current single-owner `execute_upgrade`.
- Guardian key can be set immediately after the upgrade that introduces it, with no additional timelock.
- The guardian key should be stored in a **geographically separate** location from the owner key (different cloud provider, different HSM).
- Guardian does NOT participate in `schedule_upgrade` or `cancel_upgrade` — only in `execute_upgrade`. This keeps the proposal/cancellation flow simple.

---

## 8. Open Questions for Multi-Sig Governance

- Which multi-sig tool / UI will NeuroWealth use for Stellar? (Recommended: Stellar native account threshold multi-sig via Lobstr Vault / Casa / custom coordination tool).
- What M-of-N threshold is appropriate? (Recommended: 2-of-3 for balance of
  security and availability.)
- Who holds each signer key, and what are the custody policies?
- How will emergency actions (e.g., emergency pause) be handled when
  signers are unavailable?

---

## 9. References

- [`SECURITY.md`](../SECURITY.md) — Upgrade risk section, owner-compromise runbook
- [`ARCHITECTURE.md`](../ARCHITECTURE.md) — Upgrade safety, timelock design
- Issue #316 — Upgrade timelock implementation
- Issue #607 — Guardian-key design evaluation
- Issue #922 — Formal deferral of guardian key in favor of native Stellar multi-sig
- [`docs/UPGRADE_MIGRATION.md`](UPGRADE_MIGRATION.md) — Upgrade migration guide
