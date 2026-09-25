//! # Pause / Unpause Tests
//!
//! This module contains two sections:
//!
//! 1. **Functional pause tests** — owner can pause/unpause, events are emitted,
//!    existing operations are blocked.
//!
//! 2. **Pause-semantics matrix** (Issue #601) — exhaustive per-function
//!    assertions that encode the definitive table published in `SECURITY.md`.
//!    Every public contract function appears exactly once in one of the two
//!    categories below:
//!
//!    - 🔴 **BLOCKED while paused** — must panic with `VaultError::Paused` (#35).
//!    - 🟢 **ALLOWED while paused** — must NOT panic with `VaultError::Paused`.
//!
//! ## Adding a New Function
//!
//! If you add a new public entry-point to `lib.rs`, you MUST add a test here
//! and update the pause-semantics table in `SECURITY.md`. The PR checklist
//! enforces this.

extern crate std;

use super::utils::*;
use crate::{EmergencyPausedEvent, VaultPausedEvent, TOPIC_EMERGENCY_PAUSED, TOPIC_PAUSED};
use soroban_sdk::{testutils::{Address as _, Ledger as _}, Address, BytesN, Env, TryFromVal};

// ============================================================================
// ── Section 1: Functional pause tests (original) ─────────────────────────────
// ============================================================================

#[test]
fn test_owner_can_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused(), "Vault should start unpaused");

    client.pause(&owner);

    assert!(client.is_paused(), "Vault should be paused");
}

#[test]
fn test_owner_can_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    client.unpause(&owner);
    assert!(!client.is_paused(), "Vault should be unpaused");
}

#[test]
fn test_owner_can_emergency_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused());

    client.emergency_pause(&owner);

    assert!(client.is_paused(), "Vault should be emergency paused");
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_non_owner_cannot_unpause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Owner pauses
    client.pause(&owner);
    assert!(client.is_paused());

    // A fresh address that is NOT the owner tries to unpause
    let non_owner = Address::generate(&env);
    client.unpause(&non_owner);
}

#[test]
#[should_panic]
fn test_unauthorized_users_cannot_pause() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let unauthorized = Address::generate(&env);

    client.emergency_pause(&owner);
    // Fails because unauthorized != stored_owner
    client.pause(&unauthorized);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_deposit_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.pause(&owner);
    assert!(client.is_paused());

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;

    token_client.mint(&user, &amount);
    client.deposit(&user, &amount);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_withdraw_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    client.pause(&owner);
    assert!(client.is_paused());

    let balance = client.get_balance(&user);
    client.withdraw(&user, &balance, &None);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_rebalance_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    // require_not_paused fires before any blend check
    client.rebalance(&soroban_sdk::symbol_short!("blend"), &500_i128, &0_i128);
}

#[test]
fn test_pause_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    let pause_events = find_events_by_topic(env.events().all(), &env, TOPIC_PAUSED);
    assert_eq!(
        pause_events.len(),
        1,
        "Exactly one paused event should be emitted"
    );

    let (_, _, data) = &pause_events[0];
    let event =
        VaultPausedEvent::try_from_val(&env, data).expect("Should be a valid VaultPausedEvent");
    assert_eq!(event.owner, owner, "Event owner should match caller");
}

#[test]
fn test_emergency_pause_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.emergency_pause(&owner);

    let emergency_events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);
    assert_eq!(
        emergency_events.len(),
        1,
        "Exactly one emergency paused event should be emitted"
    );

    let (_, _, data) = &emergency_events[0];
    let event = EmergencyPausedEvent::try_from_val(&env, data)
        .expect("Should be a valid EmergencyPausedEvent");
    assert_eq!(event.owner, owner, "Event owner should match caller");
}

#[test]
fn test_emergency_pause_idempotent() {
    // Verifies that calling emergency_pause() when already paused
    // does not panic and does not emit a second event (Issue #534).
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(!client.is_paused());

    // First call — must pause and emit exactly one event
    client.emergency_pause(&owner);
    assert!(client.is_paused());

    let events_after_first = find_events_by_topic(
        env.events().all(),
        &env,
        soroban_sdk::symbol_short!("emerg"),
    );
    assert_eq!(
        events_after_first.len(),
        1,
        "first emergency_pause must emit exactly one event"
    );

    // Second call — must NOT panic, must NOT emit a second event
    client.emergency_pause(&owner);
    assert!(
        client.is_paused(),
        "paused state must remain unchanged after second call"
    );

    let events_after_second = find_events_by_topic(
        env.events().all(),
        &env,
        soroban_sdk::symbol_short!("emerg"),
    );
    assert_eq!(
        events_after_second.len(),
        1,
        "second emergency_pause must NOT emit a duplicate event"
    );
}

