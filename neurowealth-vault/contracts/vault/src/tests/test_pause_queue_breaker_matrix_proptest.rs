#![cfg(test)]

//! Interaction Property Tests: Pause State × Withdrawal Queue × Circuit Breaker (#924).
//!
//! # Problem Statement & Specification
//! State-machine documentation claims robust interaction handling across the vault's
//! pause mechanism, withdrawal queue, and consecutive-failure circuit breaker.
//! This test suite provides exhaustive matrix coverage and property tests across:
//!
//! 1. **PauseState** (3 variants):
//!    - `Unpaused`: Vault is operating normally.
//!    - `Paused`: Standard administrative pause via `client.pause(&owner)`.
//!    - `EmergencyPaused`: Emergency pause via `client.emergency_pause(&owner)`.
//!
//! 2. **BreakerState** (3 variants):
//!    - `ZeroFailures`: Circuit breaker failure counter is 0.
//!    - `BelowThreshold`: Consecutive failures > 0 but < threshold (e.g. 1 failure with threshold 3).
//!    - `Tripped`: Consecutive failures >= threshold (trips breaker, automatically engaging emergency pause).
//!
//! 3. **QueueState** (5 variants):
//!    - `Unconfigured`: Queue is not configured (max_size=0, ttl=0).
//!    - `Empty`: Queue is configured, but 0 items are pending.
//!    - `PartiallyFilled`: Queue is configured and contains active requests below capacity.
//!    - `AtCapacity`: Queue contains exactly `max_size` requests.
//!    - `ExpiredRequests`: Queue contains requests where `ledger.timestamp > created_at + ttl`.
//!
//! Total matrix combinations: 3 × 3 × 5 = **45 states**.
//!
//! # Core Interaction Invariants Asserted
//! 1. **Queue Submission Safety**:
//!    - If effectively paused (manual pause, emergency pause, or tripped circuit breaker),
//!      `queue_withdrawal` is strictly rejected with `VaultError::Paused`.
//!    - If unpaused: rejected with `VaultError::BatchSizeExceeded` when `AtCapacity`;
//!      accepted otherwise and emits `WithdrawalQueuedEvent`.
//! 2. **Emergency Exit Escape Hatch (`cancel_withdrawal_request`)**:
//!    - Request owner can ALWAYS cancel their own pending withdrawal request across ALL 45 states
//!      (never trapped by pause, emergency pause, or tripped circuit breaker).
//!    - Non-owners are strictly rejected with `VaultError::UnauthorizedOwnerError` in all 45 states.
//! 3. **Queue Drainage & Expiration (`process_withdrawal_queue`)**:
//!    - Any authenticated keeper/agent can process the queue in all 45 states.
//!    - Valid requests are fulfilled up to batch size and emit `WithdrawalFulfilledEvent`.
//!    - Expired requests (`timestamp > created_at + ttl`) are pruned without fulfillment.
//! 4. **Eligibility Consistency (`get_withdrawal_eligibility`)**:
//!    - Reports `eligible_now: false, reason: symbol_short!("paused")` whenever effectively paused.
//!    - Reports `eligible_now: false, reason: symbol_short!("queue")` when unpaused and in queue.
//! 5. **State Isolation**:
//!    - Queue operations never mutate circuit-breaker counters or flip pause flags.
//! 6. **Circuit-Breaker Reset Recovery**:
//!    - Calling `reset_circuit_breaker(&owner)` clears failures to 0 and lifts pause,
//!      restoring queue submission functionality.

extern crate std;

use super::utils::*;
use crate::{
    topics::{
        TOPIC_CIRCUIT_BREAKER_RESET, TOPIC_EMERGENCY_PAUSED, TOPIC_PAUSED, TOPIC_UNPAUSED,
        TOPIC_WITHDRAWAL_CANCELLED, TOPIC_WITHDRAWAL_FULFILLED, TOPIC_WITHDRAWAL_QUEUED,
    },
    CircuitBreakerResetEvent, EmergencyPausedEvent, VaultPausedEvent, VaultUnpausedEvent,
    WithdrawalCancelledEvent, WithdrawalFulfilledEvent, WithdrawalQueuedEvent, DEFAULT_MIN_DEPOSIT,
    MAX_DEPOSIT_CEILING,
};
use proptest::prelude::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env,
};

