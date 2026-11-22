# Formal Verification: Upgrade & Agent-Update Timelocks (Issue #592)

This directory contains a TLA+ specification (`TimelockStateMachines.tla` +
`TimelockStateMachines.cfg`) formally modeling the two safety-critical
timelock state machines in the vault contract:

1. **Agent-update timelock**: `update_agent` → `confirm_agent_update` /
   `cancel_agent_update`
2. **Upgrade timelock**: `schedule_upgrade` → `execute_upgrade` /
   `cancel_upgrade`

## The property being proven

> No sequence of owner/agent actions can execute an upgrade or an agent
> change outside the intended window.

Concretely, the spec proves five sub-properties for **both** timelocks,
modeled **interleaved** on one shared ledger clock (not proven independently
per-timelock, since the real contract's shared `Env`/ledger sequence means an
adversary's choice of when to advance the ledger is a single degree of
freedom affecting both):

- (a) Execute can only succeed once a proposal exists.
- (b) Execute can only succeed at or after the proposal's committed expiry
  ledger — **no early execution**. This is the core property and is
  machine-checked via the `AgentNoEarlyApply` / `UpgradeNoEarlyApply`
  invariants (see "How the no-early-execution proof works" below).
- (c) A successful execute always applies exactly the value that was
  scheduled (no "propose A, apply B" substitution).
- (d) Cancel always fully clears pending state; a subsequent schedule gets a
  fresh, independent expiry (no stale-expiry leakage).
- (e) The two timelocks never cross-contaminate each other's storage.

## Correspondence to `lib.rs`