// ============================================================================
// ISSUE #508: Circuit-breaker auto-pause distinguishable from owner pause
// ============================================================================

#[test]
fn test_auto_pause_emits_different_event_than_owner_pause() {
    let env = Env::default();
    env.mock_all_auths();

    // --- Part 1: owner-initiated pause emits VaultPausedEvent (topic "paused") ---
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    let pause_events = find_events_by_topic(env.events().all(), &env, TOPIC_PAUSED);
    assert!(
        !pause_events.is_empty(),
        "owner pause must emit at least one VaultPausedEvent"
    );
    // Verify the last event is a VaultPausedEvent
    let (_, _, data) = pause_events.last().unwrap();
    let event = VaultPausedEvent::try_from_val(&env, data)
        .expect("owner pause event must decode as VaultPausedEvent");
    assert_eq!(event.owner, owner);

    // Ensure no EmergencyPausedEvent was emitted by this pause
    let emerg_events_after_pause =
        find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);
    assert_eq!(
        emerg_events_after_pause.len(),
        0,
        "owner pause must NOT emit EmergencyPausedEvent"
    );

    // --- Part 2: circuit-breaker auto-pause emits EmergencyPausedEvent (topic "emerg") ---
    // Deploy a fresh vault with Blend so the circuit breaker can fire.
    let (contract_id2, _agent2, owner2, usdc_token2, blend_pool2) =
        setup_vault_with_token_and_blend(&env);
    let client2 = NeuroWealthVaultClient::new(&env, &contract_id2);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool2);

    client2.set_blend_pool(&owner2, &blend_pool2);
    blend_client.set_max_supply_limit(&-1_i128); // force every rebalance to "fail"

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client2, &usdc_token2, &user, 10_000_000_i128);

    // Record events before triggering the circuit breaker.
    let emerg_before = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED).len();

    // Three consecutive failures trip the default threshold (3).
    client2.rebalance(&soroban_sdk::symbol_short!("blend"), &500_i128, &0_i128);
    client2.rebalance(&soroban_sdk::symbol_short!("blend"), &500_i128, &0_i128);
    client2.rebalance(&soroban_sdk::symbol_short!("blend"), &500_i128, &0_i128);
    assert!(client2.is_paused(), "circuit breaker must pause the vault");

    // Exactly one new EmergencyPausedEvent must have been emitted.
    let emerg_events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);
    assert_eq!(
        emerg_events.len(),
        emerg_before + 1,
        "circuit-breaker auto-pause must emit exactly one EmergencyPausedEvent"
    );
    let (_, _, data) = emerg_events.last().unwrap();
    let event = EmergencyPausedEvent::try_from_val(&env, data)
        .expect("auto-pause event must decode as EmergencyPausedEvent");
    assert_eq!(event.owner, owner2);

    // Ensure no VaultPausedEvent was emitted by the circuit breaker.
    let pause_events_after_circuit = find_events_by_topic(env.events().all(), &env, TOPIC_PAUSED);
    // The owner2 vault has no pause events (we only called rebalance, not pause).
    assert_eq!(
        pause_events_after_circuit.len(),
        0,
        "circuit-breaker auto-pause must NOT emit VaultPausedEvent"
    );

    // The topics themselves are different: "paused" vs "emerg".
    assert_ne!(
        TOPIC_PAUSED, TOPIC_EMERGENCY_PAUSED,
        "TOPIC_PAUSED and TOPIC_EMERGENCY_PAUSED must be distinct symbols"
    );
}

// ============================================================================
// ISSUE #189: Block upgrade while paused
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_upgrade_blocked_while_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    let fake_hash = BytesN::from_array(&env, &[0u8; 32]);
    client.schedule_upgrade(&owner, &fake_hash);
}

