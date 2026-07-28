//! Tests for rebalance functionality

use super::utils::*;
use crate::{BlendWithdrawEvent, RebalanceEvent, RebalanceFailedEvent};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, TryFromVal,
};

#[test]
fn test_agent_can_rebalance_with_custom_protocol() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Verify agent is set correctly
    assert_eq!(client.get_agent(), agent);

    // Use "none" protocol — always safe, no external pool required
    let protocol = symbol_short!("none");
    let expected_apy = 500_i128; // 5% APY in basis points

    // Should succeed with mock_all_auths (require_is_agent passes)
    client.rebalance(&protocol, &expected_apy, &0_i128);
}

#[test]
fn test_owner_can_configure_blend_approval_ttl() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(client.get_blend_approval_ttl(), 100_000_u32);

    client.set_blend_approval_ttl(&owner, &42_u32);

    assert_eq!(client.get_blend_approval_ttl(), 42_u32);
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_non_owner_cannot_configure_blend_approval_ttl() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let attacker = Address::generate(&env);

    client.set_blend_approval_ttl(&attacker, &42_u32);
}

#[test]
fn test_blend_approval_expires_at_next_ledger_after_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_approval_ttl(&owner, &0_u32);

    let from = Address::generate(&env);
    let spender = Address::generate(&env);
    let amount = 1_234_i128;
    let approval_ledger = env.ledger().sequence();

    token_client.approve(&from, &spender, &amount, &approval_ledger);
    assert_eq!(
        token_client.allowance(&from, &spender),
        amount,
        "Allowance should remain valid on the exact approval ledger"
    );

    env.ledger()
        .set_sequence_number(approval_ledger.saturating_add(1));

    assert_eq!(
        token_client.allowance(&from, &spender),
        0_i128,
        "Allowance should expire once the ledger advances past the approval ledger"
    );
}

#[test]
fn test_rebalance_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set up Blend pool and deposit so there are assets
    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    let rebalance_events =
        find_events_by_topic(env.events().all(), &env, symbol_short!("rebalance"));
    assert!(
        !rebalance_events.is_empty(),
        "Rebalance should emit an event"
    );

    // Assert storage state change: CurrentProtocol should be "blend"
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol should be 'blend' after rebalance to blend"
    );
}

#[test]
fn test_rebalance_storage_current_protocol_changes() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    // Initial state: no protocol set
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "Initial CurrentProtocol should be 'none'"
    );

    // Deposit so vault has funds to supply
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Rebalance to blend
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    // Assert storage state changed
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol should be 'blend' after rebalance"
    );
}

#[test]
fn test_rebalance_storage_current_protocol_changes_to_none() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // First rebalance to blend
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol should be 'blend'"
    );

    // Then rebalance to none
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // Assert storage state changed to none
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "CurrentProtocol should be 'none' after rebalance to none"
    );
}

#[test]
fn test_set_blend_pool_storage() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Initially no blend pool
    assert!(
        client.get_blend_pool().is_none(),
        "BlendPool should be None before set_blend_pool"
    );

    // Set blend pool
    client.set_blend_pool(&owner, &blend_pool);

    // Assert storage state changed
    assert_eq!(
        client.get_blend_pool(),
        Some(blend_pool.clone()),
        "BlendPool should be set to the provided address"
    );
}

#[test]
fn test_rebalance_with_none_protocol_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // "none" protocol just sets current protocol to "none" — always safe to call
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
}

#[test]
fn test_rebalance_with_blend_after_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.set_blend_pool(&owner, &blend_pool);

    // Deposit so vault has a token balance to supply
    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(token_client.balance(&blend_pool), deposit_amount);
    assert_eq!(token_client.allowance(&contract_id, &blend_pool), 0);
}

