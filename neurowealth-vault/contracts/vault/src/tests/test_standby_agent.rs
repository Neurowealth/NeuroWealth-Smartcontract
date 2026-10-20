#![cfg(test)]

use super::utils::*;
use crate::{
    AgentKeyRotatedEvent, StandbyAgentUpdatedEvent, TOPIC_AGENT_KEY_ROTATED,
    TOPIC_STANDBY_AGENT_UPDATED,
};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, TryFromVal,
};

#[test]
fn test_standby_agent_rotation_updates_roles_and_emits_payloads() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, old_primary, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let new_primary = Address::generate(&env);

    assert_eq!(client.get_standby_agent(), None);
    client.update_standby_agent(&new_primary);
    assert_eq!(client.get_standby_agent(), Some(new_primary.clone()));

    let updated_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_STANDBY_AGENT_UPDATED);
    assert_eq!(updated_events.len(), 1);
    let updated = StandbyAgentUpdatedEvent::try_from_val(&env, &updated_events[0].2).unwrap();
    assert_eq!(updated.old_standby, None);
    assert_eq!(updated.new_standby, new_primary);
    assert_eq!(updated.owner, owner);
    assert_eq!(updated.timestamp, env.ledger().timestamp());

    // Switchover is immediate — no timelock (#653).
    client.switch_to_standby_agent();

    assert_eq!(client.get_agent(), new_primary);
    assert_eq!(client.get_standby_agent(), Some(old_primary.clone()));
    assert_ne!(client.get_agent(), old_primary);

    let rotated_events = find_events_by_topic(env.events().all(), &env, TOPIC_AGENT_KEY_ROTATED);
    assert_eq!(rotated_events.len(), 1);
    let rotated = AgentKeyRotatedEvent::try_from_val(&env, &rotated_events[0].2).unwrap();
    assert_eq!(rotated.old_primary, old_primary);
    assert_eq!(rotated.new_primary, new_primary);
    assert_eq!(rotated.new_standby, rotated.old_primary);
    assert_eq!(rotated.owner, owner);
    assert_eq!(rotated.timestamp, env.ledger().timestamp());
}

#[test]
fn test_switch_to_standby_without_configuration_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(client.try_switch_to_standby_agent().is_err());
    assert_eq!(client.get_standby_agent(), None);
}

#[test]
fn test_non_owner_cannot_update_standby_agent() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let candidate = Address::generate(&env);
    let attacker = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "update_standby_agent",
            args: (candidate.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client.try_update_standby_agent(&candidate).is_err());
    assert_eq!(client.get_standby_agent(), None);
}

#[test]
fn test_non_owner_cannot_switch_to_standby_agent() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, primary, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let standby = Address::generate(&env);
    client.update_standby_agent(&standby);
    let attacker = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "switch_to_standby_agent",
            args: ().into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client.try_switch_to_standby_agent().is_err());
    assert_eq!(client.get_agent(), primary);
    assert_eq!(client.get_standby_agent(), Some(standby));
}

#[test]
fn test_standby_cannot_equal_primary() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, primary, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(client.try_update_standby_agent(&primary).is_err());
    assert_eq!(client.get_standby_agent(), None);
}

#[test]
fn test_standby_switch_is_immediate_without_creating_agent_timelock() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _old_primary, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let standby = Address::generate(&env);

    client.update_standby_agent(&standby);
    client.switch_to_standby_agent();

    assert_eq!(client.get_agent(), standby);
    assert!(client.get_pending_agent_update().is_none());
}

#[test]
fn test_rotated_out_primary_loses_agent_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, old_primary, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let new_primary = Address::generate(&env);
    let replacement_standby = Address::generate(&env);

    client.update_standby_agent(&new_primary);
    client.switch_to_standby_agent();
    client.update_standby_agent(&replacement_standby);

    env.mock_auths(&[MockAuth {
        address: &old_primary,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "rebalance",
            args: (
                old_primary.clone(),
                soroban_sdk::symbol_short!("none"),
                500_i128,
                0_i128,
            )
                .into_val(&env),
            sub_invokes: &[],
        },
    }]);

    assert!(client
        .try_rebalance(
            &old_primary,
            &soroban_sdk::symbol_short!("none"),
            &500_i128,
            &0_i128
        )
        .is_err());
}
