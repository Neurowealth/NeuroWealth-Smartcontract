#![cfg(test)]

use super::utils::{setup_vault, NeuroWealthVaultClient};
use crate::{ApyPrediction, ApyPredictionSubmittedEvent, DataKey};
use soroban_sdk::testutils::storage::Persistent as _;
use soroban_sdk::testutils::Events as _;
use soroban_sdk::{symbol_short, Env, IntoVal, Symbol, TryFromVal};

fn prediction(protocol: Symbol) -> ApyPrediction {
    ApyPrediction {
        protocol,
        predicted_apy_bps: 800,
        apy_1h_bps: 810,
        apy_6h_bps: 820,
        apy_24h_bps: 830,
        confidence_bps: 9_000,
        submitted_at_ledger: 0,
    }
}

#[test]
fn stores_prediction_with_ttl_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.submit_apy_prediction(&agent, &prediction(symbol_short!("blend")));

    let key = DataKey::ApyPrediction(symbol_short!("blend"));
    let ttl = env.as_contract(&contract_id, || env.storage().persistent().get_ttl(&key));
    assert!(ttl >= 100_000);

    let recorded = client.get_apy_prediction(&symbol_short!("blend")).unwrap();
    assert_eq!(recorded.submitted_at_ledger, env.ledger().sequence());
    let event = env
        .events()
        .all()
        .iter()
        .find_map(|(_, topics, data)| {
            if topics
                .get(0)
                .and_then(|t| Symbol::try_from_val(&env, &t).ok())
                == Some(symbol_short!("apy_pred"))
            {
                ApyPredictionSubmittedEvent::try_from_val(&env, &data).ok()
            } else {
                None
            }
        })
        .expect("prediction event should be emitted");
    assert_eq!(event.protocol, symbol_short!("blend"));
    assert_eq!(event.predicted_apy_bps, 800);
    assert_eq!(event.confidence_bps, 9_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn rejects_unsupported_protocol() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner) = setup_vault(&env);
    NeuroWealthVaultClient::new(&env, &contract_id)
        .submit_apy_prediction(&agent, &prediction(symbol_short!("other")));
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn rejects_apys_above_ten_thousand_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner) = setup_vault(&env);
    let mut invalid = prediction(symbol_short!("blend"));
    invalid.apy_24h_bps = 10_001;
    NeuroWealthVaultClient::new(&env, &contract_id).submit_apy_prediction(&agent, &invalid);
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn rejects_negative_predicted_apy() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner) = setup_vault(&env);
    let mut invalid = prediction(symbol_short!("dex"));
    invalid.predicted_apy_bps = -1;
    NeuroWealthVaultClient::new(&env, &contract_id).submit_apy_prediction(&agent, &invalid);
}

#[test]
#[should_panic(expected = "Error(Contract, #47)")]
fn rejects_confidence_above_ten_thousand_bps() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner) = setup_vault(&env);
    let mut invalid = prediction(symbol_short!("blend"));
    invalid.confidence_bps = 10_001;
    NeuroWealthVaultClient::new(&env, &contract_id).submit_apy_prediction(&agent, &invalid);
}
