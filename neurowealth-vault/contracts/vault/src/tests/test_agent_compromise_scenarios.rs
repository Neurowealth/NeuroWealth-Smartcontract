//! Adversarial testing suite for agent-key compromise scenarios (Issue #673).
//!
//! Extends the OwnerCompromiseBlastRadius pattern and the isolated
//! `#596` suite in `test_adversarial_agent_simulation.rs` with
//! scenario-oriented attacks: a fully compromised agent key is used to try
//! stealing funds, changing ownership, rewriting storage, pausing,
//! upgrading, rotating pools, manipulating `TotalAssets`, and front-running
//! deposits.
//!
//! Every attack must fail with an authorization / identity error and leave
//! victim balances and privileged configuration unchanged.
//!
//! Auth modelling matches `#596`:
//! - Entrypoints with an explicit identity argument (`pause(owner)`, …)
//!   are invoked with the agent's address in that slot under
//!   `mock_all_auths()`. The contract's own equality check rejects them.
//! - Entrypoints that call `require_is_owner()` with no identity argument
//!   are invoked under [`mock_agent_only_auth`], so the owner's key is
//!   genuinely absent.

use super::utils::*;
extern crate std;
use crate::ApyPrediction;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, BytesN, Env, IntoVal};

fn setup_compromise() -> (Env, Address, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let victim = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &victim, 10_000_000_i128);

    (env, contract_id, agent, owner, usdc_token, victim)
}

fn assert_err_and_frozen(
    client: &NeuroWealthVaultClient,
    agent: &Address,
    victim: &Address,
    before: &VaultStateSnapshot,
    accepted: bool,
    label: &str,
) {
    assert!(!accepted, "compromised agent must not succeed: {label}");
    let after = snapshot_vault_state(client, &[agent.clone(), victim.clone()]);
    assert!(
        diff_vault_state(before, &after).is_empty(),
        "compromised agent mutated privileged state via {label}: {:?}",
        diff_vault_state(before, &after)
    );
    assert_eq!(
        client.get_shares(victim),
        before
            .watched
            .iter()
            .find(|(addr, _, _, _)| addr == victim)
            .map(|(_, _, shares, _)| *shares)
            .unwrap_or(0),
        "victim shares changed after {label}"
    );
}

// ============================================================================
// 1. AGENT TRIES TO CALL OWNER-ONLY FUNCTIONS
// ============================================================================

#[test]
fn test_agent_cannot_set_caps() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let user_cap = 1_000_000_i128;
    let tvl_cap = 50_000_000_000_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_caps",
        (user_cap, tvl_cap).into_val(&env),
    );
    let accepted = client.try_set_caps(&user_cap, &tvl_cap).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(&client, &agent, &victim, &before, accepted, "set_caps");
}

#[test]
fn test_agent_cannot_transfer_ownership() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let new_owner = Address::generate(&env);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "transfer_ownership",
        (new_owner.clone(),).into_val(&env),
    );
    let accepted = client.try_transfer_ownership(&new_owner).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "transfer_ownership",
    );
}

#[test]
fn test_agent_cannot_set_tvl_cap() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let cap = 1_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_tvl_cap",
        (cap,).into_val(&env),
    );
    let accepted = client.try_set_tvl_cap(&cap).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(&client, &agent, &victim, &before, accepted, "set_tvl_cap");
}

#[test]
fn test_agent_cannot_set_user_deposit_cap() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let cap = 1_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_user_deposit_cap",
        (cap,).into_val(&env),
    );
    let accepted = client.try_set_user_deposit_cap(&cap).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_user_deposit_cap",
    );
}

#[test]
fn test_agent_cannot_set_limits() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let (min, max) = (1_000_000_i128, 50_000_000_000_i128);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_limits",
        (min, max).into_val(&env),
    );
    let accepted = client.try_set_limits(&min, &max).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(&client, &agent, &victim, &before, accepted, "set_limits");
}

