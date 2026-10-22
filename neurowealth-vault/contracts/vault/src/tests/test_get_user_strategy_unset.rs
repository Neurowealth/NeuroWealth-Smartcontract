//! Tests for get_user_strategy() returning default for unset users (Issue #227).
//!
//! Verify that get_user_strategy() returns the default strategy for users who
//! have never called set_user_strategy(). The getter should return a sensible
//! default ("balanced") rather than panicking or returning garbage data.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

/// Test that a completely new user (never interacted) gets the default strategy.
#[test]
fn test_get_user_strategy_unset_returns_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Generate a new address that has never interacted with the vault
    let unset_user = Address::generate(&env);

    // get_user_strategy should return the default, not panic
    let strategy = client.get_user_strategy(&unset_user);
    assert_eq!(
        strategy,
        Symbol::new(&env, "balanced"),
        "unset user should have default 'balanced' strategy"
    );
}

/// Edge case: User who deposited but never set a strategy.
#[test]
fn test_get_user_strategy_after_deposit_without_set_returns_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let deposit_amount = 5_000_000_i128;

    // User deposits but never calls set_user_strategy
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Strategy should still be the default
    let strategy = client.get_user_strategy(&user);
    assert_eq!(
        strategy,
        Symbol::new(&env, "balanced"),
        "deposited user who never set strategy should have default 'balanced'"
    );

    // User still has shares (deposit was successful)
    let shares = client.get_shares(&user);
    assert!(shares > 0, "user should have positive shares after deposit");

    // Verify token balance was transferred
    assert_eq!(
        token_client.balance(&contract_id),
        deposit_amount,
        "vault should hold the deposited amount"
    );
}

/// Edge case: Owner address (non-depositor) gets default strategy.
#[test]
fn test_get_user_strategy_for_owner_returns_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Owner should also get the default strategy (owner never sets their own strategy)
    let strategy = client.get_user_strategy(&owner);
    assert_eq!(
        strategy,
        Symbol::new(&env, "balanced"),
        "owner address should have default 'balanced' strategy"
    );
}

/// Edge case: Agent address (non-depositor) gets default strategy.
#[test]
fn test_get_user_strategy_for_agent_returns_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Agent should also get the default strategy
    let strategy = client.get_user_strategy(&agent);
    assert_eq!(
        strategy,
        Symbol::new(&env, "balanced"),
        "agent address should have default 'balanced' strategy"
    );
}

/// Verify multiple unset users all get the same default independently.
#[test]
fn test_multiple_unset_users_each_get_default() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let user_c = Address::generate(&env);

    // All unset users should get the same default
    assert_eq!(
        client.get_user_strategy(&user_a),
        Symbol::new(&env, "balanced")
    );
    assert_eq!(
        client.get_user_strategy(&user_b),
        Symbol::new(&env, "balanced")
    );
    assert_eq!(
        client.get_user_strategy(&user_c),
        Symbol::new(&env, "balanced")
    );

    // Changing one user's strategy shouldn't affect others
    client.set_user_strategy(&user_a, &Symbol::new(&env, "conservative"));
    assert_eq!(
        client.get_user_strategy(&user_a),
        Symbol::new(&env, "conservative"),
        "user_a should have conservative after set"
    );
    assert_eq!(
        client.get_user_strategy(&user_b),
        Symbol::new(&env, "balanced"),
        "user_b should still have default balanced"
    );
    assert_eq!(
        client.get_user_strategy(&user_c),
        Symbol::new(&env, "balanced"),
        "user_c should still have default balanced"
    );
}

/// Test that get_user_strategy is read-only and doesn't mutate state.
#[test]
fn test_get_user_strategy_does_not_mutate() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let unset_user = Address::generate(&env);

    // Call get_user_strategy multiple times on the same unset user
    let strategy1 = client.get_user_strategy(&unset_user);
    let strategy2 = client.get_user_strategy(&unset_user);
    let strategy3 = client.get_user_strategy(&unset_user);

    // All calls should return the same default
    assert_eq!(strategy1, Symbol::new(&env, "balanced"));
    assert_eq!(strategy2, Symbol::new(&env, "balanced"));
    assert_eq!(strategy3, Symbol::new(&env, "balanced"));

    // Now set a strategy and verify it changes
    client.set_user_strategy(&unset_user, &Symbol::new(&env, "growth"));
    let strategy_after_set = client.get_user_strategy(&unset_user);
    assert_eq!(
        strategy_after_set,
        Symbol::new(&env, "growth"),
        "strategy should change after set_user_strategy is called"
    );
}
