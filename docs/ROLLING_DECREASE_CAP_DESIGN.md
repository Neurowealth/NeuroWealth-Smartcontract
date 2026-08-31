# Rolling-Window Decrease Cap on `update_total_assets`

> **Issue:** #589
> **Category:** Security-Hardening
> **Status:** Design Decision Recorded
> **Author:** NeuroWealth Security Team
> **Date:** 2026-08-25

---

## 1. Background

`update_total_assets(agent, new_total, allow_decrease, max_decrease_bps)`
already bounds any **single** reported decrease to at most `max_decrease_bps`
(minimum floor: 100 bps = 1%) of the current total, and requires the
**owner's co-signature** on every decrease — see the docstring above
`update_total_assets` in [`lib.rs`](../neurowealth-vault/contracts/vault/src/lib.rs)
and [`monitoring.md`](monitoring.md) §1 (`Loss reporting bps` row).

That single-event cap does **not** bound the *sum* of decreases across
multiple calls. A compromised agent that also has (or obtains) owner
co-signature — or an owner key that is itself compromised and calling
`update_total_assets` directly — can report the maximum allowed decrease
repeatedly, ledger after ledger, bleeding the vault's reported value down
to near zero over time while every individual call stays "compliant" with
the existing per-event cap.

This document evaluates adding a **rolling-window cumulative decrease cap**
(e.g., "no more than X% cumulative decrease across any trailing N-ledger
window") as containment against exactly that repeated-small-decrease pattern.

---

## 2. Threat Model

### 2.1 Why the Existing Per-Event Cap Isn't Enough

The per-event cap answers "how much damage can one call do?" It does not
answer "how much damage can an attacker with sustained access do?" Given:

- Owner co-signature is already required for every decrease (this is not a
  single-key compromise scenario — it requires the owner key too, or an
  owner who is themselves the attacker/compromised).
- The rolling window is specifically containment for the case where that
  bar is cleared: owner key compromised, or a malicious/coerced owner
  colluding with a compromised agent.

At, say, `max_decrease_bps = 1000` (10%), ten consecutive max-sized decrease
calls reduce total assets to roughly `0.9^10 ≈ 35%` of the starting value —
a 65% loss — while every individual call was within policy. There is
currently no mechanism that would flag or block the 2nd, 3rd, ... 10th call
as different from the 1st.

### 2.2 What a Rolling Window Changes

A rolling-window cap doesn't prevent an owner+agent collusion attack from
eventually draining value if given unlimited time and unlimited calls — no
on-chain cap can prevent an authorized party from acting maliciously forever.
What it *does* do is force the decrease to happen **slowly enough** that:

- Off-chain monitoring (the `tvl_drop_20pct` and `share_price_decrease`
  alerts already defined in [`monitoring.md`](monitoring.md) §4) has time to
  fire and page on-call *before* cumulative damage becomes catastrophic,
  rather than after.
- The team has a real window to invoke `emergency_pause` (see
  [`AGENT_KEY_COMPROMISE_RUNBOOK.md`](AGENT_KEY_COMPROMISE_RUNBOOK.md)) and
  freeze further calls, because the attacker is rate-limited into needing
  multiple ledger-spaced transactions rather than draining value in one
  block.
- The blast radius of a *single* compromised-owner-co-signature event (e.g.
  a phished multisig signer approving one bad transaction, once multi-sig
  ships per Issue #607) is bounded, even if that one signature is abused
  for the maximum the current per-event cap allows.

This reframes the feature correctly: **it is a rate limiter on cumulative
reported loss, not a hard ceiling on total possible loss.** Honest strategy
losses (e.g., a genuine Blend bad-debt event) are also rate-limited by this
design — see the tradeoff discussion below.

---

## 3. Design Tradeoffs: Honest Loss Reporting vs. Containment

This is the central tension the issue asks to evaluate, and it does not
have a clean answer — hence "design note," not "implementation issue."

### 3.1 The Case *For* a Rolling Cap

- Turns an unbounded-frequency attack into a bounded-rate one, buying
  detection and response time (§2.2).
- Symmetric with existing philosophy: the per-event cap already accepts
  that *some* friction on legitimate large losses is worth the containment
  benefit (see the existing `max_decrease_bps` floor of 100 bps — the
  contract already refuses to let the caller set an arbitrarily tight cap
  that would block *any* decrease reporting, but does impose a nonzero
  floor of friction by design).
- A sufficiently generous window (see §4) makes the false-positive cost
  low for realistic strategy-loss magnitudes, while still bounding the
  worst case.

### 3.2 The Case *Against* (or for a generous window)

- **A genuine bad-debt event might need to be reported all at once.** If
  Blend or a DEX pool the vault is deployed into suffers an actual,
  confirmed loss (see [`BLEND_INTEGRATION_RESEARCH.md`](BLEND_INTEGRATION_RESEARCH.md)
  and [`monitoring.md`](monitoring.md) §9, Blend bad-debt monitoring), the
  *honest* thing to do is report the real new total promptly, once, fully
  co-signed by the owner after verification. A rolling cap that blocks or
  delays that honest report leaves `TotalAssets` — and therefore every
  user's `get_balance()` — overstated for however long it takes to trickle
  the correction through multiple gated calls. Overstated assets mean users
  can withdraw *more* than their true pro-rata share, which is a direct
  fund-loss vector for later withdrawers, arguably worse than the slow-bleed
  scenario the rolling cap is meant to contain.
- **The owner co-signature is already the primary control.** Every decrease
  requires explicit owner authorization today; the rolling cap adds friction
  to an already-gated path rather than closing an ungated one. Its value is
  specifically for the owner-key-compromised or malicious-owner case (§2.1),
  which is a narrower threat than "any decrease at all."
- **Interacts with the emergency-harvest and circuit-breaker paths.** A
  strategy already experiencing repeated failures (circuit breaker, Issue
  #439) may legitimately need several corrective `update_total_assets` calls
  in succession as the team stabilizes the position — a rolling cap sized
  too tightly could itself become an operational obstacle during an incident
  it should be helping contain.

### 3.3 Reconciling the Tension: Escape Hatch Requirement

Any accepted design **must** include an owner-gated override/escape hatch
for the confirmed-bad-debt case — otherwise the rolling cap trades one
failure mode (slow bleed) for another (stale, overstated `TotalAssets`
during a real incident), which is a worse outcome. See §4.4.

---

## 4. Design Options Considered

### Option A — Rolling-window cumulative-bps cap with owner-gated override

Track cumulative decrease-bps within a trailing `N`-ledger window (e.g.
`WINDOW_LEDGERS = 17_280`, ~24h, reusing the existing timelock-duration
convention). Each decrease call checks that `cumulative_decrease_in_window +
this_decrease <= window_cap_bps` (a separate, presumably larger, threshold
than the existing per-event `max_decrease_bps`, e.g. `window_cap_bps = 2500`
= 25% cumulative per 24h). A new owner-only `force_update_total_assets`
(or an `override` flag threading through the existing function, gated behind
an *additional* explicit owner confirmation beyond the normal co-sign) bypasses
the window cap for confirmed bad-debt events, emitting a distinct event so
the override itself is loudly visible on-chain.

**Pros:** Directly targets the threat in §2; the override in §4.4 resolves
the honest-loss tension; window duration/cap are independently tunable from
the per-event cap, so operators can calibrate without touching existing
behavior.

**Cons:** Requires new persistent state (a rolling log or a simpler
decaying-counter approximation — see §4.1 implementation note), a new error
variant (colliding with the 50-variant `#[contracterror]` ceiling already
noted in [`LEAST_PRIVILEGE_AGENT.md`](LEAST_PRIVILEGE_AGENT.md) and
[`CAP_CHANGE_TIMELOCK_DESIGN.md`](CAP_CHANGE_TIMELOCK_DESIGN.md)), and a new
override path that is itself a fresh attack surface if not carefully gated
(an override that's too easy to invoke defeats the whole point).

#### 4.1 Implementation note: exact rolling log vs. decaying counter

An *exact* rolling window (a log of `(ledger, decrease_bps)` entries, pruned
each call) is simple to reason about but has unbounded storage growth under
adversarial-frequency calls — bounded only by the fact that each call
requires owner co-signature, which rate-limits call frequency in practice,
but is not a hard on-chain guarantee. A *decaying-counter* approximation
(a single `cumulative_decrease_estimate` that decays linearly/exponentially
per elapsed ledger since the last update, à la a leaky bucket) uses O(1)
storage and is the pattern already used elsewhere in the codebase for
similar rate-limiting (see `rate-limiting` conventions referenced in
[`monitoring.md`](monitoring.md) if applicable, or the existing rebalance
cooldown's simple "last ledger" tracking as the nearest on-chain precedent).
**Recommend the decaying-counter approach** for storage-cost and simplicity
reasons, accepting the minor imprecision (a decaying approximation is
slightly more permissive near window boundaries than an exact log) as a
reasonable tradeoff given this is a rate limiter, not a hard ceiling (§2.2).

### Option B — Alert-only (no on-chain gating), rely on off-chain monitoring

Do not add any on-chain cumulative check. Instead, strengthen the existing
`monitoring.md` §4 alerts (`tvl_drop_20pct`, `share_price_decrease`) to
specifically track cumulative decrease attributable to `update_total_assets`
calls (as opposed to withdrawals, which also reduce `TotalAssets` but are a
different, expected code path) and page immediately.

**Pros:** Zero on-chain complexity or new attack surface; no risk of
blocking a legitimate large loss report; monitoring-only changes ship fast
and don't touch the `#[contracterror]` budget.

**Cons:** Provides detection but not containment — an attacker who has
cleared the owner-co-signature bar can still execute the full slow-bleed
before any human responds, since nothing on-chain slows them down. This is
strictly weaker than Option A for the stated goal ("bleeds value slowly
enough to trigger alerts and response" — Option B gets the alert but not the
"slowly enough" part that buys response time).

### Option C — Do nothing beyond the existing per-event cap

Rely on: the existing per-event `max_decrease_bps` cap, mandatory owner
co-signature on every decrease, and the planned owner multi-sig migration
(Issue #607) to make owner-key compromise itself much harder.

**Pros:** Zero implementation cost; avoids the honest-loss-reporting
friction in §3.2 entirely; multi-sig, once shipped, substantially raises the
bar for the exact threat this design targets (a single compromised owner key
co-signing repeated decreases), which may make the residual risk acceptable
without additional contract complexity.

**Cons:** Leaves the repeated-small-decrease pattern (§2.1) fully available
to anyone who *does* clear the co-signature bar (including a malicious owner
acting alone, which multi-sig does mitigate but a threshold-N-of-M scheme
doesn't eliminate if N signers collude or are individually compromised over
time).

---

## 5. Recorded Decision

### Decision: **ACCEPT (Option A) — Implementation Deferred, Tracked as a Follow-Up Issue**

**Rationale:**

1. The threat this design targets (repeated small decreases within existing
   policy, each individually compliant) is real and not addressed by any
   existing control — the per-event cap and owner co-signature both operate
   per-call, with no memory across calls. Option B (monitoring-only) gets
   detection but explicitly not the "slowly enough" containment property the
   issue asks for, so it does not fully satisfy the goal even though it is
   cheaper.
2. The honest-loss-reporting tradeoff (§3) is real and must not be waved
   away — but it is resolved, not avoided, by requiring an explicit
   owner-gated override path (§4.4) for confirmed bad-debt events. This
   mirrors the existing philosophy that decreases already require owner
   co-signature; the override simply adds a second, more visible
   confirmation for the case where the window cap would otherwise block a
   legitimate report.
3. Implementation is deferred behind the same sequencing rationale as
   [`CAP_CHANGE_TIMELOCK_DESIGN.md`](CAP_CHANGE_TIMELOCK_DESIGN.md): the
   owner multi-sig migration (Issue #607) is a broader-impact, higher-priority
   hardening step that reduces the likelihood of the "owner key compromised"
   half of this threat model on its own. Scheduling this rolling cap after
   multi-sig ships lets the implementation account for multi-sig's actual
   auth flow (e.g., should the override require a *separate* multisig
   threshold from the routine co-sign?) rather than building it twice.
4. The `#[contracterror]` 50-variant ceiling (shared constraint, see
   [`LEAST_PRIVILEGE_AGENT.md`](LEAST_PRIVILEGE_AGENT.md) §5 and
   [`CAP_CHANGE_TIMELOCK_DESIGN.md`](CAP_CHANGE_TIMELOCK_DESIGN.md) §4) means
   new error variants for "window cap exceeded" / "override required" need
   to be planned alongside the cap-change-timelock error variants rather
   than independently, to avoid two features separately exhausting the
   budget.

### Parameters (recommended starting point for implementation)

| Parameter | Recommended value | Rationale |
| --------- | ------------------ | --------- |
| Window duration | `AGENT_TIMELOCK_LEDGERS` (17,280 ledgers, ~24h) | Reuse the existing constant; consistent operator mental model across all timelock-adjacent features. |
| Cumulative window cap | `2500` bps (25%) | Meaningfully larger than the existing single-event floor (100 bps) and typical single-event usage (~500-1000 bps per `monitoring.md` examples), so routine operations are unaffected; still bounds worst-case 24h loss to a quarter of TVL rather than unbounded. |
| Approximation method | Decaying counter (§4.1), O(1) storage | Avoids unbounded storage growth; acceptable imprecision for a rate-limiting (not hard-ceiling) control. |
| Override path | New owner-only function/flag, requires the *same* owner auth as routine co-sign at minimum — re-evaluate requiring a stronger auth (e.g. separate multisig threshold) once Issue #607 ships | Must not silently equal "just call it twice" — the override should be a deliberately distinct, loudly-logged action (`AssetsUpdateOverrideEvent` or similar), not a hidden bypass flag indistinguishable from routine decreases in the event log. |
| Override visibility | Distinct event topic from `AssetsUpdatedEvent` | Lets `monitoring.md` alerting treat every override as an automatic page, regardless of the decrease size — the override itself is the signal, not just the magnitude. |

### Conditions for Re-Prioritizing Ahead of the Multi-Sig Migration

- A repeated-small-decrease pattern is actually observed (even on testnet)
  before multi-sig ships.
- A security audit specifically flags the lack of cumulative-decrease
  containment as a finding.
- The team determines multi-sig alone is insufficient mitigation for this
  specific threat (e.g., threshold is low enough that collusion risk remains
  material).

---

## 6. Implementation Plan (for the follow-up issue, once scheduled)

1. **Storage:** Add a decaying-counter state, e.g.
   `DataKey::CumulativeDecreaseTracker { estimate_bps: u32, last_ledger: u32 }`.
2. **Logic in `update_total_assets`:** Before applying a decrease, decay
   `estimate_bps` by elapsed ledgers since `last_ledger` (linear decay to 0
   over `WINDOW_LEDGERS`), add `actual_decrease`'s bps contribution, compare
   against `window_cap_bps`; reject (existing per-event cap check happens
   first, unchanged) unless the override path is used.
3. **New function or flag:** `update_total_assets_with_override` (preferred
   over a boolean flag on the existing function, to keep the override
   maximally visible in call traces and event logs) — owner-only, same
   preconditions as a normal decrease, but bypasses the window-cap check and
   emits a distinct event.
4. **Errors:** Plan new/reused variants alongside the cap-change-timelock
   work to jointly respect the 50-variant ceiling.
5. **Docs:** Add `AssetsUpdateOverrideEvent` to [`EVENTS.md`](../EVENTS.md);
   update [`monitoring.md`](monitoring.md) §4/§5 with a new
   `ALERT: cumulative_decrease_window_exceeded` and
   `ALERT: assets_update_override_used` (always-page severity); update
   [`AGENT_KEY_COMPROMISE_RUNBOOK.md`](AGENT_KEY_COMPROMISE_RUNBOOK.md) with
   the new signal.
6. **Tests:** Mirror the existing `max_decrease_bps` test coverage —
   cumulative-within-window rejection, decay-over-time acceptance, override
   path success and visibility, and the interaction with the existing
   per-event cap (both checks must independently hold; the window cap does
   not loosen the per-event cap).

---

## 7. References

- [`CAP_CHANGE_TIMELOCK_DESIGN.md`](CAP_CHANGE_TIMELOCK_DESIGN.md) — Sibling
  design-decision doc (#590), same deferral rationale and shared error-budget
  constraint.
- [`LEAST_PRIVILEGE_AGENT.md`](LEAST_PRIVILEGE_AGENT.md) — Precedent for this
  document's structure and for the `#[contracterror]` ceiling constraint.
- [`monitoring.md`](monitoring.md) §1, §4, §9 — Existing loss-reporting and
  bad-debt monitoring this design complements.
- [`AGENT_KEY_COMPROMISE_RUNBOOK.md`](AGENT_KEY_COMPROMISE_RUNBOOK.md) —
  Incident response this design is meant to buy time for.
- [`BLEND_INTEGRATION_RESEARCH.md`](BLEND_INTEGRATION_RESEARCH.md) — Context
  on the kind of genuine bad-debt event the override path must accommodate.
- Issue #439 — Circuit-breaker auto-pause; interaction noted in §3.2.
- Issue #607 — Owner multi-sig migration, the higher-priority hardening this
  implementation is sequenced behind.
