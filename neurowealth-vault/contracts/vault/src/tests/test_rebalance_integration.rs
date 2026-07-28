//! Integration tests for rebalance protocol-path behavior (Issue #46)
//!
//! These tests validate end-to-end rebalance transitions including:
//!  - Rebalance to Blend with pool configured
//!  - Rebalance to Blend without pool (expected panic)
//!  - Rebalance away from current protocol (none and switch cases)
//!  - Failed/partial protocol call handling
//!  - CurrentProtocol storage state correctness
//!  - Asset accounting invariants
//!  - Emitted rebalance events
//!  - Atomicity of protocol exit (Issue #289)
//!  - ProtocolChangedEvent emission on every CurrentProtocol transition (Issue #290)
//!  - Canonical deposit → rebalance → yield → withdraw full lifecycle (Issue #291)

extern crate std;

use super::utils::*;
use crate::{BlendSupplyEvent, BlendWithdrawEvent, ProtocolChangedEvent, RebalanceEvent, RebalanceFailedEvent, TOPIC_PROTOCOL_CHANGED, TOPIC_REBALANCE_FAILED};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, TryFromVal};

// ============================================================================
// HELPERS
// ============================================================================

/// Total USDC held by the vault contract.
fn vault_usdc_balance(env: &Env, token: &Address, contract_id: &Address) -> i128 {
    TestTokenClient::new(env, token).balance(contract_id)
}

/// Return all decoded RebalanceEvents emitted so far.
fn collect_rebalance_events(env: &Env) -> std::vec::Vec<RebalanceEvent> {
    find_events_by_topic(env.events().all(), env, symbol_short!("rebalance"))
        .into_iter()
        .map(|(_, _, data)| {
            RebalanceEvent::try_from_val(env, &data).expect("data should decode to RebalanceEvent")
        })
        .collect()
}

/// Return all decoded BlendWithdrawEvents emitted so far.
fn collect_blend_withdraw_events(env: &Env) -> std::vec::Vec<BlendWithdrawEvent> {
    find_events_by_topic(env.events().all(), env, symbol_short!("blend_wd"))
        .into_iter()
        .map(|(_, _, data)| {
            BlendWithdrawEvent::try_from_val(env, &data)
                .expect("data should decode to BlendWithdrawEvent")
        })
        .collect()
}

/// Return all decoded BlendSupplyEvents emitted so far.
fn collect_blend_supply_events(env: &Env) -> std::vec::Vec<BlendSupplyEvent> {
    find_events_by_topic(env.events().all(), env, symbol_short!("blend_sup"))
        .into_iter()
        .map(|(_, _, data)| {
            BlendSupplyEvent::try_from_val(env, &data)
                .expect("data should decode to BlendSupplyEvent")
        })
        .collect()
}

// ============================================================================
// 1. REBALANCE TO BLEND — POOL CONFIGURED
// ============================================================================

/// Happy-path: rebalance into Blend when a pool is configured and the vault
/// has a deposited balance.  
/// Validates: CurrentProtocol, asset accounting, and the emitted events.
#[test]
fn test_integration_rebalance_to_blend_with_pool_configured() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    // Pre-condition: no protocol, no pool configured
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    assert!(client.get_blend_pool().is_none());

    // Configure pool
    client.set_blend_pool(&owner, &blend_pool);
    assert_eq!(client.get_blend_pool(), Some(blend_pool.clone()));

    // Deposit funds
    let deposit_amount = 15_000_000_i128; // 15 USDC
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Pre-rebalance state
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        deposit_amount
    );
    assert_eq!(blend_client.supplied(&usdc_token), 0);
    assert_eq!(client.get_total_assets(), deposit_amount);

    // Rebalance to Blend
    let apy = 700_i128; // 7%
    client.rebalance(&symbol_short!("blend"), &apy, &0_i128);

    // ---- CurrentProtocol ----
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol should switch to 'blend'"
    );

    // ---- Asset accounting ----
    // All vault USDC moved to Blend; total assets unchanged
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        0,
        "Vault should have 0 idle USDC after supplying to Blend"
    );
    assert_eq!(
        blend_client.supplied(&usdc_token),
        deposit_amount,
        "Blend pool should hold all deposited USDC"
    );
    assert_eq!(
        client.get_total_assets(),
        deposit_amount,
        "Total assets tracked by vault should be unchanged"
    );

    // ---- Emitted events ----
    let supply_events = collect_blend_supply_events(&env);
    assert!(
        !supply_events.is_empty(),
        "BlendSupplyEvent must be emitted"
    );
    let supply_event = supply_events.last().unwrap();
    assert_eq!(supply_event.amount_actual, deposit_amount);
    assert!(supply_event.success);

    let rebalance_events = collect_rebalance_events(&env);
    assert!(
        !rebalance_events.is_empty(),
        "RebalanceEvent must be emitted"
    );
    let rebalance_event = rebalance_events.last().unwrap();
    assert_eq!(rebalance_event.protocol, symbol_short!("blend"));
    assert_eq!(rebalance_event.expected_apy, apy);
}