#[test]
fn test_rebalance_apy_parameter_accepted() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Various APY values should be accepted without panicking
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    client.rebalance(&symbol_short!("none"), &850_i128, &0_i128);
    client.rebalance(&symbol_short!("none"), &2000_i128, &0_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_rebalance_rejects_expected_apy_above_range_with_typed_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.rebalance(&symbol_short!("none"), &10_001_i128, &0_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_rebalance_while_paused_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Pause the vault
    client.pause(&owner);
    assert!(client.is_paused());

    client.rebalance(&symbol_short!("none"), &500_i128, &0_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_blend_rebalance_without_pool_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Deposit so vault_balance > 0, triggering the blend pool check
    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // blend pool not set → should panic
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
}

#[test]
fn test_mock_token_transfer_from_uses_and_decrements_allowance() {
    let env = Env::default();
    env.mock_all_auths();

    let token = env.register_contract(None, TestToken);
    let token_client = TestTokenClient::new(&env, &token);

    let owner = Address::generate(&env);
    let spender = Address::generate(&env);
    let recipient = Address::generate(&env);

    token_client.mint(&owner, &10_000_000_i128);
    token_client.approve(&owner, &spender, &6_000_000_i128, &10_000_u32);

    assert_eq!(token_client.allowance(&owner, &spender), 6_000_000_i128);

    token_client.transfer_from(&spender, &owner, &recipient, &4_000_000_i128);

    assert_eq!(token_client.balance(&owner), 6_000_000_i128);
    assert_eq!(token_client.balance(&recipient), 4_000_000_i128);
    assert_eq!(token_client.allowance(&owner, &spender), 2_000_000_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn test_rebalance_with_unsupported_protocol_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // "balanced" is not a supported protocol — should panic
    client.rebalance(&symbol_short!("balanced"), &500_i128, &0_i128);
}

#[test]
fn test_rebalance_unsupported_protocol_emits_no_events() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // try_rebalance captures the panic but doesn't crash the test
    let _result = client.try_rebalance(&symbol_short!("invalid"), &0_i128, &0_i128);

    // Verify no rebalance events were published
    let rebalance_events =
        find_events_by_topic(env.events().all(), &env, symbol_short!("rebalance"));
    assert_eq!(
        rebalance_events.len(),
        0,
        "No rebalance event should be emitted on failure"
    );
}

#[test]
fn test_blend_supply_and_withdraw_with_events() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    // Configure Blend pool
    client.set_blend_pool(&owner, &blend_pool);

    // Deposit funds into vault
    let user = Address::generate(&env);
    let deposit_amount = 20_000_000_i128; // 20 USDC
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Verify initial state
    assert_eq!(client.get_total_assets(), deposit_amount);
    assert_eq!(token_client.balance(&contract_id), deposit_amount);
    assert_eq!(blend_client.supplied(&usdc_token), 0);

    // Rebalance to Blend (supply)
    client.rebalance(&symbol_short!("blend"), &850_i128, &0_i128);

    // Verify funds moved to Blend
    assert_eq!(
        token_client.balance(&contract_id),
        0,
        "Vault should have 0 USDC after supply"
    );
    assert_eq!(
        blend_client.supplied(&usdc_token),
        deposit_amount,
        "Blend should have all USDC"
    );
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    // Verify BlendSupplyEvent was emitted
    let supply_events = find_events_by_topic(env.events().all(), &env, symbol_short!("blend_sup"));
    assert!(
        !supply_events.is_empty(),
        "BlendSupplyEvent should be emitted"
    );

    // User withdraws half their balance
    let withdraw_amount = 10_000_000_i128; // 10 USDC
    client.withdraw(&user, &withdraw_amount, &None);

    // Verify funds were pulled from Blend and given to user
    assert_eq!(
        token_client.balance(&user),
        withdraw_amount,
        "User should receive withdrawn USDC"
    );
    assert_eq!(
        blend_client.supplied(&usdc_token),
        deposit_amount - withdraw_amount,
        "Blend should have remaining USDC"
    );

    // Verify BlendWithdrawEvent was emitted
    let withdraw_events = find_events_by_topic(env.events().all(), &env, symbol_short!("blend_wd"));
    assert!(
        !withdraw_events.is_empty(),
        "BlendWithdrawEvent should be emitted"
    );

    // Rebalance back to none (withdraw all from Blend)
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // Verify all funds withdrawn from Blend
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    assert_eq!(
        token_client.balance(&contract_id),
        deposit_amount - withdraw_amount,
        "Vault should have remaining USDC"
    );
    assert_eq!(
        blend_client.supplied(&usdc_token),
        0,
        "Blend should have 0 USDC"
    );

    // Verify second BlendWithdrawEvent was emitted
    let all_withdraw_events =
        find_events_by_topic(env.events().all(), &env, symbol_short!("blend_wd"));
    assert!(
        all_withdraw_events.len() >= 2,
        "Should have at least 2 BlendWithdrawEvents"
    );
}

#[test]
fn test_rebalance_blend_to_none_withdraws_all_and_updates_state_and_events() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 30_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.rebalance(&symbol_short!("blend"), &900_i128, &0_i128);

    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol should be 'blend' after rebalance to blend"
    );
    assert_eq!(
        blend_client.supplied(&usdc_token),
        deposit_amount,
        "All deposited funds should be supplied to Blend"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        0,
        "Vault should hold no idle USDC while fully allocated to Blend"
    );

    let none_apy = 0_i128;
    client.rebalance(&symbol_short!("none"), &none_apy, &0_i128);

    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "CurrentProtocol should be 'none' after rebalance to none"
    );
    assert_eq!(
        blend_client.supplied(&usdc_token),
        0,
        "Blend should be fully withdrawn after rebalance to none"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        deposit_amount,
        "All funds should be pulled back to vault after rebalance to none"
    );

    let blend_withdraw_events =
        find_events_by_topic(env.events().all(), &env, symbol_short!("blend_wd"));
    assert!(
        !blend_withdraw_events.is_empty(),
        "BlendWithdrawEvent should be emitted when switching blend -> none"
    );

    let (_, _, blend_withdraw_data) = blend_withdraw_events
        .last()
        .expect("Expected at least one blend_wd event");
    let blend_withdraw_event = BlendWithdrawEvent::try_from_val(&env, blend_withdraw_data)
        .expect("blend_wd data should decode to BlendWithdrawEvent");

    assert_eq!(
        blend_withdraw_event.amount_actual, deposit_amount,
        "blend_wd amount_actual should match full withdrawal"
    );
    assert!(
        blend_withdraw_event.success,
        "blend_wd event should mark withdrawal as successful"
    );

    let rebalance_events =
        find_events_by_topic(env.events().all(), &env, symbol_short!("rebalance"));
    let (_, _, rebalance_data) = rebalance_events
        .last()
        .expect("Expected rebalance event for none transition");
    let rebalance_event = RebalanceEvent::try_from_val(&env, rebalance_data)
        .expect("rebalance data should decode to RebalanceEvent");

    assert_eq!(
        rebalance_event.protocol,
        symbol_short!("none"),
        "rebalance event protocol should reflect target none"
    );
    assert_eq!(
        rebalance_event.expected_apy, none_apy,
        "rebalance event APY should match provided value"
    );
    assert_eq!(
        rebalance_event.amount_supplied, 0,
        "rebalance event should not report supplied amount on none transition"
    );
    assert_eq!(
        rebalance_event.amount_withdrawn, deposit_amount,
        "rebalance event should report the withdrawn amount on none transition"
    );
}

