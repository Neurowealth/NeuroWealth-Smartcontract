//! Rate-limit coverage for user, global, TTL, preview, and batch paths.
//!
//! The tests deliberately use small owner-configured windows so the fixed-window
//! reset behavior can be exercised by moving the test ledger sequence.

use super::utils::*;
use crate::{
    BatchSizeLimitUpdatedEvent, RateLimitConfig, RateLimitConfigUpdatedEvent,
    RateLimitExceededEvent, RateLimitState, RATE_LIMIT_BATCH_DEPOSIT, RATE_LIMIT_DEPOSIT,
    RATE_LIMIT_PREVIEW, RATE_LIMIT_REBALANCE, RATE_LIMIT_TOUCH_TTL, RATE_LIMIT_WITHDRAW,
    TOPIC_BATCH_SIZE_LIMIT_UPDATED, TOPIC_RATE_LIMIT_CONFIG_UPDATED,
};
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Ledger, Address, Env, TryFromVal, Vec,
};

fn set_limit(
    client: &NeuroWealthVaultClient,
    category: soroban_sdk::Symbol,
    max_calls: u32,
    window_ledgers: u32,
) {
    client.set_rate_limit(&category, &max_calls, &window_ledgers);
}

#[test]
fn owner_can_configure_each_rate_limit_category() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    set_limit(&client, RATE_LIMIT_DEPOSIT, 2, 10);
    set_limit(&client, RATE_LIMIT_WITHDRAW, 3, 11);
    set_limit(&client, RATE_LIMIT_REBALANCE, 4, 12);
    set_limit(&client, RATE_LIMIT_TOUCH_TTL, 1, 1);
    set_limit(&client, RATE_LIMIT_PREVIEW, 5, 2);
    set_limit(&client, RATE_LIMIT_BATCH_DEPOSIT, 6, 13);

    assert_eq!(
        client.get_rate_limit(&RATE_LIMIT_DEPOSIT),
        RateLimitConfig {
            max_calls: 2,
            window_ledgers: 10,
        }
    );
    assert_eq!(
        client.get_rate_limit_config(&RATE_LIMIT_PREVIEW),
        RateLimitConfig {
            max_calls: 5,
            window_ledgers: 2,
        }
    );

    let config_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_RATE_LIMIT_CONFIG_UPDATED);
    assert_eq!(config_events.len(), 6);
    let (_, _, data) = &config_events[0];
    let event = RateLimitConfigUpdatedEvent::try_from_val(&env, data)
        .expect("rate-limit config event should decode");
    assert_eq!(event.new_max_calls, 2);
    assert_eq!(event.new_window_ledgers, 10);
}

#[test]
fn deposit_limit_is_per_user_and_resets_after_window() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);

    set_limit(&client, RATE_LIMIT_DEPOSIT, 1, 3);
    mint_and_deposit(&env, &client, &usdc_token, &user_a, 1_000_000);

    let token = TestTokenClient::new(&env, &usdc_token);
    token.mint(&user_a, &1_000_000);
    assert!(
        client.try_deposit(&user_a, &1_000_000).is_err(),
        "the second deposit by one user must be rate limited"
    );
    let hit_events = find_events_by_topic(env.events().all(), &env, crate::TOPIC_RATE_LIMIT_HIT);
    assert_eq!(
        hit_events.len(),
        1,
        "bucket exhaustion must be observable through the rate-limit event"
    );
    let (_, _, data) = &hit_events[0];
    let hit = RateLimitExceededEvent::try_from_val(&env, data)
        .expect("rate-limit hit event should decode");
    assert_eq!(hit.category, RATE_LIMIT_DEPOSIT);
    assert_eq!(hit.user, Some(user_a.clone()));
    assert_eq!(hit.max_calls, 1);
    assert_eq!(hit.calls, 1);

    // A different user has an independent bucket in the same ledger window.
    mint_and_deposit(&env, &client, &usdc_token, &user_b, 1_000_000);
    let state_a = client.get_user_rate_limit_state(&user_a, &RATE_LIMIT_DEPOSIT);
    let state_b = client.get_user_rate_limit_state(&user_b, &RATE_LIMIT_DEPOSIT);
    assert_eq!(state_a.calls, 1);
    assert_eq!(state_b.calls, 1);

    let first_window = state_a.window_start;
    env.ledger().set_sequence_number(first_window + 3);
    token.mint(&user_a, &1_000_000);
    client.deposit(&user_a, &1_000_000);
    let reset_state = client.get_user_rate_limit_state(&user_a, &RATE_LIMIT_DEPOSIT);
    assert_eq!(reset_state.calls, 1);
    assert_eq!(reset_state.window_start, first_window + 3);
}