/// Re-entrant call: rebalancing to Blend while already deployed to Blend
/// should not duplicate supply or cause double-accounting.
#[test]
fn test_integration_rebalance_to_blend_already_in_blend_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 10_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // First rebalance to Blend
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);

    // Second rebalance to Blend — no vault idle balance, nothing more to supply
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    let rebalance_events = collect_rebalance_events(&env);
    assert_eq!(
        rebalance_events.last().unwrap().status,
        symbol_short!("noop"),
        "Redundant blend rebalance with zero idle should be noop"
    );

    // State should remain consistent
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(
        blend_client.supplied(&usdc_token),
        deposit_amount,
        "Supplied amount should not change on redundant rebalance to blend"
    );
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        0,
        "Vault should still hold 0 idle USDC"
    );

    // Total assets must remain the same
    assert_eq!(client.get_total_assets(), deposit_amount);
}

// ============================================================================
// 2. REBALANCE TO BLEND — NO POOL (EXPECTED PANIC)
// ============================================================================

/// Rebalancing to Blend without a pool configured must panic with the
/// "Error(Contract, #18)" message when the vault holds funds.
#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_integration_rebalance_to_blend_without_pool_panics_with_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Deposit so vault_balance > 0 which triggers the pool lookup
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    // No blend pool set → must panic
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
}

/// Rebalancing to Blend without a pool configured must panic even when the
/// vault has zero balance (the check is on pool existence, not balance).
#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_integration_rebalance_to_blend_without_pool_panics_zero_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // No deposit, no pool — still panics because pool check comes first
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
}

// ============================================================================
// 3. REBALANCE AWAY FROM CURRENT PROTOCOL
// ============================================================================

/// Switching from Blend back to "none" must:
/// - Withdraw all funds from Blend
/// - Update CurrentProtocol to "none"
/// - Emit BlendWithdrawEvent and RebalanceEvent
/// - Restore full asset accounting in vault
#[test]
fn test_integration_rebalance_from_blend_to_none_withdraws_all() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 25_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Move funds into Blend
    client.rebalance(&symbol_short!("blend"), &800_i128, &0_i128);
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);

    // Switch back to none
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // ---- CurrentProtocol ----
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "CurrentProtocol should revert to 'none'"
    );

    // ---- Asset accounting ----
    assert_eq!(
        blend_client.supplied(&usdc_token),
        0,
        "Blend pool should be fully emptied"
    );
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        deposit_amount,
        "All funds must return to the vault"
    );
    assert_eq!(client.get_total_assets(), deposit_amount);

    // ---- Events ----
    let wd_events = collect_blend_withdraw_events(&env);
    assert!(!wd_events.is_empty(), "BlendWithdrawEvent must be emitted");
    let wd_event = wd_events.last().unwrap();
    assert_eq!(wd_event.amount_actual, deposit_amount);
    assert!(wd_event.success, "Withdrawal should be marked successful");

    let rebalance_events = collect_rebalance_events(&env);
    let none_event = rebalance_events
        .iter()
        .find(|e| e.protocol == symbol_short!("none"))
        .expect("RebalanceEvent for 'none' must be emitted");
    assert_eq!(none_event.expected_apy, 0_i128);
}