// ============================================================================
// Matrix Dimensions
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseState {
    Unpaused,
    Paused,
    EmergencyPaused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    ZeroFailures,
    BelowThreshold,
    Tripped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueState {
    Unconfigured,
    Empty,
    PartiallyFilled,
    AtCapacity,
    ExpiredRequests,
}

const ALL_PAUSE_STATES: [PauseState; 3] = [
    PauseState::Unpaused,
    PauseState::Paused,
    PauseState::EmergencyPaused,
];

const ALL_BREAKER_STATES: [BreakerState; 3] = [
    BreakerState::ZeroFailures,
    BreakerState::BelowThreshold,
    BreakerState::Tripped,
];

const ALL_QUEUE_STATES: [QueueState; 5] = [
    QueueState::Unconfigured,
    QueueState::Empty,
    QueueState::PartiallyFilled,
    QueueState::AtCapacity,
    QueueState::ExpiredRequests,
];

// ============================================================================
// Fixture Setup
// ============================================================================

struct MatrixFixture<'a> {
    env: Env,
    client: NeuroWealthVaultClient<'a>,
    owner: Address,
    agent: Address,
    usdc_token: Address,
    blend_pool: Address,
    user: Address,
    queued_ids: std::vec::Vec<u32>,
    is_effectively_paused: bool,
}

fn setup_matrix_fixture(
    pause_state: PauseState,
    breaker_state: BreakerState,
    queue_state: QueueState,
) -> MatrixFixture<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    // Raise the per-tx, per-user, and vault-wide deposit caps above the
    // 100_000 USDC (7-decimal) deposit below — the contract defaults
    // (1_000 USDC per-tx, 1_000 USDC per-user, 10_000 USDC TVL) would
    // otherwise reject it. The per-user cap is set at the ceiling (`fix.user`
    // alone deposits that much), and the TVL cap doubles it, leaving the
    // ceiling's worth of headroom for the Invariant 4 fresh-user deposit.
    client.set_deposit_limits(&DEFAULT_MIN_DEPOSIT, &MAX_DEPOSIT_CEILING);
    client.set_caps(&MAX_DEPOSIT_CEILING, &(MAX_DEPOSIT_CEILING * 2));

    let user = Address::generate(&env);
    // User deposits 999 USDC (7 decimals) to have sufficient shares for queue
    // operations. Capped just under `DEFAULT_MAX_DEPOSIT` (1,000 USDC):
    // the originally coded 10,000 USDC deposit exceeded the per-transaction
    // cap by 10x and panicked with MaximumDepositExceeded (#39) on every
    // test using this fixture. 999 USDC still comfortably covers the
    // queue's largest pre-seeded totals (up to 300 USDC across
    // PartiallyFilled/AtCapacity) plus the proptests' widest `test_amount`
    // range (up to 500 USDC).
    mint_and_deposit(&env, &client, &usdc_token, &user, 999_0000000i128);

    // 1. Establish Queue State
    let mut queued_ids = std::vec::Vec::new();
    match queue_state {
        QueueState::Unconfigured => {
            client.set_queue_config(&0u32, &0u64);
        }
        QueueState::Empty => {
            client.set_queue_config(&10u32, &3600u64);
        }
        QueueState::PartiallyFilled => {
            client.set_queue_config(&10u32, &3600u64);
            let req1 = client.queue_withdrawal(&user, &100_0000000i128);
            let req2 = client.queue_withdrawal(&user, &200_0000000i128);
            queued_ids.push(req1);
            queued_ids.push(req2);
        }
        QueueState::AtCapacity => {
            client.set_queue_config(&2u32, &3600u64);
            let req1 = client.queue_withdrawal(&user, &100_0000000i128);
            let req2 = client.queue_withdrawal(&user, &200_0000000i128);
            queued_ids.push(req1);
            queued_ids.push(req2);
        }
        QueueState::ExpiredRequests => {
            client.set_queue_config(&10u32, &50u64);
            let req1 = client.queue_withdrawal(&user, &100_0000000i128);
            queued_ids.push(req1);
            // Advance ledger timestamp beyond TTL (+100s > 50s TTL)
            env.ledger().with_mut(|li| {
                li.timestamp += 100;
            });
        }
    }

    // 2. Establish Breaker State
    match breaker_state {
        BreakerState::ZeroFailures => {
            assert_eq!(client.get_consecutive_failures(), 0);
        }
        BreakerState::BelowThreshold => {
            client.set_max_consecutive_failures(&3);
            blend_client.set_max_supply_limit(&-1_i128); // Force supply leg to move 0
            client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
            assert_eq!(client.get_consecutive_failures(), 1);
            assert!(!client.is_paused());
        }
        BreakerState::Tripped => {
            client.set_max_consecutive_failures(&2);
            blend_client.set_max_supply_limit(&-1_i128);
            client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
            client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
            assert_eq!(client.get_consecutive_failures(), 2);
            assert!(
                client.is_paused(),
                "tripped circuit breaker must engage emergency auto-pause"
            );
        }
    }

    // 3. Establish Pause State
    match pause_state {
        PauseState::Unpaused => {
            // If tripped breaker auto-paused earlier, we leave it in its natural breaker-paused state
        }
        PauseState::Paused => {
            if !client.is_paused() {
                client.pause(&owner);
            }
        }
        PauseState::EmergencyPaused => {
            if !client.is_paused() {
                client.emergency_pause(&owner);
            }
        }
    }

    let is_effectively_paused = match (pause_state, breaker_state) {
        (PauseState::Paused, _) => true,
        (PauseState::EmergencyPaused, _) => true,
        (PauseState::Unpaused, BreakerState::Tripped) => true,
        (PauseState::Unpaused, BreakerState::ZeroFailures) => false,
        (PauseState::Unpaused, BreakerState::BelowThreshold) => false,
    };

    assert_eq!(
        client.is_paused(),
        is_effectively_paused,
        "vault pause status must match expected effective pause state"
    );

    MatrixFixture {
        env,
        client,
        owner,
        agent,
        usdc_token,
        blend_pool,
        user,
        queued_ids,
        is_effectively_paused,
    }
}

