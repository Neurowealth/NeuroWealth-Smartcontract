//! Tests for per-user investment strategy preference (Issue #227)

use super::utils::*;
use crate::{UserStrategyUpdatedEvent, TOPIC_USER_STRATEGY_UPDATED};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, Symbol, TryFromVal};

fn setup(env: &Env) -> (Address, NeuroWealthVaultClient<'_>, Address, Address) {
    env.mock_all_auths();
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(env);
    let client = NeuroWealthVaultClient::new(env, &contract_id);
    (contract_id, client, agent, usdc_token)
}

#[test]
fn test_set_and_get_strategy() {
    let env = Env::default();
    let (_contract_id, client, _agent, _usdc_token) = setup(&env);
    let user = Address::generate(&env);

    // Default should be balanced before any strategy is set
    let default = client.get_user_strategy(&user);
    assert_eq!(
        default,
        Symbol::new(&env, "balanced"),
        "default strategy should be balanced"
    );

    // Set to conservative
    client.set_user_strategy(&user, &Symbol::new(&env, "conservative"));
    let strategy = client.get_user_strategy(&user);
    assert_eq!(
        strategy,
        Symbol::new(&env, "conservative"),
        "strategy should be conservative"
    );

    // Change to growth
    client.set_user_strategy(&user, &Symbol::new(&env, "growth"));
    let strategy = client.get_user_strategy(&user);
    assert_eq!(
        strategy,
        Symbol::new(&env, "growth"),
        "strategy should be growth"
    );
}

#[test]
fn test_strategies_are_independent_per_user() {
    let env = Env::default();
    let (_contract_id, client, _agent, _usdc_token) = setup(&env);
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);

    client.set_user_strategy(&user_a, &Symbol::new(&env, "conservative"));
    client.set_user_strategy(&user_b, &Symbol::new(&env, "growth"));

    assert_eq!(
        client.get_user_strategy(&user_a),
        Symbol::new(&env, "conservative")
    );
    assert_eq!(
        client.get_user_strategy(&user_b),
        Symbol::new(&env, "growth")
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_invalid_strategy_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client, _agent, _usdc_token) = setup(&env);
    let user = Address::generate(&env);
    client.set_user_strategy(&user, &Symbol::new(&env, "invalid"));
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_empty_strategy_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client, _agent, _usdc_token) = setup(&env);
    let user = Address::generate(&env);
    client.set_user_strategy(&user, &Symbol::new(&env, ""));
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_uppercase_strategy_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client, _agent, _usdc_token) = setup(&env);
    let user = Address::generate(&env);
    client.set_user_strategy(&user, &Symbol::new(&env, "CONSERVATIVE"));
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_mixed_case_strategy_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client, _agent, _usdc_token) = setup(&env);
    let user = Address::generate(&env);
    client.set_user_strategy(&user, &Symbol::new(&env, "Balanced"));
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn test_symbol_longer_than_nine_chars_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client, _agent, _usdc_token) = setup(&env);
    let user = Address::generate(&env);
    client.set_user_strategy(&user, &Symbol::new(&env, "safeststrategy"));
}

#[test]
#[should_panic(expected = "Error(Contract, #36)")]
fn test_set_strategy_before_init_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let deployer = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    client.set_user_strategy(&user, &Symbol::new(&env, "balanced"));
}

#[test]
fn test_default_strategy_on_first_deposit() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client, _agent, usdc_token) = setup(&env);
    let user = Address::generate(&env);

    // Before deposit, strategy should be default
    assert_eq!(
        client.get_user_strategy(&user),
        Symbol::new(&env, "balanced"),
        "default strategy before deposit should be balanced"
    );

    // Mint tokens and deposit
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &10_000_000);
    client.deposit(&user, &10_000_000);

    // After first deposit, strategy should still be balanced (default set)
    assert_eq!(
        client.get_user_strategy(&user),
        Symbol::new(&env, "balanced"),
        "strategy after first deposit should be balanced"
    );

    // User can change strategy after deposit
    client.set_user_strategy(&user, &Symbol::new(&env, "growth"));
    assert_eq!(
        client.get_user_strategy(&user),
        Symbol::new(&env, "growth"),
        "strategy should be changeable after deposit"
    );
}

#[test]
fn test_set_strategy_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (_contract_id, client, _agent, _usdc_token) = setup(&env);
    let user = Address::generate(&env);

    client.set_user_strategy(&user, &Symbol::new(&env, "conservative"));

    let strategy_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_USER_STRATEGY_UPDATED);
    assert_eq!(
        strategy_events.len(),
        1,
        "Exactly one usr_strat event should be emitted"
    );

    let (_, _, data) = &strategy_events[0];
    let event = UserStrategyUpdatedEvent::try_from_val(&env, data)
        .expect("Should be a valid UserStrategyUpdatedEvent");
    assert_eq!(event.user, user, "Event user should match caller");
    assert_eq!(
        event.new_strategy,
        Symbol::new(&env, "conservative"),
        "Event strategy should match set value"
    );
}