/// Rebalancing from "none" to "none" is a no-op and should succeed without
/// touching any pool or changing any state.
#[test]
fn test_integration_rebalance_none_to_none_is_noop() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 8_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Rebalance none → none
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        deposit_amount,
        "Vault funds should be untouched when protocol stays 'none'"
    );
    assert_eq!(client.get_total_assets(), deposit_amount);

    let rebalance_events = collect_rebalance_events(&env);
    assert_eq!(rebalance_events.len(), 1, "Expected one RebalanceEvent");
    assert_eq!(
        rebalance_events[0].status,
        symbol_short!("noop"),
        "none→none with no fund movement should be a noop rebalance"
    );
}

/// Full round-trip: none → blend → none → blend, validating state at each step.
#[test]
fn test_integration_full_protocol_round_trip() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 20_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Step 1: none → blend
    client.rebalance(&symbol_short!("blend"), &600_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &contract_id), 0);

    // Step 2: blend → none
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    assert_eq!(blend_client.supplied(&usdc_token), 0);
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        deposit_amount
    );

    // Step 3: none → blend again
    client.rebalance(&symbol_short!("blend"), &550_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &contract_id), 0);
    assert_eq!(client.get_total_assets(), deposit_amount);
}

// ============================================================================
// 4. FAILED / PARTIAL PROTOCOL CALL HANDLING
// ============================================================================

/// When the vault has a zero balance (no deposits), rebalancing to Blend
/// must not panic — it should complete successfully even though there is
/// nothing to supply to the pool.
///
/// Note: The contract only writes CurrentProtocol = "blend" inside
/// `supply_to_blend`, which is guarded by `vault_balance > 0`.
/// When vault balance is zero, rebalance to blend is a noop but must update
/// CurrentProtocol to "blend" so protocol tracking is always consistent with
/// the agent's intent (Issue #146).
#[test]
fn test_integration_rebalance_blend_zero_vault_balance_no_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    // No deposit — vault has zero balance; this must NOT panic
    client.rebalance(&symbol_short!("blend"), &400_i128, &0_i128);

    // Even with zero vault balance, CurrentProtocol must be updated to "blend"
    // so state reflects the agent's intent (Issue #146).
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol must update to 'blend' even when no funds are moved"
    );

    let rebalance_events = collect_rebalance_events(&env);
    assert_eq!(rebalance_events.len(), 1, "Expected one RebalanceEvent");
    assert_eq!(
        rebalance_events[0].status,
        symbol_short!("noop"),
        "blend rebalance with zero idle balance should be a noop"
    );
}

/// Unsupported protocol names must panic with the canonical error message
/// so that callers receive a deterministic, explicit failure.
#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn test_integration_rebalance_unknown_protocol_is_explicit_failure() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.rebalance(&symbol_short!("aave"), &1200_i128, &0_i128);
}

/// Paused-vault rebalance must fail with an explicit "Error(Contract, #35)" panic
/// regardless of which protocol is targeted.
#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_integration_rebalance_while_paused_explicit_failure() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);
    client.pause(&owner);
    assert!(client.is_paused());

    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
}

// ============================================================================
// 5. ASSET ACCOUNTING ACROSS DEPOSIT / REBALANCE / WITHDRAW
// ============================================================================

/// Verifies that total assets remain conserved across a deposit → blend supply
/// → partial user withdraw → blend → none cycle.
#[test]
fn test_integration_asset_accounting_invariant_across_full_cycle() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    // Two users deposit
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let amount_a = 12_000_000_i128;
    let amount_b = 8_000_000_i128;
    let total = amount_a + amount_b;

    mint_and_deposit(&env, &client, &usdc_token, &user_a, amount_a);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, amount_b);
    assert_eq!(client.get_total_assets(), total);

    // Rebalance to Blend
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(blend_client.supplied(&usdc_token), total);
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &contract_id), 0);

    // User A withdraws
    client.withdraw(&user_a, &amount_a, &None);
    let expected_in_blend = blend_client.supplied(&usdc_token);
    assert_eq!(
        expected_in_blend, amount_b,
        "Blend should retain only user B's share after user A withdraws"
    );
    assert_eq!(client.get_total_assets(), amount_b);

    // Rebalance back to none
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    assert_eq!(blend_client.supplied(&usdc_token), 0);
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        amount_b,
        "Vault should hold exactly user B's remaining funds"
    );
    assert_eq!(client.get_total_assets(), amount_b);
}

// ============================================================================
// 6. EVENT SCHEMA VALIDATION
// ============================================================================