#[test]
fn test_agent_cannot_set_deposit_limits() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let (min, max) = (2_000_000_i128, 50_000_000_000_i128);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_deposit_limits",
        (min, max).into_val(&env),
    );
    let accepted = client.try_set_deposit_limits(&min, &max).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_deposit_limits",
    );
}

#[test]
fn test_agent_cannot_set_rebalance_cooldown() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let interval = 1_u32;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_rebalance_cooldown",
        (interval,).into_val(&env),
    );
    let accepted = client.try_set_rebalance_cooldown(&interval).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_rebalance_cooldown",
    );
}

#[test]
fn test_agent_cannot_set_max_consecutive_failures() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let threshold = 99_u32;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_max_consecutive_failures",
        (threshold,).into_val(&env),
    );
    let accepted = client.try_set_max_consecutive_failures(&threshold).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_max_consecutive_failures",
    );
}

#[test]
fn test_agent_cannot_set_approval_ttl() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let ttl = 2_000_u32;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_approval_ttl",
        (ttl,).into_val(&env),
    );
    let accepted = client.try_set_approval_ttl(&ttl).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_approval_ttl",
    );
}

#[test]
fn test_agent_cannot_update_agent() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let new_agent = Address::generate(&env);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "update_agent",
        (new_agent.clone(),).into_val(&env),
    );
    let accepted = client.try_update_agent(&new_agent).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(&client, &agent, &victim, &before, accepted, "update_agent");
}

#[test]
fn test_agent_cannot_confirm_agent_update() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "confirm_agent_update",
        ().into_val(&env),
    );
    let accepted = client.try_confirm_agent_update().is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "confirm_agent_update",
    );
}

#[test]
fn test_agent_cannot_cancel_agent_update() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "cancel_agent_update",
        ().into_val(&env),
    );
    let accepted = client.try_cancel_agent_update().is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "cancel_agent_update",
    );
}

#[test]
fn test_agent_cannot_cancel_ownership_transfer() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "cancel_ownership_transfer",
        ().into_val(&env),
    );
    let accepted = client.try_cancel_ownership_transfer().is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "cancel_ownership_transfer",
    );
}

#[test]
fn test_agent_cannot_emergency_harvest() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "emergency_harvest",
        (0_i128,).into_val(&env),
    );
    let accepted = client.try_emergency_harvest(&0_i128).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "emergency_harvest",
    );
}

#[test]
fn test_agent_cannot_set_min_holding_period() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_min_holding_period",
        (1_u32,).into_val(&env),
    );
    let accepted = client.try_set_min_holding_period(&1_u32).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_min_holding_period",
    );
}

#[test]
fn test_agent_cannot_set_max_acceptable_mev_loss() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_max_acceptable_mev_loss",
        (0_i128,).into_val(&env),
    );
    let accepted = client.try_set_max_acceptable_mev_loss(&0_i128).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_max_acceptable_mev_loss",
    );
}

#[test]
fn test_agent_cannot_set_migration_target() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let target = Address::generate(&env);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_migration_target",
        (target.clone(),).into_val(&env),
    );
    let accepted = client.try_set_migration_target(&target).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_migration_target",
    );
}

#[test]
fn test_agent_cannot_set_migration_paused() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_migration_paused",
        (true,).into_val(&env),
    );
    let accepted = client.try_set_migration_paused(&true).is_ok();
    env.mock_all_auths();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_migration_paused",
    );
}

#[test]
fn test_agent_cannot_set_min_withdrawal() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let amount = 1_000_000_i128;

    let accepted = client.try_set_min_withdrawal(&agent, &amount).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_min_withdrawal",
    );
}

#[test]
fn test_agent_cannot_set_queue_config() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_set_queue_config(&agent, &1_u32, &1_u64).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_queue_config",
    );
}

#[test]
fn test_agent_cannot_set_max_batch_size() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_set_max_batch_size(&agent, &1_u32).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_max_batch_size",
    );
}

#[test]
fn test_agent_cannot_set_blend_approval_ttl() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_set_blend_approval_ttl(&agent, &100_u32).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_blend_approval_ttl",
    );
}

