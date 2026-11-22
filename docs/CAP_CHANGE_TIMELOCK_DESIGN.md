# Two-Step Confirmation for Large Admin Cap Changes

> **Issue:** #590
> **Category:** Security-Hardening
> **Status:** Design Decision Recorded
> **Author:** NeuroWealth Security Team
> **Date:** 2026-08-25

---

## 1. Background

`set_tvl_cap`, `set_user_deposit_cap`, and `set_caps` are single-transaction,
single-signature owner setters (see [`lib.rs`](../neurowealth-vault/contracts/vault/src/lib.rs)).
A single compromised or mistaken owner key can, in one call:

- Set `tvl_cap` or `user_deposit_cap` to `0`, blocking all new deposits
  (a denial-of-service against depositors, not a fund-loss risk — existing
  balances and withdrawals are unaffected by the caps).
- Set either cap to an absurdly high value, silently removing a safety rail
  that was protecting against over-concentration before the team is ready
  for that TVL tier.

Both are already visible on-chain (`TvlCapUpdatedEvent` / `UserDepositCapUpdatedEvent`
/ `CapsUpdatedEvent`, see [`EVENTS.md`](../EVENTS.md#8b-tvlcapupdatedevent)),
so the gap is not observability — it's that observation happens *after* the
change has already taken effect.

This document evaluates mirroring the existing `update_agent` /
`confirm_agent_update` timelock pattern (Issue #317, 17,280-ledger / ~24h
delay) for cap changes that exceed a configurable percentage delta.

---

## 2. Threat Model

### 2.1 What a Compromised or Mistaken Owner Can Do Today

| Action | Immediate effect | Reversible? |
| ------ | ----------------- | ----------- |
| `set_tvl_cap(0)` | Blocks all new deposits instantly | Yes — owner calls `set_tvl_cap` again. No fund loss. |
| `set_tvl_cap(huge_value)` | Removes the TVL safety rail instantly | Yes, same as above. |
| `set_user_deposit_cap(0)` | Blocks all new user deposits instantly | Yes, same as above. |
| `set_caps(...)` | Both of the above, atomically | Yes, same as above. |

Critically: **caps do not gate withdrawals or existing balances.** A
malicious cap change is a deposit-availability incident, not a solvency
incident. This bounds the urgency of the fix relative to, say, the agent-key
timelock (which guards against fund redirection) or the upgrade timelock
(which guards against arbitrary code execution).

### 2.2 Why It Still Matters

- **Reputational/UX damage is real even without fund loss.** Users hitting
  an unexpected deposit block during normal operation erodes trust, and a
  cap silently raised to an unsafe level defeats the purpose of having a cap
  at all.
- **Detection lag.** Without a delay, the *first* signal anyone gets is the
  `*CapUpdatedEvent` itself — after the change is live. A delay turns that
  into an early warning the team (or a future owner multi-sig co-signer) can
  act on before the change takes effect.
- **Blast radius scales with the delta.** A cap nudged by 5% is routine
  operations; a cap dropped to 0 or raised 100x is either a fat-fingered
  input or a compromised key. The risk is concentrated in large deltas,
  which is exactly what a percentage threshold can target without adding
  friction to routine adjustments.

---

## 3. Design Options Considered

### Option A — Percentage-delta-gated timelock (mirrors `update_agent`)

Any `set_tvl_cap` / `set_user_deposit_cap` / `set_caps` call whose new value
differs from the current value by more than a configurable threshold (e.g.
`±25%`) is **not applied immediately**. Instead it is recorded as a pending
change with an effective ledger `now + CAP_CHANGE_TIMELOCK_LEDGERS`, mirroring
`update_agent(new_agent)` → `confirm_agent_update()` → `cancel_agent_update()`.
Changes within the threshold apply immediately, exactly as today.

**Pros:**
- Directly reuses a pattern the codebase (and its test suite) already
  knows: `PendingAgent` / `AgentTimelockExpiry` → `PendingTvlCap` /
  `CapTimelockExpiry`, `confirm_agent_update` → `confirm_cap_change`,
  `cancel_agent_update` → `cancel_cap_change`.
- Threshold-gating means routine cap tuning (small increases as TVL grows)
  is unaffected — only large, suspicious deltas are delayed.
- `set_tvl_cap(0)` — the most damaging single call — always exceeds any
  reasonable percentage threshold from a nonzero cap, so it is always gated.

**Cons:**
- Adds two new storage keys per gated cap type (or one combined pending-cap
  struct for `set_caps`), plus two new public functions and two new events.
- Soroban's `#[contracterror]` enum is at its 50-variant limit (see
  [`LEAST_PRIVILEGE_AGENT.md`](LEAST_PRIVILEGE_AGENT.md) §5, rationale #2) —
  new error variants for "no pending cap change" / "timelock not yet
  expired" would require reusing existing variants or a restructuring pass,
  same constraint that deferred the least-privilege-agent split.
- `set_caps` gates two values atomically today; a threshold-gated version
  must decide whether *either* value exceeding the delta gates *both*, or
  each is evaluated independently (recommended: either exceeding gates the
  whole call, for atomicity and to avoid a partial-apply footgun).
- First-cap-ever-set (`old_cap == 0` in the `unwrap_or(0)` fallback) needs a
  defined percentage-delta behavior; recommend always gating in this case
  regardless of the raw percentage, since a from-zero delta divides by zero.

### Option B — Fixed absolute threshold instead of percentage

Gate changes only above/below fixed absolute cap bounds (e.g. any cap set
below `1_000 USDC` or moved by more than `500_000 USDC` in one call).

**Pros:** Simpler math (`i128` comparison, no percentage arithmetic or
divide-by-zero edge case for the first-ever cap).

**Cons:** Absolute thresholds don't scale with TVL — a threshold sized for a
$100k vault is meaningless (too loose) once TVL reaches $10M, and one sized
for $10M is too restrictive (blocks routine tuning) at $100k. Would need
periodic manual re-tuning by the owner, which is itself an ungated action.
Rejected in favor of Option A's percentage delta, which self-scales.

### Option C — Do nothing (status quo)

Rely on the existing immediate-effect setters plus event-based monitoring
(already covered by the `*CapUpdatedEvent` alerts in
[`monitoring.md`](monitoring.md)) and the planned owner multi-sig migration
(Issue #607, [`SECURITY.md`](../SECURITY.md) §6) to raise the bar on a single
compromised key initiating *any* privileged call, caps included.

**Pros:** Zero implementation cost; caps are already the least-damaging
category of admin setter (no direct fund-loss path); multi-sig, once shipped,
mitigates the single-compromised-key scenario for every setter at once,
including caps, without cap-specific code.

**Cons:** Does nothing for the honest-mistake case (fat-fingered value) even
after multi-sig ships, and leaves the "removed a safety rail without a
grace period" risk in place indefinitely.

---

## 4. Recorded Decision

### Decision: **ACCEPT (Option A) — Implementation Deferred, Tracked as a Follow-Up Issue**

**Rationale:**

1. Caps are a deposit-availability control, not a solvency control — the
   risk this design closes is real but strictly lower-severity than what the
   agent-key and upgrade timelocks already guard (fund redirection, arbitrary
   code execution). That lower severity justifies designing the mitigation
   now and scheduling implementation after the higher-priority owner
   multi-sig migration (Issue #607), rather than displacing it.
2. Option A's percentage-delta gate cleanly reuses the `update_agent` /
   `confirm_agent_update` / `cancel_agent_update` pattern this codebase
   already has tests, docs, and operational familiarity with — this is a
   proven pattern, not a novel one, which lowers implementation risk once
   scheduled.
3. The `#[contracterror]` 50-variant ceiling is a real constraint shared with
   the least-privilege-agent proposal. Reusing existing variants (e.g.
   `TimelockAlreadyPending`, `TimelockNotYetExpired`, or their equivalents
   from the agent-update timelock, generalized to cover caps) rather than
   minting cap-specific ones is the intended approach and should be
   confirmed feasible during implementation.
4. Multi-sig owner auth (once shipped) reduces — but does not eliminate —
   the value of this design: multi-sig raises the bar to compromise, this
   timelock adds a *detection window* even after a valid multi-sig-approved
   change, which is a different and complementary guarantee (catches an
   insider or a mistake approved by the full multisig, not just a stolen
   single key).

### Threshold & UX Parameters (recommended starting point for implementation)

| Parameter | Recommended value | Rationale |
| --------- | ------------------ | --------- |
| Gating threshold | `±25%` change from current value | Mirrors the "large, suspicious delta" framing; matches the tolerance band the monitoring `ALERT: tvl_cap_approach` rule (95% of cap) already assumes stays roughly stable between changes. |
| First-ever cap set (`old == 0`) | Always gated | Avoids divide-by-zero in the percentage calculation and treats "vault previously had no cap" as inherently a large-delta event. |
| Timelock duration | `AGENT_TIMELOCK_LEDGERS` (17,280 ledgers, ~24h) | Reuse the existing constant rather than introduce a second magic number; keeps operator mental model (\"admin timelocks are ~24h\") consistent. |
| `set_tvl_cap(0)` specifically | Always gated regardless of percentage | The single most damaging call (total deposit freeze) should never bypass the delay via threshold-boundary edge cases. |
| Cancel path | Owner-only, mirrors `cancel_agent_update` | Lets the legitimate owner abort a change they now recognize as mistaken or malicious, without waiting out the full delay. |
| `set_caps` atomicity | Gate the whole call if *either* value exceeds the threshold | Prevents a partial-apply state where one cap changed immediately and the other is pending — matches the existing atomic intent of `set_caps`. |

### Implementation Plan (for the follow-up issue)

1. **Storage:** Add `DataKey::PendingTvlCap(i128)`, `DataKey::PendingUserDepositCap(i128)`,
   `DataKey::CapChangeTimelockExpiry(u32)` — or a single combined
   `PendingCapChange { tvl_cap: Option<i128>, user_cap: Option<i128>, expiry: u32 }`
   struct to keep `set_caps` atomic under one storage key (preferred, avoids
   two independent timelocks racing against each other).
2. **Setters:** `set_tvl_cap`, `set_user_deposit_cap`, `set_caps` compute the
   percentage delta against the current effective value; below threshold,
   behavior is unchanged (immediate apply, existing events); at/above
   threshold, write the pending struct and emit a new
   `CapChangeScheduledEvent { old_value, proposed_value, effective_ledger }`
   instead of the immediate `*CapUpdatedEvent`.
3. **New functions:** `confirm_cap_change(env: Env)` (owner-only, requires
   `ledger().sequence() >= effective_ledger`, applies the pending value(s)
   and emits the existing `*CapUpdatedEvent`(s) plus a new
   `CapChangeConfirmedEvent`) and `cancel_cap_change(env: Env)` (owner-only,
   clears the pending struct, emits `CapChangeCancelledEvent`).
4. **Errors:** Reuse or generalize `TimelockAlreadyPending` /
   `TimelockNotYetExpired` (confirm naming doesn't collide with agent-update
   semantics in test assertions) rather than adding new variants, respecting
   the 50-variant ceiling.
5. **Docs:** Add the three new events to [`EVENTS.md`](../EVENTS.md)
   following the existing `AgentUpdateProposedEvent` /
   `AgentUpdateConfirmedEvent` / `AgentUpdateCancelledEvent` pattern; update
   [`MAINNET_CHECKLIST.md`](MAINNET_CHECKLIST.md) §3 (Administrative Caps)
   to document the new two-step flow for operators.
6. **Tests:** Mirror `test_agent_timelock.rs` structure — happy path
   (schedule → wait → confirm), early-confirm rejection, cancel path,
   below-threshold immediate-apply regression, `set_caps` atomicity under
   gating, and the `old == 0` always-gated edge case.

### Conditions for Re-Prioritizing Ahead of the Multi-Sig Migration

- A cap-change incident (mistaken or malicious) actually occurs on testnet
  or mainnet before multi-sig ships.
- A security audit flags the ungated cap setters as a finding.
- TVL grows large enough that a cap misconfiguration's UX/reputational
  blast radius becomes disproportionate to the remaining implementation cost.

---

## 5. References

- [`LEAST_PRIVILEGE_AGENT.md`](LEAST_PRIVILEGE_AGENT.md) — Sibling
  design-decision doc; same `#[contracterror]` ceiling constraint, same
  "defer non-fund-loss hardening behind the multi-sig migration" reasoning.
- [`EVENTS.md`](../EVENTS.md#8b-tvlcapupdatedevent) — Existing cap-change
  event schema.
- [`monitoring.md`](monitoring.md) — `ALERT: tvl_cap_approach` and related
  cap-monitoring rules this design complements.
- [`MAINNET_CHECKLIST.md`](MAINNET_CHECKLIST.md) §3 — Administrative Caps &
  Deposit Limits Configuration.
- Issue #317 — Agent update timelock (`update_agent` / `confirm_agent_update`
  / `cancel_agent_update`), the pattern this design mirrors.
- Issue #607 — Owner multi-sig migration, referenced as the higher-priority,
  complementary hardening this implementation is sequenced behind.