/// Validates that every rebalance emits exactly one RebalanceEvent with the
/// correct protocol and expected_apy fields.
#[test]
fn test_integration_rebalance_event_schema_correctness() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Rebalance 1: none (initial state → none)
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    // Rebalance 2: none → blend
    client.rebalance(&symbol_short!("blend"), &850_i128, &0_i128);
    // Rebalance 3: blend → none
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let events = collect_rebalance_events(&env);
    assert_eq!(events.len(), 3, "Expected exactly 3 RebalanceEvents");

    assert_eq!(events[0].protocol, symbol_short!("none"));
    assert_eq!(events[0].expected_apy, 0_i128);
    assert_eq!(events[0].amount_supplied, 0);
    assert_eq!(events[0].amount_withdrawn, 0);

    assert_eq!(events[1].protocol, symbol_short!("blend"));
    assert_eq!(events[1].expected_apy, 850_i128);
    assert_eq!(events[1].amount_supplied, 10_000_000_i128);
    assert_eq!(events[1].amount_withdrawn, 0);

    assert_eq!(events[2].protocol, symbol_short!("none"));
    assert_eq!(events[2].expected_apy, 0_i128);
    assert_eq!(events[2].amount_supplied, 0);
    assert_eq!(events[2].amount_withdrawn, 10_000_000_i128);
}

/// Validates that a Blend supply emits BlendSupplyEvent with the correct
/// asset address and success flag.
#[test]
fn test_integration_blend_supply_event_fields_are_correct() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 18_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.rebalance(&symbol_short!("blend"), &900_i128, &0_i128);

    let supply_events = collect_blend_supply_events(&env);
    assert_eq!(
        supply_events.len(),
        1,
        "Exactly one BlendSupplyEvent expected"
    );

    let evt = &supply_events[0];
    assert_eq!(
        evt.asset, usdc_token,
        "BlendSupplyEvent.asset must be the USDC token"
    );
    assert_eq!(evt.amount_actual, deposit_amount);
    assert!(evt.success);
}

/// Validates that a Blend withdrawal emits BlendWithdrawEvent with correct
/// amount_actual and success fields.
#[test]
fn test_integration_blend_withdraw_event_fields_on_protocol_switch() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 22_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Move to Blend then away
    client.rebalance(&symbol_short!("blend"), &600_i128, &0_i128);
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let wd_events = collect_blend_withdraw_events(&env);
    assert!(!wd_events.is_empty(), "BlendWithdrawEvent must be emitted");

    let evt = wd_events.last().unwrap();
    assert_eq!(
        evt.amount_actual, deposit_amount,
        "Amount actual should match full deposited balance"
    );
    assert!(evt.success, "Withdrawal event must be marked successful");
}

/// User withdrawal that pulls all funds out of Blend must update
/// CurrentProtocol to "none".
#[test]
fn test_integration_withdraw_all_updates_current_protocol_to_none() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 25_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Move funds into Blend
    client.rebalance(&symbol_short!("blend"), &800_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);

    // User withdraws everything
    client.withdraw_all(&user, &None);

    // Vault should have pulled everything from Blend
    assert_eq!(blend_client.supplied(&usdc_token), 0);

    // CurrentProtocol should be updated to none
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "CurrentProtocol should switch to 'none' when all funds are withdrawn from Blend"
    );
}

// ============================================================================
// 7. CANONICAL FULL-LIFECYCLE FLOW
// ============================================================================

