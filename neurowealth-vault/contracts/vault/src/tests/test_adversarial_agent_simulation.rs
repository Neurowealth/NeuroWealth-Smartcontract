//! Adversarial-agent simulation suite (#596).
//!
//! Simulates a fully compromised agent — one that controls only its own
//! signing key, never the owner's — invoking every public entrypoint on the
//! vault. After each call, the vault's full observable state is snapshotted
//! and diffed (see [`snapshot_vault_state`] / [`diff_vault_state`] in
//! `tests/utils.rs`). Any mutation outside the agent's allowed set is a
//! privilege-escalation bug.
//!
//! ## Allowed-mutation allowlist
//!
//! The agent's role (`require_is_agent`) legitimately gates exactly three
//! entrypoints: `rebalance`, `harvest`, and the *increase* path of
//! `update_total_assets` (a *decrease* additionally requires the owner's
//! co-signature via `require_is_owner`, per the #477 fix). Calling these as
//! the agent is expected to succeed and is allowed to mutate:
//! - `current_protocol`, `last_rebalance_ledger`, `consecutive_failures`,
//!   `paused` (rebalance/harvest — the circuit breaker in
//!   `record_rebalance_outcome` can auto-pause after repeated failures; not
//!   exercised by the single-call happy paths below, but documented here so
//!   a future `paused` flip during rebalance is not mistaken for a bug)
//! - `total_assets` (update_total_assets, harvest)
//!
//! Separately, `deposit`, `batch_deposit`, `withdraw`, `withdraw_all`, and
//! `set_user_strategy` carry **no role gate at all** — they are keyed to
//! `user.require_auth()` on whichever address is passed in, so any address
//! (including the agent's own) may call them as an ordinary depositor. That
//! is not a privilege escalation; it is identical to what any other wallet
//! could do. Calling these as the agent is allowed to mutate `total_assets`,
//! `total_shares`, and the agent's own watched `(balance, shares, strategy)`
//! entry — nothing else.
//!
//! Every other public entrypoint — all owner-gated configuration, timelocked
//! upgrade/agent-rotation, pause control, and ownership transfer — must
//! reject a compromised agent's call outright, with **zero** observable
//! state mutation.
//!
//! ## Two ways an owner-only call is rejected
//!
//! - **Explicit identity parameter** (`pause(owner)`, `schedule_upgrade(owner, ..)`,
//!   `update_total_assets(agent, ..)`, `accept_ownership(new_owner)`, ...): the
//!   compromised agent supplies its own address in the identity slot. Auth
//!   succeeds (the agent did sign this call), but the contract's own
//!   `address == stored_address` check rejects it. These tests keep
//!   `env.mock_all_auths()` from setup active and rely on that check.
//! - **Implicit owner check** (`set_tvl_cap`, `update_agent`, `transfer_ownership`,
//!   ...): the function takes no identity parameter at all — it fetches the
//!   real owner from storage and calls `owner.require_auth()` on *that*
//!   address. There is no caller-identity slot to swap, so the only way to
//!   simulate "agent without the owner's key" is to scope authorization to
//!   *only* the agent via [`mock_agent_only_auth`], leaving the owner
//!   unauthorized. These tests explicitly re-scope auth before the call.

use super::utils::*;
extern crate std;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, BytesN, Env, IntoVal, Symbol};

/// Boots a vault, funds a real (non-agent) depositor so `total_assets` /
/// `total_shares` are non-zero, and returns the pieces every test needs.
/// `env.mock_all_auths()` remains active on return — sufficient for the
/// identity-parameter-swap group; the implicit-owner-check group re-scopes
/// auth per call via [`mock_agent_only_auth`].
fn setup_baseline() -> (Env, Address, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let victim = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &victim, 10_000_000_i128);

    (env, contract_id, agent, owner, usdc_token)
}

// ============================================================================
// GROUP 1 — EXPLICIT IDENTITY PARAMETER: agent passes its own address where
// the owner/agent identity is expected; the contract's own equality check
// must reject it with no state mutation.
// ============================================================================

