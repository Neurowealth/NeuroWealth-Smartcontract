# Least-Privilege Agent Design: Separate Rebalancer vs Reporter Roles

> **Issue:** #606  
> **Category:** Audit-Prep  
> **Status:** Design Decision Recorded  
> **Author:** NeuroWealth Security Team  
> **Date:** 2026-08-24

---

## 1. Background

The NeuroWealth Vault grants a single AI-agent keypair two distinct powers:

| Power | Function | Risk if agent is compromised |
|-------|----------|------------------------------|
| **Rebalancer** | `rebalance(protocol, expected_apy, min_out)` | Moves vault funds between Blend, DEX, and idle — bounded by on-chain whitelisted pool addresses and `min_out` slippage guards. |
| **Reporter** | `update_total_assets(agent, new_total, allow_decrease, max_decrease_bps)` | Reports yield accrual or strategy losses, updating the `TotalAssets` figure used for all share pricing. |

Because both powers live under the same `DataKey::Agent` address, a single
compromised (or stolen) agent key gives an attacker **both** capabilities
simultaneously. The question this document answers is: **should these be
split into two independently-rotatable sub-roles?**

---

## 2. Threat Model

### 2.1 What a Compromised Agent Can Do Today

With the current single-agent design, an attacker who obtains the agent key
can:

1. **Rebalance to a malicious pool** — the pool address must already be
   whitelisted by the owner (`set_blend_pool` / `set_dex_pool`), so this
   attack requires *either* a prior owner compromise or a social-engineering
   attack to get a malicious pool whitelisted first. Direct fund theft is
   therefore **not possible with the agent key alone**.

2. **Report a loss via `update_total_assets`** — the function contains
   several guardrails:
   - Losses require `allow_decrease = true` **and** owner co-signature.
   - The loss is bounded per call by `max_decrease_bps` (floor 100 bps = 1%).
   - Total assets cannot be reported above the on-chain USDC balance.
   
   An attacker without the owner key **cannot report a loss**, removing the
   most dangerous asset-inflation/deflation vector.

3. **Rebalance-churn** — repeatedly rebalancing between protocols consumes
   gas and may trigger slippage, but is rate-limited by
   `RebalanceCooldown`. Maximum damage rate is bounded.

4. **Harvest spam** — calling `harvest()` repeatedly triggers the cooldown
   and circuit-breaker, potentially auto-pausing the vault. Disruptive but
   recoverable by the owner.

### 2.2 What a Compromised Rebalancer-Only Key Could Do

If `rebalance()` were gated on a separate `RebalancerAgent` key, a compromise
would be **strictly less dangerous** than today: the attacker could not
report losses (the reporter key is separate) and could not drain funds
(pool addresses still require owner whitelisting).

### 2.3 What a Compromised Reporter-Only Key Could Do

If `update_total_assets()` were gated on a separate `ReporterAgent` key,
a compromise without the owner key would be nearly harmless: the solvency
check and owner co-signature requirement already prevent malicious
downward reports. The reporter key alone could only report inflated totals
up to the real on-chain balance — which is a no-op beyond honest reporting.

---

## 3. Sub-Role Separation Design

### 3.1 Proposed Storage Changes

```rust
// New keys — additive, backward-compatible:
DataKey::RebalancerAgent  // address that may call rebalance() and harvest()
DataKey::ReporterAgent    // address that may call update_total_assets()

// Retained for backward compatibility and emergency_harvest:
DataKey::Agent            // legacy key; if RebalancerAgent is absent, falls back to Agent
```

During a migration `initialize_v2()` or a contract upgrade, the existing
`Agent` value would be copied into **both** `RebalancerAgent` and
`ReporterAgent`, preserving current behavior until operators choose to
rotate them independently.

### 3.2 Auth Changes

```rust
// rebalance() / harvest():
let rebalancer: Address = env.storage().instance()
    .get(&DataKey::RebalancerAgent)
    .unwrap_or_else(|| env.storage().instance().get(&DataKey::Agent).unwrap());
rebalancer.require_auth();

// update_total_assets():
let reporter: Address = env.storage().instance()
    .get(&DataKey::ReporterAgent)
    .unwrap_or_else(|| env.storage().instance().get(&DataKey::Agent).unwrap());
Self::require(&env, agent == reporter, VaultError::OnlyAgentCanUpdateTotalAssets);
agent.require_auth();
```

### 3.3 New Admin Functions

```rust
pub fn set_rebalancer_agent(env: Env, new_rebalancer: Address) { /* owner-only, timelocked */ }
pub fn set_reporter_agent(env: Env, new_reporter: Address)    { /* owner-only, timelocked */ }
```