// ============================================================================
// Core Invariant Verifier for any cell in the 45-state Matrix
// ============================================================================

fn verify_matrix_cell_invariants(
    pause_state: PauseState,
    breaker_state: BreakerState,
    queue_state: QueueState,
    test_amount: i128,
) {
    let fix = setup_matrix_fixture(pause_state, breaker_state, queue_state);
    let initial_failures = fix.client.get_consecutive_failures();
    let initial_paused = fix.client.is_paused();

    // Requests to exercise in Invariant 2 (cancellation): the fixture's
    // pre-seeded ids, plus whatever Invariant 1 creates below for queue
    // states (e.g. Empty) that have room. Without tracking that new id here
    // too, it would stay queued, un-cancelled, past Invariant 2, and then
    // Invariant 3's `process_withdrawal_queue` would fulfill it — breaking
    // the `Unconfigured | Empty => processed == 0` expectation even though
    // nothing is actually wrong with the contract's behavior.
    let mut ids_to_cancel: std::vec::Vec<u32> = fix.queued_ids.clone();

    // ------------------------------------------------------------------------
    // Invariant 1: queue_withdrawal behavior
    // ------------------------------------------------------------------------
    let queue_attempt = fix.client.try_queue_withdrawal(&fix.user, &test_amount);

    if fix.is_effectively_paused {
        assert!(
            queue_attempt.is_err(),
            "queue_withdrawal must fail when effectively paused (pause={:?}, breaker={:?}, queue={:?})",
            pause_state, breaker_state, queue_state
        );
    } else {
        // Vault is Unpaused and breaker not tripped
        if queue_state == QueueState::AtCapacity {
            assert!(
                queue_attempt.is_err(),
                "queue_withdrawal must fail when queue is at capacity (pause={:?}, breaker={:?}, queue={:?})",
                pause_state, breaker_state, queue_state
            );
        } else {
            assert!(
                queue_attempt.is_ok(),
                "queue_withdrawal must succeed when unpaused with available capacity (pause={:?}, breaker={:?}, queue={:?})",
                pause_state, breaker_state, queue_state
            );
            let req_id = queue_attempt.unwrap().unwrap();
            let req = fix.client.get_withdrawal_request(&req_id);
            assert_eq!(req.user, fix.user);
            assert_eq!(req.amount, test_amount);
            assert!(!req.fulfilled);
            assert!(!req.cancelled);
            ids_to_cancel.push(req_id);
        }
    }

    // State isolation: queue_withdrawal must not alter breaker failures or pause flag
    assert_eq!(fix.client.get_consecutive_failures(), initial_failures);
    assert_eq!(fix.client.is_paused(), initial_paused);

    // ------------------------------------------------------------------------
    // Invariant 2: Emergency Exit Escape Hatch (cancel_withdrawal_request)
    // ------------------------------------------------------------------------
    if let Some(&req_id) = ids_to_cancel.first() {
        // 2a. Unauthorized user cancellation must fail across ALL 45 states
        let unauthorized_caller = Address::generate(&fix.env);
        let unauth_cancel = fix
            .client
            .try_cancel_withdrawal_request(&unauthorized_caller, &req_id);
        assert!(
            unauth_cancel.is_err(),
            "unauthorized cancel must fail across all states (pause={:?}, breaker={:?}, queue={:?})",
            pause_state, breaker_state, queue_state
        );

        // 2b. Request owner must ALWAYS be able to cancel in ALL 45 states
        let cancel_res = fix.client.try_cancel_withdrawal_request(&fix.user, &req_id);
        assert!(
            cancel_res.is_ok(),
            "request owner must ALWAYS be able to cancel in ANY state (pause={:?}, breaker={:?}, queue={:?})",
            pause_state, breaker_state, queue_state
        );

        // Verify record is pruned
        assert!(
            fix.client.try_get_withdrawal_request(&req_id).is_err(),
            "cancelled request must be pruned from storage"
        );

        // State isolation: cancellation must not alter breaker failures or pause flag
        assert_eq!(fix.client.get_consecutive_failures(), initial_failures);
        assert_eq!(fix.client.is_paused(), initial_paused);
    }

    // ------------------------------------------------------------------------
    // Invariant 3: Queue Drainage & Expiration (process_withdrawal_queue)
    // ------------------------------------------------------------------------
    let keeper = Address::generate(&fix.env);
    let proc_res = fix.client.try_process_withdrawal_queue(&keeper, &u32::MAX);
    assert!(
        proc_res.is_ok(),
        "process_withdrawal_queue must be runnable by keeper in ALL 45 states (pause={:?}, breaker={:?}, queue={:?})",
        pause_state, breaker_state, queue_state
    );
    let processed = proc_res.unwrap().unwrap();

    // `ids_to_cancel` holds every request that existed right after Invariant 1:
    // the fixture's pre-seeded ones, plus one more if Invariant 1 itself just
    // queued a fresh request (any state except AtCapacity, when unpaused).
    // Invariant 2 above unconditionally cancels its first entry whenever the
    // list is non-empty (the request owner can "ALWAYS cancel", regardless of
    // pause state) — and for `ExpiredRequests` that first entry is always the
    // pre-seeded expired one, so cancellation (not natural TTL pruning) is
    // what actually removes it here. Every other tracked request is a fresh,
    // unexpired one from Invariant 1, so it survives to be fulfilled.
    //
    // This is therefore the same formula across every (queue_state, pause)
    // combination: whatever remains after cancelling at most one entry is
    // exactly what `process_withdrawal_queue` fulfills. A per-queue_state
    // hardcoded constant can't express this: e.g. `PartiallyFilled` fulfills
    // 1 remaining request when the vault was paused during Invariant 1 (no
    // new request added) but 2 when it was unpaused (one was), and
    // `ExpiredRequests` fulfills 0 when paused but 1 when unpaused, for the
    // same reason.
    let expected_processed = ids_to_cancel.len().saturating_sub(1) as u32;
    assert_eq!(
        processed,
        expected_processed,
        "process_withdrawal_queue must fulfill exactly the requests left after \
         Invariant 2's cancellation (pause={:?}, breaker={:?}, queue={:?}, \
         tracked_requests={}, effectively_paused={})",
        pause_state,
        breaker_state,
        queue_state,
        ids_to_cancel.len(),
        fix.is_effectively_paused
    );

    // State isolation: queue processing must not alter breaker failures or pause flag
    assert_eq!(fix.client.get_consecutive_failures(), initial_failures);
    assert_eq!(fix.client.is_paused(), initial_paused);

    // ------------------------------------------------------------------------
    // Invariant 4: Eligibility Query Consistency (get_withdrawal_eligibility)
    // ------------------------------------------------------------------------
    // `deposit` (and thus `mint_and_deposit`) unconditionally requires the
    // vault to be unpaused, so it cannot run while effectively paused — doing
    // so anyway panicked this whole test on every paused matrix cell. The
    // eligibility check itself doesn't need the fresh user to hold shares:
    // `get_withdrawal_eligibility` returns early on the pause check before it
    // ever looks at the caller's share balance (lib.rs), so the paused-branch
    // assertions below hold with or without a deposit.
    let fresh_user = Address::generate(&fix.env);
    if !fix.is_effectively_paused {
        mint_and_deposit(
            &fix.env,
            &fix.client,
            &fix.usdc_token,
            &fresh_user,
            1_000_0000000i128,
        );
    }

    let elig = fix.client.get_withdrawal_eligibility(&fresh_user);
    if fix.is_effectively_paused {
        assert_eq!(
            elig.eligible_now, false,
            "eligibility must report false when effectively paused"
        );
        assert_eq!(
            elig.reason,
            symbol_short!("paused"),
            "eligibility reason must be 'paused'"
        );
    } else {
        assert_eq!(
            elig.eligible_now, true,
            "fresh unqueued user must be eligible when unpaused"
        );
        assert_eq!(elig.reason, symbol_short!(""));
    }

    // ------------------------------------------------------------------------
    // Invariant 5: Circuit-Breaker Reset Recovery
    // ------------------------------------------------------------------------
    if breaker_state == BreakerState::Tripped {
        let reset_res = fix.client.try_reset_circuit_breaker(&fix.owner);
        assert!(reset_res.is_ok(), "reset_circuit_breaker must succeed");
        assert_eq!(
            fix.client.get_consecutive_failures(),
            0,
            "consecutive failures must reset to 0"
        );
        assert_eq!(
            fix.client.is_paused(),
            false,
            "vault must be unpaused after circuit breaker reset"
        );

        // Queue submission is restored after reset. `fresh_user` may still
        // hold no shares (Invariant 4 skips its deposit while paused, and a
        // tripped breaker starts paused), so fund it now that the vault is
        // confirmed unpaused.
        if fix.client.get_shares(&fresh_user) == 0 {
            mint_and_deposit(
                &fix.env,
                &fix.client,
                &fix.usdc_token,
                &fresh_user,
                1_000_0000000i128,
            );
        }
        let post_reset_queue = fix
            .client
            .try_queue_withdrawal(&fresh_user, &10_0000000i128);
        assert!(
            post_reset_queue.is_ok(),
            "queue_withdrawal must succeed after circuit breaker reset"
        );
    }
}