/// Canonical end-to-end scenario covering the full vault lifecycle:
/// 1. Dual-user deposit
/// 2. Deployment to protocol (Blend)
/// 3. Yield accrual (Price inflation)
/// 4. Withdrawal from protocol
/// 5. Rebalance back to idle
/// 6. Final withdrawal with yield
#[test]
fn test_integration_canonical_full_lifecycle_flow() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    // Initial setup: Configure blend pool
    client.set_blend_pool(&owner, &blend_pool);

    // --- STEP 1: Dual-user deposit ---
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let amount_a = 10_000_000_i128; // 10 USDC
    let amount_b = 10_000_000_i128; // 10 USDC
    let initial_total = amount_a + amount_b;

    mint_and_deposit(&env, &client, &usdc_token, &user_a, amount_a);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, amount_b);

    assert_eq!(client.get_total_assets(), initial_total);
    assert_eq!(client.get_total_shares(), initial_total); // 1:1 initial price
    assert_eq!(client.get_shares(&user_a), amount_a);

    // --- STEP 2: Deployment to protocol (Blend) ---
    client.rebalance(&symbol_short!("blend"), &750_i128, &0_i128);

    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(blend_client.supplied(&usdc_token), initial_total);
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &contract_id), 0);

    // Verify Blend Supply Event
    let supply_events = collect_blend_supply_events(&env);
    assert!(supply_events
        .iter()
        .any(|e| e.amount_actual == initial_total));

    // --- STEP 3: Yield Accrual (Price Inflation) ---
    // Simulate 10% yield accrual by minting 2 USDC to vault address
    let yield_amount = 2_000_000_i128; // 2 USDC
    let total_with_yield = initial_total + yield_amount;
    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &total_with_yield, &false, &0);

    assert_eq!(client.get_total_assets(), total_with_yield);
    // Shares are still the same, so price per share has increased
    // Price = 22,000,000 / 20,000,000 = 1.1 USDC/share
    assert_eq!(client.get_total_shares(), initial_total);

    // --- STEP 4: Withdrawal from Protocol ---
    // User A withdraws their full position (including yield)
    // Should burn 10M shares and receive 11M USDC
    let expected_withdraw_a = 11_000_000_i128;
    client.withdraw(&user_a, &expected_withdraw_a, &None);

    assert_eq!(token_client.balance(&user_a), expected_withdraw_a);
    assert_eq!(client.get_shares(&user_a), 0);
    assert_eq!(client.get_total_shares(), 10_000_000_i128); // User B's shares remaining
    assert_eq!(client.get_total_assets(), 11_000_000_i128); // User B's assets remaining

    // Verify protocol withdrawal occurred
    let withdraw_events = collect_blend_withdraw_events(&env);
    assert!(!withdraw_events.is_empty());

    // --- STEP 5: Rebalance back to idle ---
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    assert_eq!(blend_client.supplied(&usdc_token), 0);
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        11_000_000_i128
    );

    // --- STEP 6: Final withdrawal with yield ---
    // User B withdraws remaining funds
    let expected_withdraw_b = 11_000_000_i128;
    client.withdraw(&user_b, &expected_withdraw_b, &None);

    assert_eq!(token_client.balance(&user_b), expected_withdraw_b);
    assert_eq!(client.get_total_assets(), 0);
    assert_eq!(client.get_total_shares(), 0);

    // FINAL CHECK: All invariants hold
    assert_eq!(client.get_total_deposits(), 0);
    assert_eq!(vault_usdc_balance(&env, &usdc_token, &contract_id), 0);
}

// ============================================================================
// #289 — REBALANCE ATOMICITY: EXIT FAILURE BLOCKS TRANSITION
// ============================================================================

/// When the mock Blend pool is configured with a withdrawal limit that prevents
/// a full exit, `rebalance()` must:
///  - Emit `RebalanceFailedEvent` with reason "exit_fail"
///  - NOT update `CurrentProtocol` to the new target
///  - NOT supply any funds to the new protocol
///
/// This test pins the atomicity guarantee: no partial protocol transition occurs.
#[test]
fn test_rebalance_exit_failure_blocks_protocol_transition() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    // Set up a DEX pool so the vault could theoretically transition to it
    let dex_pool = env.register_contract(None, MockDexPool);
    client.set_dex_pool(&owner, &dex_pool);

    let deposit_amount = 20_000_000_i128; // 20 USDC
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Move funds to Blend
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);

    // Simulate stuck liquidity: only 5 USDC can be withdrawn per call
    blend_client.set_max_withdraw_limit(&5_000_000_i128);

    // Attempt to switch blend → dex — exit is incomplete, must abort
    client.rebalance(&symbol_short!("dex"), &1200_i128, &0_i128);

    // CurrentProtocol must remain "blend" — no partial transition
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol must not change when Blend exit is incomplete"
    );

    // Blend must still hold funds (only partial exit occurred)
    assert!(
        blend_client.supplied(&usdc_token) > 0,
        "Blend pool must still hold funds after failed exit"
    );

    // DEX must have received nothing
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);
    assert_eq!(
        dex_client.balance(&usdc_token, &contract_id),
        0,
        "DEX must not receive any funds when protocol exit fails"
    );

    // RebalanceFailedEvent must be emitted
    let failed_events = find_events_by_topic(
        env.events().all(),
        &env,
        TOPIC_REBALANCE_FAILED,
    );
    assert_eq!(
        failed_events.len(),
        1,
        "Exactly one RebalanceFailedEvent must be emitted on exit failure"
    );

    let (_, _, data) = &failed_events[0];
    let failed_event = RebalanceFailedEvent::try_from_val(&env, data)
        .expect("data should decode to RebalanceFailedEvent");
    assert_eq!(
        failed_event.from_protocol,
        symbol_short!("blend"),
        "RebalanceFailedEvent.from_protocol must be 'blend'"
    );
    assert_eq!(
        failed_event.reason,
        symbol_short!("exit_fail"),
        "RebalanceFailedEvent.reason must be 'exit_fail'"
    );
}