#[test]
fn test_agent_cannot_accept_ownership() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_accept_ownership(&agent).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "accept_ownership",
    );
}

// ============================================================================
// 2. AGENT TRIES TO WITHDRAW USER FUNDS DIRECTLY
// ============================================================================

#[test]
fn test_agent_cannot_withdraw_victim_funds() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let victim_shares_before = client.get_shares(&victim);
    let amount = 1_000_000_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "withdraw",
        (victim.clone(), amount).into_val(&env),
    );
    let accepted = client.try_withdraw(&victim, &amount).is_ok();
    env.mock_all_auths();

    assert!(!accepted, "agent must not withdraw another user's funds");
    assert_eq!(client.get_shares(&victim), victim_shares_before);
}

#[test]
fn test_agent_cannot_withdraw_all_victim_funds() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let victim_shares_before = client.get_shares(&victim);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "withdraw_all",
        (victim.clone(),).into_val(&env),
    );
    let accepted = client.try_withdraw_all(&victim).is_ok();
    env.mock_all_auths();

    assert!(
        !accepted,
        "agent must not withdraw_all another user's funds"
    );
    assert_eq!(client.get_shares(&victim), victim_shares_before);
}

#[test]
fn test_agent_cannot_emergency_withdraw_victim_funds() {
    let (env, contract_id, agent, owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.pause(&owner);
    let victim_shares_before = client.get_shares(&victim);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "emergency_withdraw",
        (victim.clone(), 1_000_000_i128).into_val(&env),
    );
    let accepted = client
        .try_emergency_withdraw(&victim, &1_000_000_i128)
        .is_ok();
    env.mock_all_auths();

    assert!(
        !accepted,
        "agent must not emergency-withdraw another user's funds"
    );
    assert_eq!(client.get_shares(&victim), victim_shares_before);
}

#[test]
fn test_agent_cannot_cancel_victim_withdrawal_request() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "cancel_withdrawal_request",
        (victim.clone(), 1_u64).into_val(&env),
    );
    let accepted = client
        .try_cancel_withdrawal_request(&victim, &1_u64)
        .is_ok();
    env.mock_all_auths();
    assert!(
        !accepted,
        "agent must not cancel another user's withdrawal request"
    );
}

// ============================================================================
// 3. AGENT TRIES TO MODIFY CONTRACT STORAGE DIRECTLY
// ============================================================================

/// There is no public setter for `DataKey::Shares` / `TotalShares` /
/// `Owner`. This test hammers every mutating owner-gated entrypoint as the
/// agent and asserts victim shares, owner, and total_shares are unchanged.
#[test]
fn test_agent_cannot_modify_contract_storage_directly() {
    let (env, contract_id, agent, owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before_owner = client.get_owner();
    let before_shares_total = client.get_total_shares();
    let before_victim = client.get_shares(&victim);

    let _ = client.try_pause(&agent);
    let _ = client.try_unpause(&agent);
    let _ = client.try_emergency_pause(&agent);
    let _ = client.try_set_blend_pool(&agent, &agent);
    let _ = client.try_set_dex_pool(&agent, &agent);
    let _ = client.try_schedule_upgrade(&agent, &BytesN::from_array(&env, &[1u8; 32]));
    let _ = client.try_execute_upgrade(&agent);
    let _ = client.try_cancel_upgrade(&agent);
    let _ = client.try_accept_ownership(&agent);
    let _ = client.try_set_min_withdrawal(&agent, &1_i128);
    let _ = client.try_set_queue_config(&agent, &1_u32, &1_u64);
    let _ = client.try_set_max_batch_size(&agent, &1_u32);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_tvl_cap",
        (1_i128,).into_val(&env),
    );
    let _ = client.try_set_tvl_cap(&1_i128);
    env.mock_all_auths();

    assert_eq!(client.get_owner(), before_owner);
    assert_eq!(client.get_owner(), owner);
    assert_eq!(client.get_total_shares(), before_shares_total);
    assert_eq!(client.get_shares(&victim), before_victim);
    assert!(!client.is_paused());
}