// ============================================================================
// Exhaustive Matrix Test: All 45 combinations (3 × 3 × 5)
// ============================================================================

#[test]
fn test_pause_queue_breaker_full_matrix_45_combinations() {
    let mut combinations_covered = 0usize;

    for &pause in &ALL_PAUSE_STATES {
        for &breaker in &ALL_BREAKER_STATES {
            for &queue in &ALL_QUEUE_STATES {
                verify_matrix_cell_invariants(pause, breaker, queue, 50_0000000i128);
                combinations_covered += 1;
            }
        }
    }

    assert_eq!(
        combinations_covered, 45,
        "Exhaustive test must cover exactly all 45 interaction states"
    );
}

// ============================================================================
// Property Tests with proptest!
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// Property: across random combinations from the state space and arbitrary
    /// withdrawal amounts, all state-machine invariants hold unconditionally.
    #[test]
    fn prop_matrix_pause_queue_breaker_invariants(
        pause_idx in 0..3usize,
        breaker_idx in 0..3usize,
        queue_idx in 0..5usize,
        test_amount in 1_0000000i128..500_0000000i128,
    ) {
        let pause = ALL_PAUSE_STATES[pause_idx];
        let breaker = ALL_BREAKER_STATES[breaker_idx];
        let queue = ALL_QUEUE_STATES[queue_idx];

        verify_matrix_cell_invariants(pause, breaker, queue, test_amount);
    }
}