Each would follow the existing agent-update timelock pattern (Issue #317):
propose → wait `AGENT_TIMELOCK_LEDGERS` → confirm.

### 3.4 Event Implications

New events would be required:

| Event | Topic | Data |
|-------|-------|------|
| `RebalancerAgentUpdatedEvent` | `rb_agent` | `{old, new, effective_ledger}` |
| `ReporterAgentUpdatedEvent`   | `rp_agent` | `{old, new, effective_ledger}` |

Indexers and monitoring dashboards must be updated to watch both events.
The existing `AgentUpdatedEvent` (topic `ag_upd`) would be retained and
emitted when the legacy `Agent` key is updated.

### 3.5 Rotation Paths

| Scenario | Today | With Sub-Roles |
|----------|-------|----------------|
| Agent key leaked | Rotate single key via owner (24 h timelock) | Rotate only the compromised sub-role; the other remains live |
| Rebalancer outage | N/A | Rotate only `RebalancerAgent`; reporter continues operating |
| Reporter outage | N/A | Rotate only `ReporterAgent`; rebalancer continues operating |
| Both roles on same machine | Same exposure as today | Same — operator must segregate keys on separate infra to gain benefit |

---

## 4. Cost–Benefit Analysis

### 4.1 Benefits

- **Blast-radius reduction**: Compromise of the rebalancer key cannot report
  losses (requires owner co-sign anyway) and vice-versa.
- **Independent rotation**: Rebalancer and reporter can be rotated on
  different schedules, hosted on different infrastructure, and use different
  HSM/KMS policies.
- **Auditability**: On-chain events make it clear which sub-role performed
  which action, improving incident forensics.

### 4.2 Costs

- **Contract complexity**: Two new storage keys, two new admin functions, two
  new timelocked update flows, two new events, and updated auth checks.
- **Client / SDK breakage**: Off-chain clients that currently read
  `get_agent()` and assume it covers both powers must be updated.
- **Operational overhead**: Operators must manage two keypairs instead of
  one, with separate rotation schedules and key custody policies.
- **Upgrade required**: The change is not backward-compatible as an in-place
  storage update — it requires a contract upgrade and migration step.

### 4.3 Risk Assessment of Current Design

The current guardrails already significantly limit damage from a compromised
agent key:

- Loss reporting requires **owner co-signature** — the agent alone cannot
  deflate share value.
- Rebalancing targets **owner-whitelisted pool addresses** — the agent cannot
  redirect funds to an arbitrary address.
- The rebalance cooldown limits churn frequency.
- The circuit breaker auto-pauses after `N` consecutive failures.

These guardrails substantially reduce the incremental security benefit of
sub-role separation for the **current attack surface**.

---

## 5. Go / No-Go Decision

### Decision: **DEFER — Not Implemented in Current Version**

**Rationale:**

1. The existing guardrails (owner co-sign for losses, pool whitelist, cooldown,
   circuit-breaker) already substantially bound the blast radius of a
   compromised agent key. The marginal security improvement from sub-role
   separation does not justify the contract complexity and operational
   overhead **at this stage**.

2. The Soroban `#[contracterror]` enum is at its 50-variant limit. Adding
   sub-role-specific error codes would require restructuring the error type.

3. The planned migration to **owner multi-sig** (see Issue #607 and
   [`SECURITY.md`](../SECURITY.md) Step 6) is a higher-priority security
   hardening that will be implemented before mainnet. Multi-sig owner auth
   already provides a second key requirement for the highest-risk operations.

4. Sub-role separation is a meaningful hardening for a **post-mainnet**
   upgrade when the contract has proven stable and TVL justifies the added
   complexity.

### Conditions for Re-Opening

This decision should be revisited if **any** of the following occur:

- TVL exceeds $1M and the rebalancer and reporter agents are on the same
  infrastructure.
- A security audit identifies a novel attack path that sub-role separation
  would directly mitigate.
- The Soroban SDK removes the 50-variant error limit.
- The team moves to a fully decentralized governance model where agent keys
  are held by different parties.

---

## 6. Interim Mitigations

Until sub-role separation is implemented, the following operational controls
should be enforced:

- [ ] Rebalancer agent key and reporter agent key are generated from separate
  entropy sources, even though they share the same on-chain address today.
- [ ] Agent key is stored in a dedicated HSM or cloud KMS, separate from the
  owner key.
- [ ] Monitoring alerts fire within 5 minutes on any unexpected
  `update_total_assets` call (especially with `allow_decrease = true`).
- [ ] Owner co-sign for loss reports is enforced by policy and audited
  monthly.
- [ ] Rebalance cooldown is set to a non-zero value on mainnet
  (`set_rebalance_cooldown`).

---

## 7. References

- [`SECURITY.md`](../SECURITY.md) — Trust model, owner-compromise runbook
- [`ARCHITECTURE.md`](../ARCHITECTURE.md) — Storage layout, agent update timelock
- Issue #317 — Agent update timelock implementation
- Issue #506 — Emergency harvest (owner-gated fallback when agent is unavailable)
- Issue #439 — Circuit-breaker auto-pause