/// When the vault is in Blend and the agent calls rebalance("none") but the
/// Blend exit is incomplete, the vault must remain in Blend state and emit
/// RebalanceFailedEvent — not silently succeed.
#[test]
fn test_rebalance_to_none_exit_failure_does_not_clear_protocol() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 15_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.rebalance(&symbol_short!("blend"), &600_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    // Restrict withdrawal to simulate stuck liquidity
    blend_client.set_max_withdraw_limit(&1_000_000_i128);

    // Attempt rebalance to none — exit incomplete
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // Protocol must remain "blend"
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol must not be cleared when Blend exit is incomplete"
    );

    // Blend still holds the majority of funds
    assert!(
        blend_client.supplied(&usdc_token) > 0,
        "Blend pool must still hold funds"
    );

    // RebalanceFailedEvent must be emitted
    let failed_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_REBALANCE_FAILED);
    assert_eq!(failed_events.len(), 1);

    let (_, _, data) = &failed_events[0];
    let failed_event = RebalanceFailedEvent::try_from_val(&env, data)
        .expect("data should decode to RebalanceFailedEvent");
    assert_eq!(failed_event.from_protocol, symbol_short!("blend"));
    assert_eq!(failed_event.reason, symbol_short!("exit_fail"));
}

// ============================================================================
// #290 — ProtocolChangedEvent EMITTED ON EVERY CurrentProtocol TRANSITION
// ============================================================================

/// Every time `CurrentProtocol` changes, a `ProtocolChangedEvent` must be
/// emitted with correct `old_protocol` and `new_protocol` values.
///
/// Covers: none→blend, blend→none, and compound none→blend→none in sequence.
#[test]
fn test_protocol_changed_event_emitted_on_every_transition() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Transition 1: none → blend
    client.rebalance(&symbol_short!("blend"), &800_i128, &0_i128);

    let events_after_1 =
        find_events_by_topic(env.events().all(), &env, TOPIC_PROTOCOL_CHANGED);
    assert_eq!(
        events_after_1.len(),
        1,
        "Exactly one ProtocolChangedEvent after none→blend"
    );

    let (_, _, data) = &events_after_1[0];
    let e1 = ProtocolChangedEvent::try_from_val(&env, data)
        .expect("Should be ProtocolChangedEvent");
    assert_eq!(e1.old_protocol, symbol_short!("none"));
    assert_eq!(e1.new_protocol, symbol_short!("blend"));

    // Transition 2: blend → none
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let events_after_2 =
        find_events_by_topic(env.events().all(), &env, TOPIC_PROTOCOL_CHANGED);
    assert_eq!(
        events_after_2.len(),
        2,
        "Two ProtocolChangedEvents after blend→none"
    );

    let (_, _, data) = &events_after_2[1];
    let e2 = ProtocolChangedEvent::try_from_val(&env, data)
        .expect("Should be ProtocolChangedEvent");
    assert_eq!(e2.old_protocol, symbol_short!("blend"));
    assert_eq!(e2.new_protocol, symbol_short!("none"));

    // Idempotent rebalance (none → none): must NOT emit another event
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let events_after_3 =
        find_events_by_topic(env.events().all(), &env, TOPIC_PROTOCOL_CHANGED);
    assert_eq!(
        events_after_3.len(),
        2,
        "No new ProtocolChangedEvent when protocol stays the same"
    );
}