// ============================================================================
// 4. AGENT TRIES TO PAUSE / UNPAUSE THE VAULT
// ============================================================================

#[test]
fn test_agent_cannot_pause_the_vault() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_pause(&agent).is_ok();
    assert_err_and_frozen(&client, &agent, &victim, &before, accepted, "pause");
    assert!(!client.is_paused());
}

#[test]
fn test_agent_cannot_unpause_the_vault() {
    let (env, contract_id, agent, owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.pause(&owner);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_unpause(&agent).is_ok();
    assert_err_and_frozen(&client, &agent, &victim, &before, accepted, "unpause");
    assert!(client.is_paused());
}

#[test]
fn test_agent_cannot_emergency_pause_the_vault() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_emergency_pause(&agent).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "emergency_pause",
    );
}

// ============================================================================
// 5. AGENT TRIES TO UPGRADE THE CONTRACT
// ============================================================================

#[test]
fn test_agent_cannot_schedule_upgrade() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let fake_hash = BytesN::from_array(&env, &[7u8; 32]);

    let accepted = client.try_schedule_upgrade(&agent, &fake_hash).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "schedule_upgrade",
    );
}

#[test]
fn test_agent_cannot_execute_upgrade() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_execute_upgrade(&agent).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "execute_upgrade",
    );
}

#[test]
fn test_agent_cannot_cancel_upgrade() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);

    let accepted = client.try_cancel_upgrade(&agent).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "cancel_upgrade",
    );
}

// ============================================================================
// 6. AGENT TRIES TO SET POOL ADDRESSES TO DRAIN CONTRACTS
// ============================================================================

#[test]
fn test_agent_cannot_set_blend_pool_to_drain_address() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let drain = Address::generate(&env);

    let accepted = client.try_set_blend_pool(&agent, &drain).is_ok();
    assert_err_and_frozen(
        &client,
        &agent,
        &victim,
        &before,
        accepted,
        "set_blend_pool",
    );
    assert_eq!(client.get_blend_pool(), before.blend_pool);
}

#[test]
fn test_agent_cannot_set_dex_pool_to_drain_address() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone(), victim.clone()]);
    let drain = Address::generate(&env);

    let accepted = client.try_set_dex_pool(&agent, &drain).is_ok();
    assert_err_and_frozen(&client, &agent, &victim, &before, accepted, "set_dex_pool");
    assert_eq!(client.get_dex_pool(), before.dex_pool);
}

// ============================================================================
// 7. AGENT TRIES TO MANIPULATE TOTALASSETS ARBITRARILY
// ============================================================================

#[test]
fn test_agent_cannot_inflate_total_assets_beyond_backing() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before_assets = client.get_total_assets();
    let victim_shares = client.get_shares(&victim);

    let accepted = client
        .try_update_total_assets(&agent, &(before_assets + 1_000_000_000_i128), &false, &0)
        .is_ok();

    assert!(!accepted, "unbacked inflate must fail");
    assert_eq!(client.get_total_assets(), before_assets);
    assert_eq!(client.get_shares(&victim), victim_shares);
}

#[test]
fn test_agent_cannot_decrease_total_assets_without_owner() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before_assets = client.get_total_assets();
    let victim_shares = client.get_shares(&victim);

    let accepted = client
        .try_update_total_assets(&agent, &1_i128, &true, &1000)
        .is_ok();

    assert!(!accepted, "decrease without owner co-sign must fail");
    assert_eq!(client.get_total_assets(), before_assets);
    assert_eq!(client.get_shares(&victim), victim_shares);
}