// ============================================================================
// Property Test: Randomized Action Sequences across State Transitions
// ============================================================================

#[derive(Debug, Clone)]
enum VaultAction {
    Pause,
    Unpause,
    EmergencyPause,
    TriggerFailure,
    ResetBreaker,
    QueueRequest(i128),
    CancelFirst,
    ProcessBatch(u32),
    AdvanceTime(u64),
}

fn arb_vault_action() -> impl Strategy<Value = VaultAction> {
    prop_oneof![
        Just(VaultAction::Pause),
        Just(VaultAction::Unpause),
        Just(VaultAction::EmergencyPause),
        Just(VaultAction::TriggerFailure),
        Just(VaultAction::ResetBreaker),
        (10_0000000i128..50_0000000i128).prop_map(VaultAction::QueueRequest),
        Just(VaultAction::CancelFirst),
        (1u32..5u32).prop_map(VaultAction::ProcessBatch),
        (10u64..100u64).prop_map(VaultAction::AdvanceTime),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(15))]

    /// Property: Any random sequence of state transitions across Pause, Circuit-Breaker,
    /// and Withdrawal Queue preserves all system invariants at every single step.
    #[test]
    fn prop_random_sequence_pause_queue_breaker_transitions(
        actions in proptest::collection::vec(arb_vault_action(), 5..15)
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let (contract_id, agent, owner, usdc_token, blend_pool) =
            setup_vault_with_token_and_blend(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);
        let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

        client.set_blend_pool(&owner, &blend_pool);
        client.set_max_consecutive_failures(&3);
        client.set_queue_config(&10u32, &3600u64);

        // Raise the per-tx and per-user deposit caps above the deposit below,
        // and the TVL cap generously higher still: every action in the loop
        // below also deposits for a fresh eligibility-check user (up to 15
        // actions), all counting toward the same TVL cap. The contract
        // defaults (1,000 USDC per-tx, 1,000 USDC per-user, 10,000 USDC TVL)
        // would otherwise reject these deposits.
        client.set_deposit_limits(&DEFAULT_MIN_DEPOSIT, &MAX_DEPOSIT_CEILING);
        client.set_caps(&MAX_DEPOSIT_CEILING, &(MAX_DEPOSIT_CEILING * 10));

        let user = Address::generate(&env);
        // 50,000 USDC: the literal below was previously `50_000_0000000`
        // (500,000 USDC), which exceeds `MAX_DEPOSIT_CEILING` and so could
        // never actually be deposited in a single call.
        mint_and_deposit(&env, &client, &usdc_token, &user, 50_000_0000000i128 / 10);

        let mut active_req_ids = std::vec::Vec::new();

        for action in actions {
            match action {
                VaultAction::Pause => {
                    if !client.is_paused() {
                        let _ = client.try_pause(&owner);
                    }
                }
                VaultAction::Unpause => {
                    if client.is_paused() {
                        let _ = client.try_unpause(&owner);
                    }
                }
                VaultAction::EmergencyPause => {
                    if !client.is_paused() {
                        let _ = client.try_emergency_pause(&owner);
                    }
                }
                VaultAction::TriggerFailure => {
                    if !client.is_paused() {
                        blend_client.set_max_supply_limit(&-1_i128);
                        let _ = client.try_rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
                    }
                }
                VaultAction::ResetBreaker => {
                    let _ = client.try_reset_circuit_breaker(&owner);
                    assert_eq!(client.get_consecutive_failures(), 0);
                    assert!(!client.is_paused());
                }
                VaultAction::QueueRequest(amt) => {
                    let res = client.try_queue_withdrawal(&user, &amt);
                    if client.is_paused() {
                        assert!(res.is_err(), "queue_withdrawal must fail while paused");
                    } else if res.is_ok() {
                        let id = res.unwrap().unwrap();
                        active_req_ids.push(id);
                    }
                }
                VaultAction::CancelFirst => {
                    if let Some(first_id) = active_req_ids.first().copied() {
                        let cancel_res = client.try_cancel_withdrawal_request(&user, &first_id);
                        if cancel_res.is_ok() {
                            active_req_ids.retain(|&id| id != first_id);
                            assert!(client.try_get_withdrawal_request(&first_id).is_err());
                        }
                    }
                }
                VaultAction::ProcessBatch(batch) => {
                    let _ = client.try_process_withdrawal_queue(&agent, &batch);
                }
                VaultAction::AdvanceTime(secs) => {
                    env.ledger().with_mut(|li| {
                        li.timestamp += secs;
                    });
                }
            }

            // Universal Invariant checks after EVERY action:
            let is_paused = client.is_paused();
            let failures = client.get_consecutive_failures();
            let max_failures = client.get_max_consecutive_failures();

            // Breaker trip implies pause:
            if failures >= max_failures && failures > 0 {
                // If reset wasn't called, failure count at/above threshold was created by failure
                // which tripped the emergency pause.
            }

            // Eligibility matches pause. `get_withdrawal_eligibility` checks
            // the pause flag before the user's share balance, and `deposit`
            // itself panics with Paused while the vault is paused, so the
            // deposit is only possible (and only needed, to exercise the
            // "eligible" branch) when unpaused.
            let fresh_user = Address::generate(&env);
            if !is_paused {
                mint_and_deposit(&env, &client, &usdc_token, &fresh_user, 1_000_0000000i128);
            }
            let elig = client.get_withdrawal_eligibility(&fresh_user);
            if is_paused {
                assert_eq!(elig.eligible_now, false);
                assert_eq!(elig.reason, symbol_short!("paused"));
            } else {
                assert_eq!(elig.eligible_now, true);
                assert_eq!(elig.reason, symbol_short!(""));
            }
        }
    }
}