/// ProtocolChangedEvent is emitted when `supply_to_blend` sets the protocol
/// (not just when `set_current_protocol` is called from `rebalance` top-level).
#[test]
fn test_protocol_changed_event_emitted_from_supply_path() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    // This triggers supply_to_blend which calls set_current_protocol("blend")
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    let proto_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_PROTOCOL_CHANGED);
    assert_eq!(
        proto_events.len(),
        1,
        "ProtocolChangedEvent must be emitted via supply_to_blend path"
    );

    let (_, _, data) = &proto_events[0];
    let evt = ProtocolChangedEvent::try_from_val(&env, data)
        .expect("Should be ProtocolChangedEvent");
    assert_eq!(evt.old_protocol, symbol_short!("none"));
    assert_eq!(evt.new_protocol, symbol_short!("blend"));
}

/// ProtocolChangedEvent carries the correct old_protocol when transitioning
/// from blend → none via a user's `withdraw_all` (which drains Blend fully).
#[test]
fn test_protocol_changed_event_on_user_withdraw_draining_blend() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit_amount = 8_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Supply to Blend — emits one ProtocolChangedEvent (none→blend)
    client.rebalance(&symbol_short!("blend"), &600_i128, &0_i128);

    let events_before_wd =
        find_events_by_topic(env.events().all(), &env, TOPIC_PROTOCOL_CHANGED);
    assert_eq!(events_before_wd.len(), 1);

    // User drains the vault — Blend balance hits 0, set_current_protocol("none") fires
    client.withdraw_all(&user, &None);

    let events_after_wd =
        find_events_by_topic(env.events().all(), &env, TOPIC_PROTOCOL_CHANGED);
    assert_eq!(
        events_after_wd.len(),
        2,
        "Second ProtocolChangedEvent must fire when withdraw_all empties Blend"
    );

    let (_, _, data) = &events_after_wd[1];
    let evt = ProtocolChangedEvent::try_from_val(&env, data)
        .expect("Should be ProtocolChangedEvent");
    assert_eq!(evt.old_protocol, symbol_short!("blend"));
    assert_eq!(evt.new_protocol, symbol_short!("none"));
}

// ============================================================================
// #291 — CANONICAL DEPOSIT → REBALANCE → YIELD → WITHDRAW LIFECYCLE TEST
// ============================================================================