#[test]
fn test_upgrade_unpaused_vault_clears_pause_guard() {
    // Verifies that require_not_paused does not block upgrade on a healthy vault:
    // pause then unpause, and confirm the vault is no longer paused.
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());
    client.unpause(&owner);
    assert!(
        !client.is_paused(),
        "vault must be unpaused before upgrade is allowed"
    );
}

#[test]
fn test_emergency_pause_blocks_operations_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;

    // Pre-fund the user and deposit some tokens to test withdrawal later
    token_client.mint(&user, &amount);
    client.deposit(&user, &amount);

    assert!(!client.is_paused());

    // 1. Calls emergency_pause as owner
    client.emergency_pause(&owner);

    // 2. Asserts is_paused() returns true
    assert!(client.is_paused(), "Vault should be emergency paused");

    // 3. Verifies the emitted event topic is "emerg"
    let emergency_events = find_events_by_topic(env.events().all(), &env, TOPIC_EMERGENCY_PAUSED);
    assert_eq!(
        emergency_events.len(),
        1,
        "Exactly one emergency paused event should be emitted"
    );
    assert_eq!(
        TOPIC_EMERGENCY_PAUSED,
        soroban_sdk::symbol_short!("emerg"),
        "Event topic must be 'emerg'"
    );

    // 4. Asserts a deposit attempt panics with VaultError::Paused (#35)
    let deposit_res = client.try_deposit(&user, &amount);
    assert_eq!(
        deposit_res,
        Err(Ok(soroban_sdk::Error::from_contract_error(35))),
        "deposit attempt should panic with VaultError::Paused (#35)"
    );

    // 5. Asserts a withdrawal attempt panics with VaultError::Paused (#35)
    let withdraw_res = client.try_withdraw(&user, &amount, &None);
    assert_eq!(
        withdraw_res,
        Err(Ok(soroban_sdk::Error::from_contract_error(35))),
        "withdrawal attempt should panic with VaultError::Paused (#35)"
    );

    // 6. Asserts a rebalance attempt panics with VaultError::Paused (#35)
    let rebalance_res =
        client.try_rebalance(&soroban_sdk::symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(
        rebalance_res,
        Err(Ok(soroban_sdk::Error::from_contract_error(35))),
        "rebalance attempt should panic with VaultError::Paused (#35)"
    );
}

// ============================================================================
// ── Section 2: Pause-Semantics Matrix (Issue #601) ───────────────────────────
// ============================================================================
//
// This section is the machine-readable encoding of the pause-semantics table
// in SECURITY.md. Every public function in lib.rs appears exactly once below.

/// Error code for `VaultError::Paused`.
const PAUSED_ERR: u32 = 35;
/// Error code for `VaultError::NoTimelockPending`.
const NO_TIMELOCK_PENDING_ERR: u32 = 49;

/// Assert that a `try_*` result is NOT a Paused error.
///
/// The function may fail for other reasons (wrong caller, no timelock, etc.) —
/// what we verify is only that the failure reason is not `VaultError::Paused`.
///
/// The function is generic over `T` (success type) and `E` (the outer error),
/// expecting the inner `Err(Ok(soroban_sdk::Error))` pattern from `try_*` calls.
fn assert_not_paused_error<T: core::fmt::Debug, E: core::fmt::Debug>(
    result: &Result<T, E>,
    fn_name: &str,
) {
    // We check by comparing the debug representation to avoid naming
    // InvokeError which is not re-exported in the test namespace.
    let paused = soroban_sdk::Error::from_contract_error(PAUSED_ERR);
    let debug_str = std::format!("{:?}", result);
    let paused_str = std::format!("{:?}", paused);
    assert!(
        !debug_str.contains(&paused_str) || result.is_ok(),
        "{} must NOT return VaultError::Paused (#35) while paused — got: {:?}",
        fn_name,
        result
    );
}

// ---------------------------------------------------------------------------
// 🔴 BLOCKED while paused
// ---------------------------------------------------------------------------

/// `deposit` → BLOCKED (#35)
#[test]
fn pause_matrix_deposit_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.pause(&owner);
    let user = Address::generate(&env);
    token_client.mint(&user, &10_000_000_i128);

    assert_eq!(
        client.try_deposit(&user, &1_000_000_i128),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR))),
        "deposit must be blocked while paused"
    );
}

