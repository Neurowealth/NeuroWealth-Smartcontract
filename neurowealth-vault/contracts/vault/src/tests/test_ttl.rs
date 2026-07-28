//! Tests for persistent-storage TTL behavior on user share entries.

use super::utils::*;
use crate::DataKey;
use soroban_sdk::testutils::storage::Persistent as _;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_get_shares_does_not_extend_ttl() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    let _ = client.get_shares(&user);

    let ttl_after_read = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert_eq!(
        ttl_before, ttl_after_read,
        "get_shares must not extend Shares TTL"
    );
}

#[test]
fn test_get_balance_does_not_extend_ttl() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    let _ = client.get_balance(&user);

    let ttl_after_read = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert_eq!(
        ttl_before, ttl_after_read,
        "get_balance must not extend Shares TTL"
    );
}

#[test]
fn test_touch_user_ttl_extends_shares_entry() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(client.touch_user_ttl(&user));

    let ttl_after_touch = env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(
        ttl_after_touch >= ttl_before,
        "touch_user_ttl should extend or preserve Shares TTL"
    );
}

#[test]
fn test_touch_user_ttl_no_entry_returns_false() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    assert!(!client.touch_user_ttl(&user));
}

// ============================================================================
// Edge-case TTL coverage — issue #253
//
// TTL constants (lib.rs):
//   USER_SHARES_TTL_THRESHOLD = 100  — extend_ttl acts when TTL < this
//   USER_SHARES_TTL_EXTEND_TO  = 100  — target ledgers after extension
// ============================================================================

/// Expiration path: once a Shares entry is removed (simulating ledger expiry),
/// get_shares returns 0 and touch_user_ttl returns false — indistinguishable
/// from a user who never deposited.
#[test]
fn test_expired_entry_is_treated_as_nonexistent() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);
    assert!(client.get_shares(&user) > 0, "shares must exist before expiry");

    // Simulate TTL expiry by removing the persistent entry directly
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::Shares(user.clone()))
    });

    assert_eq!(client.get_shares(&user), 0, "expired entry: get_shares must return 0");
    assert!(!client.touch_user_ttl(&user), "expired entry: touch_user_ttl must return false");
}

/// Restoration path: a Shares entry with TTL below threshold (100) is extended
/// to USER_SHARES_TTL_EXTEND_TO (100) by touch_user_ttl. This is the primary
/// recovery mechanism for entries approaching expiry.
#[test]
fn test_touch_restores_ttl_when_below_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    // Force TTL to 1 ledger (insufficient headroom) using threshold=0 so it always acts
    env.as_contract(&contract_id, || {
        env.storage().persistent().extend_ttl(
            &DataKey::Shares(user.clone()),
            0, // always act
            1, // 1 ledger remaining
        )
    });

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(client.touch_user_ttl(&user), "touch must return true when entry exists");

    let ttl_after = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(
        ttl_after > ttl_before,
        "TTL must increase from {ttl_before} after touch (restored from insufficient headroom)"
    );
    assert!(ttl_after >= 100, "TTL must reach at least USER_SHARES_TTL_EXTEND_TO (100)");
}

/// Boundary — exactly at threshold: TTL == 100 is NOT below threshold
/// (condition is TTL < threshold), so touch_user_ttl must leave TTL unchanged.
#[test]
fn test_touch_does_not_extend_when_ttl_equals_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    // Set TTL to exactly USER_SHARES_TTL_THRESHOLD (100)
    env.as_contract(&contract_id, || {
        env.storage().persistent().extend_ttl(
            &DataKey::Shares(user.clone()),
            0,   // always act
            100, // extend_to == threshold
        )
    });

    let ttl_at_threshold = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(client.touch_user_ttl(&user));

    let ttl_after = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&DataKey::Shares(user.clone()))
    });

    // 100 < 100 is false → no extension
    assert_eq!(
        ttl_at_threshold, ttl_after,
        "TTL must not change when it is already at the threshold (100 is not < 100)"
    );
}

/// Boundary — one below threshold: TTL == 99 IS below threshold, so
/// touch_user_ttl must extend it to USER_SHARES_TTL_EXTEND_TO (100).
#[test]
fn test_touch_extends_when_ttl_is_one_below_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    // Set TTL to 99 (threshold - 1)
    env.as_contract(&contract_id, || {
        env.storage().persistent().extend_ttl(
            &DataKey::Shares(user.clone()),
            0,  // always act
            99,
        )
    });

    let ttl_before = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(client.touch_user_ttl(&user));

    let ttl_after = env.as_contract(&contract_id, || {
        env.storage().persistent().get_ttl(&DataKey::Shares(user.clone()))
    });

    assert!(ttl_after > ttl_before, "TTL of 99 (below threshold) must be extended by touch");
    assert!(ttl_after >= 100, "TTL must reach at least USER_SHARES_TTL_EXTEND_TO (100)");
}

/// Multiple successive touch_user_ttl calls keep the entry alive without error.
/// Every call returns true while the entry exists.
#[test]
fn test_multiple_touch_calls_keep_entry_alive() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    for i in 0..5 {
        assert!(client.touch_user_ttl(&user), "touch call {i} must return true");
        let ttl = env.as_contract(&contract_id, || {
            env.storage().persistent().get_ttl(&DataKey::Shares(user.clone()))
        });
        assert!(ttl > 0, "TTL must remain positive after touch {i}");
    }
}

/// After a full withdrawal, touch behaviour reflects whether the contract
/// retains a zero-share entry. Either outcome is valid — what matters is no panic.
#[test]
fn test_touch_after_full_withdrawal_does_not_panic() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);
    client.withdraw_all(&user, &None);

    assert_eq!(client.get_shares(&user), 0, "shares must be 0 after withdraw_all");
    // Must not panic regardless of whether the key is retained or removed
    let _touched = client.touch_user_ttl(&user);
}