/// Single canonical test covering the full vault lifecycle with assertions on:
///  - Share minting and pricing at each stage
///  - Asset accounting (total_assets, total_shares)
///  - Protocol transitions and ProtocolChangedEvent
///  - Yield accrual reflected in withdrawal amounts
///  - Final invariant: all shares burned, all assets returned
///
/// Closes issue #291.
#[test]
fn test_full_lifecycle_deposit_rebalance_yield_withdraw() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    // ── PHASE 1: DEPOSIT ──────────────────────────────────────────────────────
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let deposit_a = 10_000_000_i128; // 10 USDC
    let deposit_b = 10_000_000_i128; // 10 USDC
    let total_deposited = deposit_a + deposit_b;

    mint_and_deposit(&env, &client, &usdc_token, &user_a, deposit_a);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, deposit_b);

    // 1:1 share price on bootstrap
    assert_eq!(client.get_total_assets(), total_deposited);
    assert_eq!(client.get_total_shares(), total_deposited);
    assert_eq!(client.get_shares(&user_a), deposit_a);
    assert_eq!(client.get_shares(&user_b), deposit_b);
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        total_deposited
    );

    // ── PHASE 2: REBALANCE → BLEND ────────────────────────────────────────────
    client.rebalance(&symbol_short!("blend"), &750_i128, &0_i128);

    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(
        blend_client.supplied(&usdc_token),
        total_deposited,
        "All funds must be in Blend"
    );
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        0,
        "Vault idle balance must be zero after rebalance to Blend"
    );
    // Total assets unchanged by rebalance
    assert_eq!(client.get_total_assets(), total_deposited);
    assert_eq!(client.get_total_shares(), total_deposited);

    // ProtocolChangedEvent: none → blend
    let proto_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_PROTOCOL_CHANGED);
    assert_eq!(proto_events.len(), 1, "One ProtocolChangedEvent after rebalance to blend");
    let (_, _, data) = &proto_events[0];
    let pce = ProtocolChangedEvent::try_from_val(&env, data).unwrap();
    assert_eq!(pce.old_protocol, symbol_short!("none"));
    assert_eq!(pce.new_protocol, symbol_short!("blend"));

    // BlendSupplyEvent
    let supply_events = collect_blend_supply_events(&env);
    assert_eq!(supply_events.len(), 1);
    assert_eq!(supply_events[0].amount_actual, total_deposited);
    assert!(supply_events[0].success);

    // ── PHASE 3: YIELD ACCRUAL ────────────────────────────────────────────────
    // 10% yield: 2 USDC minted to vault (simulating interest from Blend)
    let yield_amount = 2_000_000_i128;
    let total_with_yield = total_deposited + yield_amount;

    token_client.mint(&contract_id, &yield_amount);
    client.update_total_assets(&agent, &total_with_yield, &false, &0);

    // total_assets grows; total_shares unchanged → price per share = 1.1
    assert_eq!(client.get_total_assets(), total_with_yield);
    assert_eq!(client.get_total_shares(), total_deposited); // shares unchanged

    // Each user's share value: 10_000_000 shares × (22/20) = 11 USDC
    let share_price_numerator = total_with_yield;  // 22_000_000
    let share_price_denominator = total_deposited; // 20_000_000
    let user_a_entitlement =
        deposit_a * share_price_numerator / share_price_denominator; // 11_000_000
    assert_eq!(user_a_entitlement, 11_000_000_i128);

    // ── PHASE 4: USER A WITHDRAWS (from Blend) ───────────────────────────────
    client.withdraw(&user_a, &user_a_entitlement, &None);

    // User A received yield-bearing amount
    assert_eq!(
        token_client.balance(&user_a),
        user_a_entitlement,
        "User A must receive principal + yield share"
    );
    // User A's shares are fully burned
    assert_eq!(client.get_shares(&user_a), 0);

    // Vault state after user A's withdrawal
    let remaining_shares = deposit_b; // 10_000_000
    let remaining_assets = total_with_yield - user_a_entitlement; // 11_000_000
    assert_eq!(client.get_total_shares(), remaining_shares);
    assert_eq!(client.get_total_assets(), remaining_assets);

    // Blend withdrawal event fired
    let wd_events = collect_blend_withdraw_events(&env);
    assert!(!wd_events.is_empty(), "BlendWithdrawEvent must fire on user withdrawal from Blend");

    // ── PHASE 5: REBALANCE → NONE ─────────────────────────────────────────────
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    assert_eq!(blend_client.supplied(&usdc_token), 0, "Blend fully emptied");
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        remaining_assets,
        "All remaining funds returned to vault"
    );

    // ProtocolChangedEvent: blend → none
    let proto_events_2 =
        find_events_by_topic(env.events().all(), &env, TOPIC_PROTOCOL_CHANGED);
    assert_eq!(
        proto_events_2.len(),
        2,
        "Second ProtocolChangedEvent after rebalance to none"
    );
    let (_, _, data2) = &proto_events_2[1];
    let pce2 = ProtocolChangedEvent::try_from_val(&env, data2).unwrap();
    assert_eq!(pce2.old_protocol, symbol_short!("blend"));
    assert_eq!(pce2.new_protocol, symbol_short!("none"));

    // ── PHASE 6: USER B WITHDRAWS (from idle vault) ───────────────────────────
    client.withdraw(&user_b, &remaining_assets, &None);

    assert_eq!(
        token_client.balance(&user_b),
        remaining_assets,
        "User B must receive their full yield-bearing entitlement"
    );
    assert_eq!(client.get_shares(&user_b), 0);

    // ── FINAL INVARIANTS ──────────────────────────────────────────────────────
    assert_eq!(client.get_total_shares(), 0, "All shares must be burned");
    assert_eq!(client.get_total_assets(), 0, "All assets must be returned");
    assert_eq!(client.get_total_deposits(), 0, "Deposits tracker must be zero");
    assert_eq!(
        vault_usdc_balance(&env, &usdc_token, &contract_id),
        0,
        "Vault idle balance must be zero at end"
    );

    // Total paid out = both users received principal + proportional yield
    let total_paid = token_client.balance(&user_a) + token_client.balance(&user_b);
    assert_eq!(
        total_paid,
        total_with_yield,
        "Sum of all withdrawals must equal total_deposited + yield"
    );
}