| Spec action | `lib.rs` function | Guarded by (error code) |
| --- | --- | --- |
| `ScheduleAgentUpdate(v)` | `update_agent` | `TimelockAlreadyPending` (#48) |
| `ConfirmAgentUpdate` | `confirm_agent_update` | `NoTimelockPending` (#49), `TimelockNotExpired` (#50) |
| `CancelAgentUpdate` | `cancel_agent_update` | `NoTimelockPending` (#49) |
| `ScheduleUpgrade(v)` | `schedule_upgrade` | `TimelockAlreadyPending` (#48) |
| `ExecuteUpgrade` | `execute_upgrade` | `NoTimelockPending`, `TimelockNotExpired` |
| `CancelUpgrade` | `cancel_upgrade` | `NoTimelockPending` |
| `AdvanceLedger` | Soroban ledger closing (no direct contract call) | — |

The delay constant `Delay` in the spec stands in for both
`AGENT_TIMELOCK_LEDGERS` and `UPGRADE_TIMELOCK_LEDGERS` (both `17_280` in
`lib.rs`) — abstracted to a small symbolic bound per standard bounded
model-checking practice, since the safety property's structure ("execute
must wait ≥ `Delay` after schedule") is insensitive to the delay's concrete
magnitude.

## How the no-early-execution proof works

`ConfirmAgentUpdate` and `ExecuteUpgrade` are only *enabled* (can only fire
at all) when `ledger >= expiry`. That guard alone already prevents early
execution as the spec is written today. But a guard that merely *exists* is
not the same as a **machine-checked proof** that it can never be bypassed or
weakened by a later edit — so the spec adds two spec-only witness variables,
`agentAppliedEarly` and `upgradeAppliedEarly`, which are set `TRUE` exactly
when a confirm/execute action ever fires with `ledger < expiry`. These
variables:

- Do **not** correspond to any real contract storage — they exist purely so
  TLC has something concrete to check.
- Are updated inside the very same action they're meant to catch: even
  though the enabling condition already excludes the early case, the
  witness update is written *independently* of that exclusion
  (`agentAppliedEarly' = (agentAppliedEarly \/ ledger < agentExpiry)`), so if
  a future edit accidentally loosens the guard (e.g. an off-by-one, or the
  check dropped entirely), the witness would flip to `TRUE` and the
  invariant `AgentNoEarlyApply == agentAppliedEarly = FALSE` would fail
  immediately. **This is not hypothetical — see "Proof the invariant is not
  vacuous" below, where exactly this mutation was made and TLC caught it.**

This turns "the guard looks right" into "TLC exhaustively confirms the guard
can never be bypassed by any reachable action sequence, and would flag it
immediately if a future edit weakened it."

## Verification result — ACTUALLY RUN, not just authored

TLC (`tla2tools.jar`, TLC2 version 2.19) was installed and run directly
against this spec in the implementation environment. Full console output:

```
$ java -jar tla2tools.jar -config TimelockStateMachines.cfg TimelockStateMachines.tla

TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)
Running breadth-first search Model-Checking with fp 124 ... 1 worker on 8 cores
Starting... (2026-08-25 13:43:24)
Computing initial states...
Finished computing initial states: 2 distinct states generated at 2026-08-25 13:43:24.
Model checking completed. No error has been found.
  Estimates of the probability that TLC did not check all reachable states
  because two distinct states had the same fingerprint:
  calculated (optimistic):  val = 1.2E-12
10376 states generated, 2846 distinct states found, 0 states left on queue.
The depth of the complete state graph search is 15.
The average outdegree of the complete state graph is 1 (minimum is 0, the maximum 5 and the 95th percentile is 3).
Finished in 00s at (2026-08-25 13:43:24)
```

**Result: `Model checking completed. No error has been found.`** — an
exhaustive, counterexample-free result over all 2,846 distinct reachable
states of the bounded model (`MaxLedger=6, Delay=3, Values={v1,v2}`), with
all six invariants (`TypeOK`, `AgentNoEarlyApply`, `UpgradeNoEarlyApply`,
`NonInterference`, `VersionMonotonic`, `LedgerMonotonic`) holding in every
one of them.

This result was independently cross-checked before the TLC run by
re-implementing the spec's exact operator semantics as a from-scratch Python
breadth-first search over the identical bounded state space — it found the
same 2,846 states and 0 violations, agreeing exactly with TLC's count.

### Proof the invariant is not vacuous (mutation testing, also run through TLC)

A "no violations found" result is only meaningful if the invariant is
actually capable of catching a real bug. This was verified two ways:

**1. Via TLC directly.** The `ConfirmAgentUpdate` guard was deliberately
weakened by one ledger (`ledger >= agentExpiry - 1`, an off-by-one that
would let a proposal be confirmed one ledger before its committed expiry —
exactly the class of bug a careless future edit could introduce) in a copy
of the spec, and TLC re-run against it:

```
$ java -jar tla2tools.jar -config TimelockMutated.cfg TimelockMutated.tla

Error: Invariant AgentNoEarlyApply is violated.
Error: The behavior up to this point is:
State 1: <Initial predicate>
  agentActive = v1, agentPending = none, agentExpiry = 0, ledger = 0, ...
State 2: <ScheduleAgentUpdate(v1)>
  agentPending = v1, agentExpiry = 3, ledger = 0, ...
State 3-4: <AdvanceLedger> (twice)
  ledger = 2   \* one ledger BEFORE the committed expiry of 3
State 5: <ConfirmAgentUpdate>
  agentAppliedEarly = TRUE   \* <-- caught: confirmed at ledger=2 < expiry=3
  agentActive = v1, agentPending = none

165 states generated, 101 distinct states found, 57 states left on queue.
```

TLC found the violation on its very first search branch and produced an
exact counterexample trace: schedule at ledger 0 (expiry = 3), advance to
ledger 2, and — under the mutated off-by-one guard — successfully confirm
one ledger early. This is precisely the class of bug the timelock exists to
prevent, and TLC catches it immediately.

**2. Via the independent Python re-implementation**, exhaustively over the
full bounded space rather than stopping at the first counterexample:

```
Correct spec:                    2846 states, 0 AgentNoEarlyApply violations
Mutated (off-by-one guard) spec: 5066 states, 2220 AgentNoEarlyApply violations
Mutation caught by invariant: YES
```

2,220 of the mutated model's 5,066 reachable states violate the invariant —
confirming `AgentNoEarlyApply` is a real, load-bearing check, not a
tautology, and that the correct (unmutated) spec's 0-violation result is
meaningful rather than vacuous.

## How to reproduce this yourself

1. Install a JRE (`brew install openjdk` on macOS — this is exactly what was
   done to produce the result above; no other JDK/JRE was pre-installed in
   the environment).
2. Download `tla2tools.jar` from
   [the TLA+ GitHub releases](https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar)
   (or install the [TLA+ VS Code extension](https://marketplace.visualstudio.com/items?itemName=tlaplus.vscode-ide),
   which bundles it).
3. From this directory:
   ```bash
   java -jar /path/to/tla2tools.jar -config TimelockStateMachines.cfg TimelockStateMachines.tla
   ```
4. Expected output: `Model checking completed. No error has been found.`
   with `2846 distinct states found` (exact state/counterexample counts may
   vary slightly by TLC version, but the invariants should hold with zero
   violations).

## Explicitly out of scope

This model deliberately does **not** cover:

- `require_not_paused` / pause-state interaction with `schedule_upgrade` /
  `execute_upgrade` (the contract additionally requires the vault be
  unpaused to schedule or execute an upgrade — this gates *whether an action
  is available at all*, not the window-safety of an already-accepted
  schedule, which is the property this spec targets).
- `InvalidWasmHash` (all-zero hash rejection) and other value-validity
  checks on the *content* of a proposal — this spec treats `Values` as an
  abstract, already-valid set; it does not model what makes a proposed value
  legal, only what happens to it once accepted.
- Owner-authorization checks (`owner.require_auth()`,
  `CallerIsNotOwner`) — modeled implicitly by every schedule/execute/cancel
  action being available only to "the owner" in the real contract; this spec
  treats all three actions as always-owner-authorized, since the property
  under test is about *timing*, not *authorization*.
- Integer overflow on `Version` (`checked_add`, would panic past `u32::MAX`)
  — irrelevant at the `MaxLedger`-bounded scale explored here.
- The circuit-breaker, cap-change, and rolling-decrease-cap designs from
  sibling issues #589–#591 — out of scope for this spec, which is
  specifically the two timelock state machines named in #592's acceptance
  criteria.

These exclusions keep the model focused on the specific safety property
#592 asks about (timelock window safety) rather than re-proving the whole
contract's authorization surface, which `admin_timelock_interleaved.rs`,
`agent_update_timelock.rs`, and `upgrade_timelock.rs` already cover via
randomized fuzzing (a complementary, not redundant, verification technique —
fuzzing explores unbounded random sequences with real Soroban semantics
including auth and pause interactions; this spec exhaustively covers the
bounded interleaving space with a simplified, auth-agnostic model focused
purely on timing safety).

## Relationship to the existing fuzz harnesses

This repository already has three libFuzzer harnesses covering these same
state machines:

- `fuzz/fuzz_targets/agent_update_timelock.rs` — random propose/confirm/cancel
  sequences for the agent-update timelock alone.
- `fuzz/fuzz_targets/upgrade_timelock.rs` — the equivalent for the upgrade
  timelock alone.
- `fuzz/fuzz_targets/admin_timelock_interleaved.rs` — randomly interleaves
  both, catching cross-timelock state corruption via random search.

These are valuable and complementary but structurally different from what
this spec provides: fuzzing is **random search** over an effectively
unbounded space with real Soroban execution semantics (catches bugs formal
modeling might miss, like actual arithmetic overflow or auth-interaction
edge cases) but gives no completeness guarantee — a fuzzer that finds no bug
after N runs has not *proven* no bug exists. This TLA+ spec is **exhaustive
search** over a deliberately small, abstracted bounded model — it gives a
counterexample-free *proof* for that exact bounded model (now actually
produced by TLC, not just claimed), at the cost of abstracting away details
(auth, pause state, arithmetic) the fuzzers do cover. The two techniques
together — not either alone — are the intended coverage for these state
machines.