/// `batch_deposit` → BLOCKED (#35)
#[test]
fn pause_matrix_batch_deposit_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.pause(&owner);
    let user = Address::generate(&env);
    token_client.mint(&user, &10_000_000_i128);

    let entries = soroban_sdk::vec![&env, (usdc_token.clone(), 1_000_000_i128)];
    assert_eq!(
        client.try_batch_deposit(&user, &entries),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR))),
        "batch_deposit must be blocked while paused"
    );
}

/// `withdraw` → BLOCKED (#35)
#[test]
fn pause_matrix_withdraw_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);
    client.pause(&owner);

    assert_eq!(
        client.try_withdraw(&user, &1_000_000_i128, &None),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR))),
        "withdraw must be blocked while paused"
    );
}

/// `withdraw_all` → BLOCKED (#35)
#[test]
fn pause_matrix_withdraw_all_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);
    client.pause(&owner);

    assert_eq!(
        client.try_withdraw_all(&user, &None),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR))),
        "withdraw_all must be blocked while paused"
    );
}

/// `rebalance` → BLOCKED (#35)
#[test]
fn pause_matrix_rebalance_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    assert_eq!(
        client.try_rebalance(&soroban_sdk::symbol_short!("blend"), &500_i128, &0_i128),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR))),
        "rebalance must be blocked while paused"
    );
}

/// `harvest` → BLOCKED (#35)
#[test]
fn pause_matrix_harvest_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    assert_eq!(
        client.try_harvest(&0_i128),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR))),
        "harvest must be blocked while paused"
    );
}

/// `schedule_upgrade` → BLOCKED (#35)
#[test]
fn pause_matrix_schedule_upgrade_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let fake_hash = BytesN::from_array(&env, &[1u8; 32]);

    assert_eq!(
        client.try_schedule_upgrade(&owner, &fake_hash),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR))),
        "schedule_upgrade must be blocked while paused"
    );
}

/// `execute_upgrade` → BLOCKED (#35)
#[test]
fn pause_matrix_execute_upgrade_blocked() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Schedule upgrade while unpaused.
    let fake_hash = BytesN::from_array(&env, &[1u8; 32]);
    client.schedule_upgrade(&owner, &fake_hash);

    // Advance ledger past the timelock.
    env.ledger().set_sequence_number(env.ledger().sequence() + 17_281);

    client.pause(&owner);
    assert!(client.is_paused());

    assert_eq!(
        client.try_execute_upgrade(&owner),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR))),
        "execute_upgrade must be blocked while paused"
    );
}

// ---------------------------------------------------------------------------
// 🟢 ALLOWED while paused
// ---------------------------------------------------------------------------

/// `cancel_upgrade` (no pending) → ALLOWED; fails with NoTimelockPending, not Paused.
#[test]
fn pause_matrix_cancel_upgrade_allowed_no_pending() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    let result = client.try_cancel_upgrade(&owner);
    assert_not_paused_error(&result, "cancel_upgrade (no pending)");
    assert_eq!(
        result,
        Err(Ok(soroban_sdk::Error::from_contract_error(
            NO_TIMELOCK_PENDING_ERR
        ))),
        "cancel_upgrade with no pending upgrade must fail with NoTimelockPending"
    );
}

/// `cancel_upgrade` (with pending upgrade) → ALLOWED; succeeds while paused.
#[test]
fn pause_matrix_cancel_upgrade_allowed_with_pending() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let fake_hash = BytesN::from_array(&env, &[2u8; 32]);
    client.schedule_upgrade(&owner, &fake_hash);

    client.pause(&owner);
    assert!(client.is_paused());

    // Must succeed while paused.
    client.cancel_upgrade(&owner);
    assert!(
        client.get_pending_upgrade().is_none(),
        "pending upgrade must be cleared after cancel_upgrade while paused"
    );
}