/// When a protocol exit is incomplete (funds remain in blend after withdrawal),
/// rebalance must abort and emit a RebalanceFailedEvent instead of panicking
/// (Issue #145). State must remain consistent: CurrentProtocol unchanged, no
/// re-supply attempted.
#[test]
fn test_rebalance_fails_on_incomplete_protocol_exit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);

    // Limit pool withdrawals to 1M — 9M stays stuck in blend
    blend_client.set_max_withdraw_limit(&1_000_000_i128);

    // rebalance("none") should abort gracefully — not panic
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // CurrentProtocol must remain "blend" — incomplete exit leaves state unchanged
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol must remain 'blend' after failed exit"
    );

    // RebalanceFailedEvent must be emitted so the failure is observable on-chain
    let failed_events = find_events_by_topic(env.events().all(), &env, symbol_short!("reb_fail"));
    assert!(
        !failed_events.is_empty(),
        "RebalanceFailedEvent must be emitted on incomplete exit"
    );
    let (_, _, data) = failed_events.last().unwrap();
    let evt = RebalanceFailedEvent::try_from_val(&env, data)
        .expect("should decode to RebalanceFailedEvent");
    assert_eq!(evt.from_protocol, symbol_short!("blend"));
    assert_eq!(evt.reason, symbol_short!("exit_fail"));
}

