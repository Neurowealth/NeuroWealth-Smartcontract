//! Per-user strategy storage and authorization tests

use super::utils::*;
use crate::{
NeuroWealthVaultClient, UserStrategyUpdatedEvent, TOPIC_USER_STRATEGY_UPDATED,
};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, TryFromVal};

const STRATEGY_BALANCED: &str = "balanced";
const STRATEGY_GROWTH: &str = "growth";
const STRATEGY_DEFENSIVE: &str = "defens";

fn supported_symbols(_env: &Env) -> [soroban_sdk::Symbol; 3] {
    [
        symbol_short!("balanced"),
        symbol_short!("growth"),
        symbol_short!("defens"),
    ]
}

#[test]
fn test_set_get_user_strategy_roundtrip_for_all_symbols() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

for strat in supported_symbols(&env).iter() {
        client.set_user_strategy(&user, strat);
        let stored = client.get_user_strategy(&user);
        assert_eq!(stored, strat.clone(), "stored strategy must match set strategy");
    }
}

#[test]
fn test_set_user_strategy_requires_user_auth() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let attacker = Address::generate(&env);

    env.mock_auths(&[]);

    let res = client.try_set_user_strategy(&attacker, &symbol_short!("growth"));
    assert!(res.is_err(), "should fail without user authorization");

    // Ensure attacker did not set
    let stored = client.get_user_strategy(&attacker);
    assert_eq!(stored, symbol_short!("balanced"), "unknown attacker should read default");
}

#[test]
fn test_set_user_strategy_event_emits_correct_old_and_new() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);

    // First deposit not strictly required for the event, but default read
    // should be "balanced".
    let old = client.get_user_strategy(&user);
    assert_eq!(old, symbol_short!("balanced"));

    client.set_user_strategy(&user, &symbol_short!("growth"));

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_USER_STRATEGY_UPDATED);
    assert_eq!(events.len(), 1, "one strategy update event expected");

    let (_, _, data) = &events[0];
    let event = UserStrategyUpdatedEvent::try_from_val(&env, data)
        .expect("Should decode UserStrategyUpdatedEvent");

    assert_eq!(event.user, user);
    assert_eq!(event.old_strategy, symbol_short!("balanced"));
    assert_eq!(event.new_strategy, symbol_short!("growth"));
}

#[test]
fn test_default_strategy_is_set_on_first_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;

    // Ensure user strategy not previously set
    let before = client.get_user_strategy(&user);
    assert_eq!(before, symbol_short!("balanced"));

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    let after = client.get_user_strategy(&user);
    assert_eq!(after, symbol_short!("balanced"));
}