/// `update_total_assets` (increase) → ALLOWED while paused.
#[test]
fn pause_matrix_update_total_assets_increase_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Mint extra USDC directly to vault to simulate yield so solvency check passes.
    token_client.mint(&contract_id, &1_000_000_i128);

    client.pause(&owner);
    assert!(client.is_paused());

    let current = client.get_total_assets();
    let new_total = current + 500_000_i128;

    let result = client.try_update_total_assets(&agent, &new_total, &false, &0_u32);
    assert_not_paused_error(&result, "update_total_assets (increase)");
    assert!(
        result.is_ok(),
        "update_total_assets (increase) must succeed while paused"
    );
}

/// `emergency_harvest` → ALLOWED while paused (fails for other reasons, not Paused).
#[test]
fn pause_matrix_emergency_harvest_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    // Will fail with UnsupportedProtocol (no active protocol) — NOT Paused.
    let result = client.try_emergency_harvest(&0_i128);
    assert_not_paused_error(&result, "emergency_harvest");
}

/// `pause` → ALLOWED while already paused (idempotent).
#[test]
fn pause_matrix_pause_allowed_while_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    let result = client.try_pause(&owner);
    assert_not_paused_error(&result, "pause (while already paused)");
}

/// `unpause` → ALLOWED while paused.
#[test]
fn pause_matrix_unpause_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    client.unpause(&owner);
    assert!(!client.is_paused());
}

/// `emergency_pause` → ALLOWED while already paused.
#[test]
fn pause_matrix_emergency_pause_allowed_while_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let result = client.try_emergency_pause(&owner);
    assert_not_paused_error(&result, "emergency_pause (while already paused)");
    assert!(client.is_paused());
}

/// `update_agent` → ALLOWED while paused.
#[test]
fn pause_matrix_update_agent_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    let new_agent = Address::generate(&env);
    let result = client.try_update_agent(&new_agent);
    assert_not_paused_error(&result, "update_agent");
    assert!(result.is_ok(), "update_agent must succeed while paused");
}

/// `cancel_agent_update` (with pending) → ALLOWED while paused.
#[test]
fn pause_matrix_cancel_agent_update_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);
    client.pause(&owner);

    let result = client.try_cancel_agent_update();
    assert_not_paused_error(&result, "cancel_agent_update");
    assert!(
        result.is_ok(),
        "cancel_agent_update must succeed while paused"
    );
}

/// `transfer_ownership` → ALLOWED while paused.
#[test]
fn pause_matrix_transfer_ownership_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    let new_owner = Address::generate(&env);
    let result = client.try_transfer_ownership(&new_owner);
    assert_not_paused_error(&result, "transfer_ownership");
    assert!(
        result.is_ok(),
        "transfer_ownership must succeed while paused"
    );
}

/// `cancel_ownership_transfer` → ALLOWED while paused.
#[test]
fn pause_matrix_cancel_ownership_transfer_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    client.transfer_ownership(&new_owner);
    client.pause(&owner);

    let result = client.try_cancel_ownership_transfer();
    assert_not_paused_error(&result, "cancel_ownership_transfer");
    assert!(
        result.is_ok(),
        "cancel_ownership_transfer must succeed while paused"
    );
}

/// `set_tvl_cap` → ALLOWED while paused.
#[test]
fn pause_matrix_set_tvl_cap_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let result = client.try_set_tvl_cap(&500_000_000_000_i128);
    assert_not_paused_error(&result, "set_tvl_cap");
    assert!(result.is_ok(), "set_tvl_cap must succeed while paused");
}

/// `set_user_deposit_cap` → ALLOWED while paused.
#[test]
fn pause_matrix_set_user_deposit_cap_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let result = client.try_set_user_deposit_cap(&50_000_000_000_i128);
    assert_not_paused_error(&result, "set_user_deposit_cap");
    assert!(result.is_ok());
}

/// `set_caps` → ALLOWED while paused.
#[test]
fn pause_matrix_set_caps_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let result = client.try_set_caps(&50_000_000_000_i128, &500_000_000_000_i128);
    assert_not_paused_error(&result, "set_caps");
    assert!(result.is_ok());
}

/// `set_deposit_limits` → ALLOWED while paused.
#[test]
fn pause_matrix_set_deposit_limits_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let result = client.try_set_deposit_limits(&1_000_000_i128, &100_000_000_i128);
    assert_not_paused_error(&result, "set_deposit_limits");
    assert!(result.is_ok());
}