/// Incomplete exit during a cross-protocol switch also aborts and emits
/// RebalanceFailedEvent — no funds re-supplied to new protocol (Issue #145).
#[test]
fn test_rebalance_fails_when_switching_protocols_with_partial_exit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);

    // Limit pool to 2M per withdrawal — 8M stays in blend
    blend_client.set_max_withdraw_limit(&2_000_000_i128);

    // Attempt protocol switch with limited withdrawal — must abort
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    // CurrentProtocol unchanged
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "CurrentProtocol must remain 'blend' after failed protocol switch"
    );

    // RebalanceFailedEvent emitted
    let failed_events = find_events_by_topic(env.events().all(), &env, symbol_short!("reb_fail"));
    assert!(
        !failed_events.is_empty(),
        "RebalanceFailedEvent must be emitted on incomplete protocol switch exit"
    );
    let (_, _, data) = failed_events.last().unwrap();
    let evt = RebalanceFailedEvent::try_from_val(&env, data)
        .expect("should decode to RebalanceFailedEvent");
    assert_eq!(evt.from_protocol, symbol_short!("blend"));
    assert_eq!(evt.reason, symbol_short!("exit_fail"));

    // No funds were re-supplied to a new protocol
    assert_eq!(
        blend_client.supplied(&usdc_token),
        deposit_amount - 2_000_000_i128,
        "Only partial withdrawal should have occurred"
    );
}

/// When `min_out > 0`, a pool that supplies less than requested must panic (#150).
#[test]
#[should_panic(expected = "Error(Contract, #42)")]
fn test_rebalance_min_out_panics_when_pool_returns_less_than_requested() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    client.set_blend_pool(&owner, &blend_pool);
    blend_client.set_max_supply_limit(&5_000_000_i128);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    client.rebalance(&symbol_short!("blend"), &500_i128, &10_000_000_i128);
}

// ─── Issue #348: DEX approval expiry ledger-boundary test ────────────────────

/// Mirrors `test_blend_approval_expires_at_next_ledger_after_boundary` for the
/// DEX approval path. Both Blend and DEX share the same token `approve` expiry
/// semantics: an approval issued with `expiry_ledger = N` is valid when
/// `current_ledger <= N` and expires when `current_ledger == N + 1`.
///
/// The DEX supply path calls `token.approve(vault, pool, amount, current + ApprovalTtl)`.
/// We exercise the identical boundary here: set `ApprovalTtl` to its minimum
/// (1 000 ledgers), issue the approval at `current + 1_000`, assert it is valid
/// at exactly that ledger, then advance to `current + 1_001` and assert it has
/// expired.
#[test]
fn test_dex_approval_expires_at_next_ledger_after_boundary() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // MIN_APPROVAL_TTL == 1_000; use the minimum so the boundary is as tight
    // as the contract allows.
    let ttl: u32 = 1_000;
    client.set_approval_ttl(&ttl);

    let from = Address::generate(&env);
    let spender = Address::generate(&env);
    let amount = 5_678_i128;
    let current_ledger = env.ledger().sequence();
    let approval_ledger = current_ledger.saturating_add(ttl);

    token_client.approve(&from, &spender, &amount, &approval_ledger);

    // Advance to the exact TTL ledger — approval must still be valid.
    env.ledger().set_sequence_number(approval_ledger);
    assert_eq!(
        token_client.allowance(&from, &spender),
        amount,
        "DEX approval should remain valid on the exact TTL boundary ledger"
    );

    // One ledger past the boundary — approval must have expired.
    env.ledger()
        .set_sequence_number(approval_ledger.saturating_add(1));
    assert_eq!(
        token_client.allowance(&from, &spender),
        0_i128,
        "DEX approval should expire once the ledger advances past the TTL boundary"
    );
}