#[test]
fn test_non_agent_cannot_update_total_assets() {
    let (env, contract_id, _agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before_assets = client.get_total_assets();

    let accepted = client
        .try_update_total_assets(&victim, &before_assets, &false, &0)
        .is_ok();
    assert!(!accepted, "only the stored agent may update total_assets");
    assert_eq!(client.get_total_assets(), before_assets);
}

// ============================================================================
// 8. AGENT TRIES TO FRONT-RUN USER DEPOSITS
// ============================================================================

/// Classic sandwich: agent deposits first, tries to inflate the share
/// price, lets the victim deposit, then withdraws. Inflation is rejected
/// (storage-based accounting + solvency check), so the agent cannot extract
/// the victim's principal.
#[test]
fn test_agent_cannot_front_run_user_deposit() {
    let (env, contract_id, agent, _owner, usdc_token, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let victim_shares_before = client.get_shares(&victim);

    // Agent front-runs with its own deposit (allowed: agent acting as a user).
    let agent_deposit = 1_000_000_i128;
    token.mint(&agent, &agent_deposit);
    client.deposit(&agent, &agent_deposit);
    let agent_shares = client.get_shares(&agent);

    // Agent tries to inflate the exchange rate before the victim's next deposit.
    let inflated = client.get_total_assets() + 50_000_000_i128;
    let inflated_ok = client
        .try_update_total_assets(&agent, &inflated, &false, &0)
        .is_ok();
    assert!(!inflated_ok, "front-run inflate must be rejected");

    // Victim deposits more.
    let extra = 5_000_000_i128;
    token.mint(&victim, &extra);
    client.deposit(&victim, &extra);

    // Agent withdraws only its own position.
    client.withdraw(&agent, &agent_deposit);

    assert!(
        client.get_shares(&agent) <= agent_shares,
        "agent must not mint extra shares from the sandwich"
    );
    assert!(
        client.get_shares(&victim) >= victim_shares_before,
        "victim shares must not decrease because of the sandwich"
    );
}

// ============================================================================
// AGENT ENTRY POINTS: allowed surface still cannot steal
// ============================================================================

#[test]
fn test_agent_rebalance_cannot_touch_user_shares() {
    let (env, contract_id, agent, owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let victim_shares = client.get_shares(&victim);

    client.rebalance(&symbol_short!("none"), &500_i128, &0_i128);

    assert_eq!(client.get_shares(&victim), victim_shares);
    assert_eq!(client.get_owner(), owner);
}

#[test]
fn test_agent_submit_mev_report_cannot_steal() {
    let (env, contract_id, _agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let victim_shares = client.get_shares(&victim);
    let assets = client.get_total_assets();

    client.submit_mev_report(&symbol_short!("blend"), &1_i128, &0_i128);

    assert_eq!(client.get_shares(&victim), victim_shares);
    assert_eq!(client.get_total_assets(), assets);
}

#[test]
fn test_agent_submit_apy_prediction_cannot_steal() {
    let (env, contract_id, _agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let victim_shares = client.get_shares(&victim);

    client.submit_apy_prediction(&ApyPrediction {
        protocol: symbol_short!("blend"),
        predicted_apy_bps: 800,
        apy_1h_bps: 800,
        apy_6h_bps: 800,
        apy_24h_bps: 800,
        confidence_bps: 9_000,
        submitted_at_ledger: 0,
    });

    assert_eq!(client.get_shares(&victim), victim_shares);
}

#[test]
fn test_agent_process_withdrawal_queue_cannot_steal_shares() {
    let (env, contract_id, agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let victim_shares = client.get_shares(&victim);

    let _ = client.process_withdrawal_queue(&agent, &10_u32);

    assert_eq!(client.get_shares(&victim), victim_shares);
}

#[test]
fn test_random_user_cannot_impersonate_agent_rebalance() {
    let (env, contract_id, _agent, _owner, _usdc, victim) = setup_compromise();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &victim,
        "rebalance",
        (symbol_short!("none"), 500_i128, 0_i128).into_val(&env),
    );
    let accepted = client
        .try_rebalance(&symbol_short!("none"), &500_i128, &0_i128)
        .is_ok();
    env.mock_all_auths();
    assert!(!accepted, "non-agent must not rebalance");
}