#[test]
fn test_agent_cannot_pause() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    let result = client.try_pause(&agent);

    assert!(
        result.is_err(),
        "compromised agent must not be able to pause"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_unpause() {
    let (env, contract_id, agent, owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.pause(&owner);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    let result = client.try_unpause(&agent);

    assert!(
        result.is_err(),
        "compromised agent must not be able to unpause"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_emergency_pause() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    let result = client.try_emergency_pause(&agent);

    assert!(
        result.is_err(),
        "compromised agent must not be able to emergency-pause"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_blend_pool() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let fake_pool = Address::generate(&env);

    let result = client.try_set_blend_pool(&agent, &fake_pool);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set the Blend pool"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_dex_pool() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let fake_pool = Address::generate(&env);

    let result = client.try_set_dex_pool(&agent, &fake_pool);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set the DEX pool"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_blend_approval_ttl() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    let result = client.try_set_blend_approval_ttl(&agent, &100_u32);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set the Blend approval TTL"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_schedule_upgrade() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let fake_hash = BytesN::from_array(&env, &[7u8; 32]);

    let result = client.try_schedule_upgrade(&agent, &fake_hash);

    assert!(
        result.is_err(),
        "compromised agent must not be able to schedule an upgrade"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_execute_upgrade() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    let result = client.try_execute_upgrade(&agent);

    assert!(
        result.is_err(),
        "compromised agent must not be able to execute an upgrade"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_cancel_upgrade() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    let result = client.try_cancel_upgrade(&agent);

    assert!(
        result.is_err(),
        "compromised agent must not be able to cancel an upgrade"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_accept_ownership() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    // No transfer was ever initiated toward the agent — accept must reject.
    let result = client.try_accept_ownership(&agent);

    assert!(
        result.is_err(),
        "compromised agent must not be able to accept ownership"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_decrease_total_assets_without_owner_cosign() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    // Attempted decrease: allow_decrease=true, but only the agent's own
    // legitimate auth is present (mock_all_auths makes every address's
    // auth succeed here — the #477 owner-cosign business rule is what must
    // reject this, exercised precisely under scoped auth in Group 2).
    let result = client.try_update_total_assets(&agent, &1_i128, &true, &1000_u32);

    assert!(
        result.is_err(),
        "agent alone must not be able to decrease total_assets"
    );
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_owner_cannot_impersonate_agent_in_update_total_assets() {
    // Complements the agent-side checks: proves the identity check is
    // exclusive in both directions, matching #118.
    let (env, contract_id, _agent, owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[owner.clone()]);

    let result = client.try_update_total_assets(&owner, &10_000_000_i128, &false, &0_u32);

    assert!(
        result.is_err(),
        "owner must not be able to impersonate the agent"
    );
    let after = snapshot_vault_state(&client, &[owner.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

// ============================================================================
// GROUP 2 — IMPLICIT OWNER CHECK: no identity parameter to swap. Auth is
// explicitly scoped to *only* the agent via `mock_agent_only_auth`, so
// `require_is_owner`'s internal `owner.require_auth()` has no matching
// signature and must fail.
// ============================================================================

#[test]
fn test_agent_cannot_set_tvl_cap() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let cap = 50_000_000_000_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_tvl_cap",
        (cap,).into_val(&env),
    );
    let result = client.try_set_tvl_cap(&cap);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set the TVL cap"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_user_deposit_cap() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let cap = 25_000_000_000_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_user_deposit_cap",
        (cap,).into_val(&env),
    );
    let result = client.try_set_user_deposit_cap(&cap);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set the per-user deposit cap"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_caps() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let user_cap = 1_000_000_i128;
    let tvl_cap = 50_000_000_000_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_caps",
        (user_cap, tvl_cap).into_val(&env),
    );
    let result = client.try_set_caps(&user_cap, &tvl_cap);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set caps"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_limits() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let min = 1_000_000_i128;
    let max = 50_000_000_000_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_limits",
        (min, max).into_val(&env),
    );
    let result = client.try_set_limits(&min, &max);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set limits"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_deposit_limits() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let min = 2_000_000_i128;
    let max = 50_000_000_000_i128;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_deposit_limits",
        (min, max).into_val(&env),
    );
    let result = client.try_set_deposit_limits(&min, &max);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set deposit limits"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_rebalance_cooldown() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let interval = 720_u32;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_rebalance_cooldown",
        (interval,).into_val(&env),
    );
    let result = client.try_set_rebalance_cooldown(&interval);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set the rebalance cooldown"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_max_consecutive_failures() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let threshold = 5_u32;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_max_consecutive_failures",
        (threshold,).into_val(&env),
    );
    let result = client.try_set_max_consecutive_failures(&threshold);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set the max-consecutive-failures threshold"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_set_approval_ttl() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let ttl = 2_000_u32;

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "set_approval_ttl",
        (ttl,).into_val(&env),
    );
    let result = client.try_set_approval_ttl(&ttl);

    assert!(
        result.is_err(),
        "compromised agent must not be able to set the approval TTL"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_update_agent() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let new_agent = Address::generate(&env);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "update_agent",
        (new_agent.clone(),).into_val(&env),
    );
    let result = client.try_update_agent(&new_agent);

    assert!(
        result.is_err(),
        "compromised agent must not be able to rotate the agent"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_confirm_agent_update() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "confirm_agent_update",
        ().into_val(&env),
    );
    let result = client.try_confirm_agent_update();

    assert!(
        result.is_err(),
        "compromised agent must not be able to confirm an agent update"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_cancel_agent_update() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "cancel_agent_update",
        ().into_val(&env),
    );
    let result = client.try_cancel_agent_update();

    assert!(
        result.is_err(),
        "compromised agent must not be able to cancel an agent update"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_transfer_ownership() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let new_owner = Address::generate(&env);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "transfer_ownership",
        (new_owner.clone(),).into_val(&env),
    );
    let result = client.try_transfer_ownership(&new_owner);

    assert!(
        result.is_err(),
        "compromised agent must not be able to initiate an ownership transfer"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_cancel_ownership_transfer() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "cancel_ownership_transfer",
        ().into_val(&env),
    );
    let result = client.try_cancel_ownership_transfer();

    assert!(
        result.is_err(),
        "compromised agent must not be able to cancel an ownership transfer"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

#[test]
fn test_agent_cannot_emergency_harvest() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    mock_agent_only_auth(
        &env,
        &contract_id,
        &agent,
        "emergency_harvest",
        (0_i128,).into_val(&env),
    );
    let result = client.try_emergency_harvest(&0_i128);

    assert!(
        result.is_err(),
        "compromised agent must not be able to call emergency_harvest"
    );
    env.mock_all_auths();
    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
}

// ============================================================================
// GROUP 3 — AGENT'S ALLOWED SET: these are expected to succeed and must only
// mutate the fields documented in the module-level allowlist above.
// ============================================================================

#[test]
fn test_agent_rebalance_mutates_only_allowlisted_fields() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    client.rebalance(&symbol_short!("none"), &500_i128, &0_i128);

    let after = snapshot_vault_state(&client, &[agent.clone()]);
    let changed = diff_vault_state(&before, &after);
    for field in &changed {
        assert!(
            matches!(
                *field,
                "current_protocol" | "last_rebalance_ledger" | "consecutive_failures" | "paused"
            ),
            "rebalance mutated disallowed field: {field}"
        );
    }
    assert!(
        diff_watched_addresses(&before, &after).is_empty(),
        "rebalance must not touch user balances"
    );
}

#[test]
fn test_agent_can_increase_total_assets_within_backing() {
    let (env, contract_id, agent, _owner, usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    // Simulate yield arriving so the increase is actually backed.
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&contract_id, &1_000_000_i128);

    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let new_total = before.total_assets + 1_000_000_i128;

    client.update_total_assets(&agent, &new_total, &false, &0_u32);

    let after = snapshot_vault_state(&client, &[agent.clone()]);
    let changed = diff_vault_state(&before, &after);
    for field in &changed {
        assert!(
            *field == "total_assets",
            "update_total_assets mutated disallowed field: {field}"
        );
    }
    assert_eq!(after.total_assets, new_total);
    assert!(diff_watched_addresses(&before, &after).is_empty());
}

#[test]
fn test_agent_self_deposit_mutates_only_allowlisted_fields() {
    let (env, contract_id, agent, _owner, usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);
    let amount = 5_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &agent, amount);

    let after = snapshot_vault_state(&client, &[agent.clone()]);
    let changed = diff_vault_state(&before, &after);
    for field in &changed {
        assert!(
            matches!(*field, "total_assets" | "total_shares"),
            "agent self-deposit mutated disallowed field: {field}"
        );
    }
    // The only watched address permitted to change is the agent itself, and
    // only because it acted as an ordinary depositor.
    let touched = diff_watched_addresses(&before, &after);
    assert_eq!(touched.len(), 1);
    assert_eq!(touched[0], agent);
}

#[test]
fn test_agent_self_withdraw_mutates_only_allowlisted_fields() {
    let (env, contract_id, agent, _owner, usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &agent, amount);

    let before = snapshot_vault_state(&client, &[agent.clone()]);
    client.withdraw(&agent, &amount, &None);

    let after = snapshot_vault_state(&client, &[agent.clone()]);
    let changed = diff_vault_state(&before, &after);
    for field in &changed {
        assert!(
            matches!(*field, "total_assets" | "total_shares"),
            "agent self-withdraw mutated disallowed field: {field}"
        );
    }
    let touched = diff_watched_addresses(&before, &after);
    assert_eq!(touched.len(), 1);
    assert_eq!(touched[0], agent);
    assert_eq!(
        client.get_shares(&agent),
        0,
        "agent shares should be fully withdrawn"
    );
}

#[test]
fn test_agent_set_own_strategy_does_not_touch_singleton_state() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    client.set_user_strategy(&agent, &Symbol::new(&env, "growth"));

    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert!(diff_vault_state(&before, &after).is_empty());
    assert_eq!(
        client.get_user_strategy(&agent),
        Symbol::new(&env, "growth")
    );
}

// ============================================================================
// GROUP 4 — READ-ONLY SURFACE: invoking every getter as the agent must never
// mutate observable state.
// ============================================================================

#[test]
fn test_agent_invoking_every_getter_causes_no_mutation() {
    let (env, contract_id, agent, _owner, _usdc_token) = setup_baseline();
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let before = snapshot_vault_state(&client, &[agent.clone()]);

    let _ = client.get_owner();
    let _ = client.get_agent();
    let _ = client.is_paused();
    let _ = client.get_version();
    let _ = client.get_usdc_token();
    let _ = client.get_current_protocol();
    let _ = client.get_blend_pool();
    let _ = client.get_dex_pool();
    let _ = client.get_blend_approval_ttl();
    let _ = client.get_exchange_rate();
    let _ = client.get_idle_balance();
    let _ = client.get_deployed_assets();
    let _ = client.get_asset_breakdown();
    let _ = client.get_tvl_cap();
    let _ = client.get_user_deposit_cap();
    let _ = client.get_min_deposit();
    let _ = client.get_max_deposit();
    let _ = client.get_rebalance_cooldown();
    let _ = client.get_last_rebalance_ledger();
    let _ = client.get_max_consecutive_failures();
    let _ = client.get_consecutive_failures();
    let _ = client.get_approval_ttl();
    let _ = client.get_pending_owner();
    let _ = client.get_pending_ownership();
    let _ = client.get_pending_agent_update();
    let _ = client.get_pending_upgrade();
    let _ = client.get_total_deposits();
    let _ = client.get_total_assets();
    let _ = client.get_total_shares();
    let _ = client.get_balance(&agent);
    let _ = client.get_shares(&agent);
    let _ = client.get_user_strategy(&agent);
    let _ = client.get_user_info(&agent);
    let _ = client.get_users_with_shares(&0_u32, &10_u32);
    let _ = client.preview_deposit_to_shares(&1_000_000_i128);
    let _ = client.preview_shares_to_assets(&1_000_000_i128);
    let _ = client.preview_withdraw(&1_000_000_i128);
    let _ = client.convert_to_shares(&1_000_000_i128);
    let _ = client.convert_to_assets(&1_000_000_i128);
    // touch_user_ttl has no role gate and only bumps a ledger TTL, which is
    // not part of the observable-value snapshot.
    let _ = client.touch_user_ttl(&agent);

    let after = snapshot_vault_state(&client, &[agent.clone()]);
    assert_eq!(
        before, after,
        "read-only entrypoints must never mutate vault state"
    );
}