// ─── Issue #383: pool-address rotation while funds are deployed ─────────────
//
// Companion to the `set_blend_pool`/`set_dex_pool` fund-stranding issue.
// Neither setter currently guards against rotating the configured pool
// address while a nonzero position is still deployed to the *old* pool:
// every internal lookup (`get_protocol_balance`, `withdraw_from_blend`,
// `withdraw_from_dex`) reads the pool address from storage *at call time*,
// so once the address is rotated the vault permanently loses any way to see
// or reach funds sitting in the old pool contract.
//
// Pre-fix, these tests document the stranding: the rotation succeeds
// silently, and a subsequent `rebalance("none")` "succeeds" without
// recovering any funds because the vault only ever queries the new (empty)
// pool. Once a guard lands that rejects rotation while a position is
// deployed, flip these assertions to expect the rotation call itself to
// panic instead.

#[test]
fn test_blend_pool_rotation_while_deployed_strands_funds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, old_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let old_pool_client = MockBlendPoolClient::new(&env, &old_pool);

    client.set_blend_pool(&owner, &old_pool);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Deploy funds to the old pool.
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(old_pool_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    // Rotate to a brand new pool while the position is still fully deployed
    // to `old_pool`. No guard currently exists — this succeeds.
    let new_pool = env.register_contract(None, MockBlendPool);
    client.set_blend_pool(&owner, &new_pool);
    assert_eq!(client.get_blend_pool(), Some(new_pool.clone()));

    // The funds never moved — they are still sitting in `old_pool` — but the
    // vault's only pointer to a Blend pool now targets `new_pool`.
    assert_eq!(
        old_pool_client.supplied(&usdc_token),
        deposit_amount,
        "deployed funds remain in the old pool, untouched by the rotation"
    );
    let new_pool_client = MockBlendPoolClient::new(&env, &new_pool);
    assert_eq!(new_pool_client.supplied(&usdc_token), 0);

    // Attempting to exit the position now silently "succeeds" without
    // recovering anything: withdraw_from_protocol queries `new_pool` (via
    // DataKey::BlendPool), sees a 0 balance, treats the exit as complete, and
    // flips CurrentProtocol to "none" — even though the real funds are stuck
    // in `old_pool`.
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "vault believes it exited the position cleanly"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        0,
        "no funds were actually recovered to the vault"
    );
    assert_eq!(
        old_pool_client.supplied(&usdc_token),
        deposit_amount,
        "funds remain permanently stranded in the old pool"
    );
}

#[test]
fn test_dex_pool_rotation_while_deployed_strands_funds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, old_pool) =
        setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let old_pool_client = MockDexPoolClient::new(&env, &old_pool);

    client.set_dex_pool(&owner, &old_pool);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Deploy funds to the old pool.
    client.rebalance(&symbol_short!("dex"), &500_i128, &0_i128);
    assert_eq!(
        old_pool_client.balance(&usdc_token, &contract_id),
        deposit_amount
    );
    assert_eq!(client.get_current_protocol(), symbol_short!("dex"));

    // Rotate to a brand new pool while the position is still fully deployed
    // to `old_pool`. No guard currently exists — this succeeds.
    let new_pool = env.register_contract(None, MockDexPool);
    client.set_dex_pool(&owner, &new_pool);
    assert_eq!(client.get_dex_pool(), Some(new_pool.clone()));

    // The funds never moved — they are still sitting in `old_pool` — but the
    // vault's only pointer to a DEX pool now targets `new_pool`.
    assert_eq!(
        old_pool_client.balance(&usdc_token, &contract_id),
        deposit_amount,
        "deployed funds remain in the old pool, untouched by the rotation"
    );
    let new_pool_client = MockDexPoolClient::new(&env, &new_pool);
    assert_eq!(new_pool_client.balance(&usdc_token, &contract_id), 0);

    // Attempting to exit the position now silently "succeeds" without
    // recovering anything: withdraw_from_protocol queries `new_pool` (via
    // DataKey::DexPool), sees a 0 balance, treats the exit as complete, and
    // flips CurrentProtocol to "none" — even though the real funds are stuck
    // in `old_pool`.
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("none"),
        "vault believes it exited the position cleanly"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        0,
        "no funds were actually recovered to the vault"
    );
    assert_eq!(
        old_pool_client.balance(&usdc_token, &contract_id),
        deposit_amount,
        "funds remain permanently stranded in the old pool"
    );
}
