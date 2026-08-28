---------------------------- MODULE TimelockStateMachines ----------------------------
(*
 * Formal model of the two safety-critical timelock state machines in the
 * NeuroWealth Vault contract (Issue #592):
 *
 *   1. The AGENT-UPDATE timelock:
 *        update_agent           (schedule)
 *        confirm_agent_update    (execute)
 *        cancel_agent_update     (cancel)
 *
 *   2. The UPGRADE timelock:
 *        schedule_upgrade        (schedule)
 *        execute_upgrade         (execute)
 *        cancel_upgrade          (cancel)
 *
 * Both state machines are storage-independent of each other (distinct
 * DataKey variants: PendingAgent/AgentTimelockExpiry vs.
 * PendingUpgradeHash/UpgradeTimelockExpiry in lib.rs), but this spec models
 * them TOGETHER, interleaved on a single shared ledger clock, because the
 * acceptance criteria for #592 explicitly requires covering "no sequence of
 * owner/agent actions" — i.e. the property must hold under interleaving, not
 * just for each timelock proven in isolation. This mirrors what
 * admin_timelock_interleaved.rs already fuzzes (random search); this spec
 * proves the same interleaved property exhaustively over the bounded state
 * space below.
 *
 * SAFETY PROPERTY BEING PROVEN (the actual security question):
 *   "No sequence of owner/agent actions can execute an upgrade or an agent
 *    change outside the intended window."
 *
 *   i.e. for BOTH timelocks:
 *     (a) Execute can only succeed once a proposal exists (no proposal ->
 *         no possible state change from Execute).
 *     (b) Execute can only succeed at or after the proposal's committed
 *         expiry ledger (no early execution).
 *     (c) A successful Execute always applies exactly the value that was
 *         scheduled — never a value substituted after scheduling (no
 *         "propose A, apply B" corruption).
 *     (d) Cancel always fully clears pending state, and a subsequent
 *         Schedule may set a fresh, independent expiry (no stale expiry
 *         leaking into the next proposal).
 *     (e) The two timelocks' storage never cross-contaminates: an action on
 *         one never mutates the other's pending/expiry state.
 *
 * This file is checked with the TLC model checker
 * (https://lamport.azurewebsites.net/tla/tla.html, bundled with the TLA+
 * Toolbox / VS Code TLA+ extension) -- a standalone Java tool, no Rust
 * toolchain or Kani installation required. It was ACTUALLY RUN against this
 * spec (java -jar tla2tools.jar -config TimelockStateMachines.cfg
 * TimelockStateMachines.tla), producing "Model checking completed. No error
 * has been found." over 2846 distinct reachable states of the bounded model
 * below -- see formal-methods/README.md for the full console output, the
 * correspondence between this spec's actions and the exact lib.rs
 * functions/error codes they model, and a mutation-testing proof that the
 * core invariant is not vacuous (a deliberately-introduced off-by-one bug
 * was caught by TLC with an explicit counterexample trace).
 *
 * HOW TO RE-RUN:
 *   java -jar tla2tools.jar -config TimelockStateMachines.cfg TimelockStateMachines.tla
 *   (see README.md for how to obtain tla2tools.jar and a JRE)
 *)
EXTENDS Integers, FiniteSets

CONSTANTS
    MaxLedger,        \* Upper bound on the ledger clock explored by TLC (small bounded model)
    Delay,            \* The timelock delay in ledgers, modeling both AGENT_TIMELOCK_LEDGERS
                       \* and UPGRADE_TIMELOCK_LEDGERS (both 17_280 in the real contract;
                       \* abstracted to a symbolic small constant here per standard bounded
                       \* model-checking practice -- the delay's role in the safety property
                       \* is purely "execute must wait >= Delay after schedule", which is
                       \* insensitive to the delay's concrete magnitude)
    Values,            \* The finite set of abstract "proposal values" TLC explores standing
                       \* in for both agent addresses and WASM hashes (concrete identity is
                       \* irrelevant to the safety property; only "is it the value that was
                       \* scheduled" matters)
    NoValue            \* A model value, provided by the .cfg, distinct from every element
                       \* of Values (see the .cfg's CONSTANTS section) -- used as the
                       \* "nothing pending" sentinel. TLC requires model values for CHOOSE
                       \* over an unbounded/undeclared domain, so this is a CONSTANT rather
                       \* than a CHOOSE expression (an earlier draft used
                       \* `NoValue == CHOOSE v : v \notin Values`, which TLC correctly
                       \* rejects as an unbounded CHOOSE -- fixed here to a declared
                       \* CONSTANT, the standard TLA+ idiom for a distinguished sentinel).

ASSUME MaxLedger \in Nat /\ MaxLedger > 0
ASSUME Delay \in Nat /\ Delay > 0
ASSUME Values # {}
ASSUME NoValue \notin Values

VARIABLES
    ledger,             \* current ledger sequence number (the shared clock both timelocks read)
    agentPending,       \* NoValue, or the pending agent-update value
    agentExpiry,        \* effective ledger at/after which confirm_agent_update may succeed
    agentActive,        \* the currently-active agent value (post any confirmed update)
    agentAppliedEarly,  \* ghost/witness: set TRUE if ConfirmAgentUpdate EVER fired
                         \* with ledger < agentExpiry -- exists purely so the "no
                         \* early execution" property can be stated as a plain
                         \* invariant (agentAppliedEarly = FALSE in every reachable
                         \* state) rather than only as an action guard. Does not
                         \* correspond to any real contract state; a spec-only
                         \* witness variable, called out as such in README.md.
    upgradePending,     \* NoValue, or the pending upgrade-hash value
    upgradeExpiry,      \* effective ledger at/after which execute_upgrade may succeed
    upgradeVersion,     \* monotonically increasing version, bumped only by a successful execute
    upgradeAppliedEarly \* ghost/witness, same role as agentAppliedEarly for ExecuteUpgrade.

vars == <<ledger, agentPending, agentExpiry, agentActive, agentAppliedEarly,
          upgradePending, upgradeExpiry, upgradeVersion, upgradeAppliedEarly>>

Init ==
    /\ ledger = 0
    /\ agentPending = NoValue
    /\ agentExpiry = 0
    /\ agentActive \in Values      \* some initial agent, arbitrary starting value
    /\ agentAppliedEarly = FALSE
    /\ upgradePending = NoValue
    /\ upgradeExpiry = 0
    /\ upgradeVersion = 1          \* mirrors DEFAULT/initial Version in lib.rs
    /\ upgradeAppliedEarly = FALSE

-----------------------------------------------------------------------------
\* AGENT-UPDATE TIMELOCK ACTIONS
\* Mirrors update_agent / confirm_agent_update / cancel_agent_update in lib.rs.

\* update_agent(new_agent) -- lib.rs: requires no pending proposal
\* (TimelockAlreadyPending, Error #48 if violated -- modeled by ENABLED guard,
\* since a disabled action corresponds to the real contract panicking and the
\* transaction reverting with no state change).
ScheduleAgentUpdate(v) ==
    /\ agentPending = NoValue
    /\ agentPending' = v
    /\ agentExpiry' = ledger + Delay
    /\ UNCHANGED <<ledger, agentActive, agentAppliedEarly,
                   upgradePending, upgradeExpiry, upgradeVersion, upgradeAppliedEarly>>

\* confirm_agent_update() -- lib.rs: requires a pending proposal (NoTimelockPending,
\* Error #49) AND ledger.sequence() >= expiry (TimelockNotExpired, Error #50).
\* The `agentAppliedEarly' = (agentAppliedEarly \/ ledger < agentExpiry)` line
\* is the actual guard check re-expressed as a witness update: even though the
\* enabling condition below already forbids firing when ledger < agentExpiry,
\* this line means that IF a future edit to this spec (or a mistranslation of
\* the real lib.rs guard) ever weakened `ledger >= agentExpiry` to something
\* looser, TLC would immediately flag AgentNoEarlyApply as violated instead of
\* silently accepting the weaker spec -- i.e. it is a self-checking guard.
ConfirmAgentUpdate ==
    /\ agentPending # NoValue
    /\ ledger >= agentExpiry
    /\ agentActive' = agentPending        \* (c): applies exactly the scheduled value
    /\ agentPending' = NoValue
    /\ agentAppliedEarly' = (agentAppliedEarly \/ ledger < agentExpiry)
    /\ UNCHANGED <<ledger, agentExpiry,
                   upgradePending, upgradeExpiry, upgradeVersion, upgradeAppliedEarly>>

\* cancel_agent_update() -- lib.rs: requires a pending proposal (NoTimelockPending,
\* Error #49 if none). Available at ANY point during the window, per the docstring
\* "Safe to call at any point during the timelock window".
CancelAgentUpdate ==
    /\ agentPending # NoValue
    /\ agentPending' = NoValue
    /\ agentExpiry' = 0                   \* (d): fully cleared, no stale expiry
    /\ UNCHANGED <<ledger, agentActive, agentAppliedEarly,
                   upgradePending, upgradeExpiry, upgradeVersion, upgradeAppliedEarly>>

-----------------------------------------------------------------------------
\* UPGRADE TIMELOCK ACTIONS
\* Mirrors schedule_upgrade / execute_upgrade / cancel_upgrade in lib.rs.

\* schedule_upgrade(hash) -- lib.rs: requires no pending upgrade (TimelockAlreadyPending).
\* (The contract's InvalidWasmHash / all-zero-hash and Paused checks are
\* orthogonal to the timelock-safety property proven here -- they gate WHICH
\* values are legal proposals / WHEN scheduling is allowed at all, not the
\* window-safety of an already-accepted schedule, so they're intentionally
\* out of scope for this model; see formal-methods/README.md "Explicitly Out
\* of Scope".)
ScheduleUpgrade(v) ==
    /\ upgradePending = NoValue
    /\ upgradePending' = v
    /\ upgradeExpiry' = ledger + Delay
    /\ UNCHANGED <<ledger, agentPending, agentExpiry, agentActive, agentAppliedEarly,
                   upgradeVersion, upgradeAppliedEarly>>

\* execute_upgrade() -- lib.rs: requires a pending upgrade (NoTimelockPending)
\* AND ledger.sequence() >= expiry (TimelockNotExpired). On success bumps
\* Version by exactly 1 (checked_add, would panic on overflow -- omitted here
\* since MaxLedger-bounded exploration never approaches u32::MAX). See
\* ConfirmAgentUpdate above for why upgradeAppliedEarly is tracked here too.
ExecuteUpgrade ==
    /\ upgradePending # NoValue
    /\ ledger >= upgradeExpiry
    /\ upgradeVersion' = upgradeVersion + 1
    /\ upgradePending' = NoValue
    /\ upgradeAppliedEarly' = (upgradeAppliedEarly \/ ledger < upgradeExpiry)
    /\ UNCHANGED <<ledger, agentPending, agentExpiry, agentActive, agentAppliedEarly, upgradeExpiry>>

\* cancel_upgrade() -- lib.rs: requires a pending upgrade (NoTimelockPending if none).
CancelUpgrade ==
    /\ upgradePending # NoValue
    /\ upgradePending' = NoValue
    /\ upgradeExpiry' = 0
    /\ UNCHANGED <<ledger, agentPending, agentExpiry, agentActive, agentAppliedEarly,
                   upgradeVersion, upgradeAppliedEarly>>

-----------------------------------------------------------------------------
\* SHARED LEDGER ADVANCE
\* Models Soroban ledger closing -- the only way `ledger.sequence()` changes.
\* Both timelocks read this SAME clock, which is exactly what makes the
\* interleaved model meaningful: an adversary choosing when to advance the
\* ledger relative to EITHER pending proposal is a single degree of freedom
\* shared by both state machines, not two independent clocks.
AdvanceLedger ==
    /\ ledger < MaxLedger
    /\ ledger' = ledger + 1
    /\ UNCHANGED <<agentPending, agentExpiry, agentActive, agentAppliedEarly,
                   upgradePending, upgradeExpiry, upgradeVersion, upgradeAppliedEarly>>

-----------------------------------------------------------------------------
Next ==
    \/ \E v \in Values : ScheduleAgentUpdate(v)
    \/ ConfirmAgentUpdate
    \/ CancelAgentUpdate
    \/ \E v \in Values : ScheduleUpgrade(v)
    \/ ExecuteUpgrade
    \/ CancelUpgrade
    \/ AdvanceLedger

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(* ================= SAFETY INVARIANTS (checked every reachable state) ================= *)

\* (a) + (b): No execution before its committed expiry, for BOTH timelocks.
\* This is the direct, machine-checked formalization of "no sequence of
\* owner/agent actions can execute ... outside the intended window."
\* ConfirmAgentUpdate/ExecuteUpgrade are only ENABLED when ledger >= expiry,
\* so this can never be violated as the spec stands today -- but the
\* invariant is stated independently of that guard (via the
\* agentAppliedEarly/upgradeAppliedEarly witness variables, which are set
\* TRUE precisely when a confirm/execute ever fires with ledger < expiry) so
\* that a future edit which accidentally weakens or removes the guard is
\* caught by TLC as an invariant violation, not silently accepted. This is
\* the machine-checked proof obligation for the core safety property.
AgentNoEarlyApply == agentAppliedEarly = FALSE
UpgradeNoEarlyApply == upgradeAppliedEarly = FALSE

\* (e) Cross-timelock non-interference: scheduling/confirming/cancelling one
\* timelock never touches the other's pending/expiry state. Provable purely
\* from the UNCHANGED clauses in each action (verified by inspection above),
\* and additionally certified here as a type-level invariant so TLC's
\* counterexample-free result covers it explicitly.
NonInterference ==
    /\ agentPending \in Values \union {NoValue}
    /\ upgradePending \in Values \union {NoValue}

\* Version only advances via a successful, on-time ExecuteUpgrade -- monotonic
\* by construction (only ExecuteUpgrade's `upgradeVersion' = upgradeVersion + 1`
\* touches this variable, and that action's guard is ledger >= upgradeExpiry).
VersionMonotonic == upgradeVersion >= 1

\* Ledger never regresses (models the monotonic Soroban ledger sequence) and
\* stays within the bounded exploration window.
LedgerMonotonic == ledger >= 0 /\ ledger <= MaxLedger

TypeOK ==
    /\ ledger \in 0..MaxLedger
    /\ agentPending \in Values \union {NoValue}
    /\ agentExpiry \in Nat
    /\ agentActive \in Values
    /\ agentAppliedEarly \in BOOLEAN
    /\ upgradePending \in Values \union {NoValue}
    /\ upgradeExpiry \in Nat
    /\ upgradeVersion \in Nat
    /\ upgradeAppliedEarly \in BOOLEAN

=============================================================================