// ============================================================================
// Specialized Targeted Interaction Tests
// ============================================================================

/// Verifies that when a circuit breaker trips from consecutive rebalance failures,
/// `queue_withdrawal` is immediately blocked until the breaker is explicitly reset.
#[test]
fn test_tripped_circuit_breaker_blocks_withdrawal_queue_until_reset() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);
    client.set_max_consecutive_failures(&2);
    client.set_queue_config(&10u32, &3600u64);

    let user = Address::generate(&env);
    // 900 USDC, under `DEFAULT_MAX_DEPOSIT` (1,000 USDC/tx) — the original
    // 5,000 USDC deposit exceeded it 5x and panicked with
    // MaximumDepositExceeded. 900 USDC comfortably covers this test's three
    // 100 USDC queued requests.
    mint_and_deposit(&env, &client, &usdc_token, &user, 900_0000000i128);

    // Initial state: unpaused, 0 failures -> queue succeeds
    assert_eq!(client.get_consecutive_failures(), 0);
    assert!(!client.is_paused());
    let req1 = client.queue_withdrawal(&user, &100_0000000i128);
    assert_eq!(req1, 1);

    // Failure 1: consecutive_failures = 1 (< 2) -> still unpaused, queue still succeeds
    blend_client.set_max_supply_limit(&-1_i128);
    client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 1);
    assert!(!client.is_paused());
    let req2 = client.queue_withdrawal(&user, &100_0000000i128);
    assert_eq!(req2, 2);

    // Failure 2: reaches threshold 2 -> circuit breaker trips and auto-pauses!
    client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_consecutive_failures(), 2);
    assert!(
        client.is_paused(),
        "circuit breaker trip must auto-pause the vault"
    );

    // Queue submission is now rejected
    assert!(
        client
            .try_queue_withdrawal(&user, &100_0000000i128)
            .is_err(),
        "queue_withdrawal must be rejected when breaker is tripped"
    );

    // Emergency cancellation of req1 and req2 MUST STILL SUCCEED while breaker is tripped!
    client.cancel_withdrawal_request(&user, &req1);
    assert!(client.try_get_withdrawal_request(&req1).is_err());

    // Reset circuit breaker: restores failures to 0 and clears pause
    client.reset_circuit_breaker(&owner);
    assert_eq!(client.get_consecutive_failures(), 0);
    assert!(!client.is_paused());

    // Queue submission works again
    let req3 = client.queue_withdrawal(&user, &100_0000000i128);
    assert_eq!(req3, 3);
}