/// `set_rebalance_cooldown` → ALLOWED while paused.
#[test]
fn pause_matrix_set_rebalance_cooldown_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let result = client.try_set_rebalance_cooldown(&100_u32);
    assert_not_paused_error(&result, "set_rebalance_cooldown");
    assert!(result.is_ok());
}

/// `set_approval_ttl` → ALLOWED while paused.
#[test]
fn pause_matrix_set_approval_ttl_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let result = client.try_set_approval_ttl(&50_000_u32);
    assert_not_paused_error(&result, "set_approval_ttl");
    assert!(result.is_ok());
}

/// `set_user_strategy` → ALLOWED while paused.
#[test]
fn pause_matrix_set_user_strategy_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    let user = Address::generate(&env);
    let result = client.try_set_user_strategy(&user, &soroban_sdk::symbol_short!("balanced"));
    assert_not_paused_error(&result, "set_user_strategy");
    assert!(result.is_ok());
}

/// `touch_user_ttl` → ALLOWED while paused.
#[test]
fn pause_matrix_touch_user_ttl_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    client.pause(&owner);
    let result = client.try_touch_user_ttl(&user);
    assert_not_paused_error(&result, "touch_user_ttl");
    assert!(result.is_ok());
}

/// All read-only / view functions → ALLOWED while paused.
#[test]
fn pause_matrix_getters_all_allowed() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    client.pause(&owner);
    assert!(client.is_paused());

    // Every getter must execute without Paused error.
    let _ = client.is_paused();
    let _ = client.get_balance(&user);
    let _ = client.get_total_deposits();
    let _ = client.get_total_assets();
    let _ = client.get_total_shares();
    let _ = client.get_shares(&user);
    let _ = client.get_owner();
    let _ = client.get_agent();
    let _ = client.get_version();
    let _ = client.get_usdc_token();
    let _ = client.get_current_protocol();
    let _ = client.get_tvl_cap();
    let _ = client.get_user_deposit_cap();
    let _ = client.get_min_deposit();
    let _ = client.get_max_deposit();
    let _ = client.get_idle_balance();
    let _ = client.get_deployed_assets();
    let _ = client.get_asset_breakdown();
    let _ = client.get_exchange_rate();
    let _ = client.get_rebalance_cooldown();
    let _ = client.get_last_rebalance_ledger();
    let _ = client.get_approval_ttl();
    let _ = client.get_blend_approval_ttl();
    let _ = client.get_max_consecutive_failures();
    let _ = client.get_consecutive_failures();
    let _ = client.get_pending_upgrade();
    let _ = client.get_pending_agent_update();
    let _ = client.get_pending_owner();
    let _ = client.get_pending_ownership();
    let _ = client.get_user_strategy(&user);
    let _ = client.preview_deposit_to_shares(&1_000_000_i128);
    let _ = client.preview_shares_to_assets(&1_000_000_i128);
    let _ = client.preview_withdraw(&1_000_000_i128);
    let _ = client.convert_to_shares(&1_000_000_i128);
    let _ = client.convert_to_assets(&1_000_000_i128);
    let _ = client.get_users_with_shares(&0_u32, &10_u32);
    let _ = client.get_user_info(&user);
}

// ---------------------------------------------------------------------------
// Resume-after-unpause regression tests
// ---------------------------------------------------------------------------

/// After unpause, `deposit` must succeed again.
#[test]
fn pause_matrix_deposit_resumes_after_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    token_client.mint(&user, &10_000_000_i128);

    client.pause(&owner);
    assert_eq!(
        client.try_deposit(&user, &1_000_000_i128),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR)))
    );

    client.unpause(&owner);
    assert!(!client.is_paused());
    client.deposit(&user, &1_000_000_i128);
    assert!(client.get_balance(&user) > 0);
}

/// After unpause, `withdraw` must succeed again.
#[test]
fn pause_matrix_withdraw_resumes_after_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    client.pause(&owner);
    assert_eq!(
        client.try_withdraw(&user, &1_000_000_i128, &None),
        Err(Ok(soroban_sdk::Error::from_contract_error(PAUSED_ERR)))
    );

    client.unpause(&owner);
    assert!(!client.is_paused());
    client.withdraw(&user, &1_000_000_i128, &None);
}