#[test]
fn withdrawal_and_withdraw_all_share_the_same_user_bucket() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    set_limit(&client, RATE_LIMIT_WITHDRAW, 1, 2);
    mint_and_deposit(&env, &client, &usdc_token, &user, 3_000_000);
    client.withdraw(&user, &1_000_000);
    assert!(
        client.try_withdraw_all(&user).is_err(),
        "withdraw_all must not bypass the withdraw bucket"
    );
    assert_eq!(
        client
            .get_user_rate_limit_state(&user, &RATE_LIMIT_WITHDRAW)
            .calls,
        1
    );
}

#[test]
fn rebalance_frequency_complements_the_existing_cooldown() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    set_limit(&client, RATE_LIMIT_REBALANCE, 1, 4);
    client.rebalance(&symbol_short!("none"), &0, &0);
    assert!(
        client
            .try_rebalance(&symbol_short!("none"), &0, &0)
            .is_err(),
        "the global rebalance bucket must reject a second call"
    );

    let state = client.get_global_rate_limit_state(&RATE_LIMIT_REBALANCE);
    assert_eq!(state.calls, 1);
    env.ledger().set_sequence_number(state.window_start + 4);
    client.rebalance(&symbol_short!("none"), &0, &0);
    assert_eq!(
        client
            .get_global_rate_limit_state(&RATE_LIMIT_REBALANCE)
            .calls,
        1
    );
}

#[test]
fn touch_ttl_is_limited_per_user_per_ledger() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    set_limit(&client, RATE_LIMIT_TOUCH_TTL, 1, 1);
    mint_and_deposit(&env, &client, &usdc_token, &user, 1_000_000);
    assert!(client.touch_user_ttl(&user));
    assert!(
        client.try_touch_user_ttl(&user).is_err(),
        "a second TTL touch in the same ledger must be rejected"
    );

    let state = client.get_user_rate_limit_state(&user, &RATE_LIMIT_TOUCH_TTL);
    env.ledger().set_sequence_number(state.window_start + 1);
    assert!(client.touch_user_ttl(&user));
}

#[test]
fn all_preview_and_conversion_entrypoints_share_one_global_bucket() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    set_limit(&client, RATE_LIMIT_PREVIEW, 2, 2);
    let _ = client.preview_deposit_to_shares(&1_000_000);
    let _ = client.convert_to_assets(&1_000_000);
    assert!(
        client.try_preview_withdraw(&1_000_000).is_err(),
        "preview aliases must not provide separate bypass buckets"
    );

    let state = client.get_global_rate_limit_state(&RATE_LIMIT_PREVIEW);
    env.ledger().set_sequence_number(state.window_start + 2);
    let _ = client.preview_shares_to_assets(&1_000_000);
    assert_eq!(
        client
            .get_global_rate_limit_state(&RATE_LIMIT_PREVIEW)
            .calls,
        1
    );
}

#[test]
fn batch_size_guard_is_independent_from_batch_frequency_limit() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let token = TestTokenClient::new(&env, &usdc_token);

    set_limit(&client, RATE_LIMIT_DEPOSIT, 10, 10);
    set_limit(&client, RATE_LIMIT_BATCH_DEPOSIT, 10, 10);
    client.set_max_batch_size(&2);

    let mut entries: Vec<(Address, i128)> = Vec::new(&env);
    entries.push_back((usdc_token.clone(), 1_000_000));
    entries.push_back((usdc_token.clone(), 1_000_000));
    entries.push_back((usdc_token.clone(), 1_000_000));
    token.mint(&user, &3_000_000);
    assert!(
        client.try_batch_deposit(&user, &entries).is_err(),
        "a batch above the configured entry limit must be rejected"
    );

    let size_events =
        find_events_by_topic(env.events().all(), &env, TOPIC_BATCH_SIZE_LIMIT_UPDATED);
    assert_eq!(size_events.len(), 1);
    let (_, _, data) = &size_events[0];
    let event = BatchSizeLimitUpdatedEvent::try_from_val(&env, data)
        .expect("batch-size config event should decode");
    assert_eq!(event.new_max_entries, 2);

    let mut valid_entries: Vec<(Address, i128)> = Vec::new(&env);
    valid_entries.push_back((usdc_token.clone(), 1_000_000));
    valid_entries.push_back((usdc_token, 1_000_000));
    client.batch_deposit(&user, &valid_entries);
    assert_eq!(client.get_shares(&user), 2_000_000);
}

#[test]
fn invalid_rate_limit_configuration_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert!(
        client
            .try_set_rate_limit(&RATE_LIMIT_DEPOSIT, &1, &0)
            .is_err(),
        "an enabled rate limit must have a non-zero window"
    );
    assert!(
        client
            .try_set_rate_limit(&symbol_short!("unknown"), &1, &1)
            .is_err(),
        "unknown categories must not create arbitrary storage keys"
    );

    let empty = RateLimitState {
        window_start: 0,
        calls: 0,
    };
    assert_eq!(
        client.get_global_rate_limit_state(&RATE_LIMIT_REBALANCE),
        empty
    );
}