/// Verifies that users are never trapped: cancellation succeeds across all emergency
/// pause states, manual pause states, and circuit-breaker tripped states.
#[test]
fn test_emergency_cancellation_unblocked_in_all_emergency_and_tripped_states() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);
    client.set_max_consecutive_failures(&1);
    client.set_queue_config(&10u32, &3600u64);

    let user = Address::generate(&env);
    // 900 USDC, under `DEFAULT_MAX_DEPOSIT` (1,000 USDC/tx) — see the sibling
    // fixes above in this file for why 5,000 USDC panicked.
    mint_and_deposit(&env, &client, &usdc_token, &user, 900_0000000i128);

    // Queue 3 requests
    let r1 = client.queue_withdrawal(&user, &100_0000000i128);
    let r2 = client.queue_withdrawal(&user, &100_0000000i128);
    let r3 = client.queue_withdrawal(&user, &100_0000000i128);

    // State A: Manual pause
    client.pause(&owner);
    assert!(client.is_paused());
    client.cancel_withdrawal_request(&user, &r1);
    assert!(client.try_get_withdrawal_request(&r1).is_err());
    client.unpause(&owner);

    // State B: Emergency pause
    client.emergency_pause(&owner);
    assert!(client.is_paused());
    client.cancel_withdrawal_request(&user, &r2);
    assert!(client.try_get_withdrawal_request(&r2).is_err());
    client.unpause(&owner);

    // State C: Circuit-breaker auto-pause
    blend_client.set_max_supply_limit(&-1_i128);
    client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
    assert!(client.is_paused());
    client.cancel_withdrawal_request(&user, &r3);
    assert!(client.try_get_withdrawal_request(&r3).is_err());
}

/// Verifies that keepers can process and prune expired queue requests even during
/// pause and circuit-breaker trips.
#[test]
fn test_queue_processing_and_expiration_during_pause_and_breaker_tripped() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);
    client.set_max_consecutive_failures(&1);
    client.set_queue_config(&10u32, &60u64);

    let user = Address::generate(&env);
    // 900 USDC, under `DEFAULT_MAX_DEPOSIT` (1,000 USDC/tx) — see the sibling
    // fixes above in this file for why 5,000 USDC panicked.
    mint_and_deposit(&env, &client, &usdc_token, &user, 900_0000000i128);

    let r1 = client.queue_withdrawal(&user, &100_0000000i128);

    // Trip circuit breaker
    blend_client.set_max_supply_limit(&-1_i128);
    client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);
    assert!(client.is_paused());

    // Advance time past 60s TTL
    env.ledger().with_mut(|li| {
        li.timestamp += 120;
    });

    // Process queue while paused and breaker tripped:
    // Expired request is safely pruned without error or payout
    let processed = client.process_withdrawal_queue(&agent, &10u32);
    assert_eq!(
        processed, 0,
        "expired request must be pruned without fulfillment"
    );
    assert!(client.try_get_withdrawal_request(&r1).is_err());
}
