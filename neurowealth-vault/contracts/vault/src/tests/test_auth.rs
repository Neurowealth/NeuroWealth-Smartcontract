//! Targeted authorization tests that reduce reliance on `mock_all_auths`.
//!
//! ## Why this file exists
//!
//! `env.mock_all_auths()` makes **every** `require_auth()` succeed. That is
//! convenient, but it also *hides* missing authorization checks: if a
//! `require_auth()` were accidentally removed from an entrypoint, a test that
//! mocks all auths would still pass, because the (now absent) check was never
//! the thing keeping the call honest.
//!
//! The tests below exercise authorization explicitly using two stricter tools:
//!
//! - [`Env::mock_auths`] with a precise [`MockAuth`] / [`MockAuthInvoke`] tree.
//!   Only the listed address is authorized for the listed invocation (and its
//!   declared sub-invocations). Anything else fails. This proves the entrypoint
//!   requires *exactly* the expected signer.
//! - `mock_auths(&[])` (an empty authorization set) followed by `try_*`. With no
//!   authorization available, a present `require_auth()` makes the call error.
//!   If the `require_auth()` were missing, the call would instead succeed and
//!   the assertion would fail — so these tests actively guard against a dropped
//!   auth check.
//!
//! ## When is `mock_all_auths` acceptable?
//!
//! `mock_all_auths` remains the right tool for tests whose purpose is **not**
//! authorization — e.g. share-math, rounding, event schema, limit, and
//! happy-path lifecycle tests. In those cases auth is incidental setup noise and
//! mocking it wholesale keeps the test focused on the behavior under scrutiny.
//!
//! Use the stricter `mock_auths` / negative-auth style here whenever the test's
//! subject *is* access control: confirming an entrypoint requires the owner,
//! the agent, or the acting user, and rejects everyone else.

use super::utils::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};

/// Sets up an initialized vault while `mock_all_auths` is active.
///
/// Initialization legitimately requires the deployer's signature; mocking it is
/// incidental setup, not the behavior under test. Callers switch to scoped or
/// empty auths *after* this returns to exercise the actual access control.
fn setup(env: &Env) -> (Address, Address, Address, Address) {
    env.mock_all_auths();
    setup_vault_with_token(env)
}

// ============================================================================
// OWNER ROLE
// ============================================================================

/// Positive: with auth scoped to *only* the owner for *only* `pause`, the call
/// succeeds. Proves `pause` accepts the owner's authorization.
#[test]
fn test_owner_pause_with_scoped_auth() {
    let env = Env::default();
    let (contract_id, _agent, owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "pause",
            args: (owner.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.pause(&owner);
    assert!(client.is_paused());
}

/// Negative: with no authorization available, `pause` must error because
/// `owner.require_auth()` cannot be satisfied. Guards against a removed check.
#[test]
fn test_pause_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_pause(&owner);
    assert!(
        result.is_err(),
        "pause must fail without the owner's authorization"
    );
    assert!(!client.is_paused(), "vault must remain unpaused");
}

/// Negative: an owner-only config setter must error without owner auth. Covers
/// the `require_is_owner()` path (which performs the `require_auth` internally).
#[test]
fn test_set_tvl_cap_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_set_tvl_cap(&50_000_000_000_i128);
    assert!(
        result.is_err(),
        "set_tvl_cap must fail without the owner's authorization"
    );
}

/// Negative: a non-owner that *does* hold a valid signature for `pause` is still
/// rejected by the identity check. Auth is scoped to the attacker only.
#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_non_owner_with_own_auth_cannot_pause() {
    let env = Env::default();
    let (contract_id, _agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let attacker = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "pause",
            args: (attacker.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    // attacker.require_auth() passes (scoped), but attacker != stored owner.
    client.pause(&attacker);
}

// ============================================================================
// AGENT ROLE
// ============================================================================

/// Positive: with auth scoped to *only* the agent for *only* `rebalance`, the
/// call succeeds. The "none" protocol needs no external pool.
#[test]
fn test_agent_rebalance_with_scoped_auth() {
    let env = Env::default();
    let (contract_id, agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[MockAuth {
        address: &agent,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "rebalance",
            args: (symbol_short!("none"), 500_i128, 0_i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.rebalance(&symbol_short!("none"), &500_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
}

/// Negative: `rebalance` must error without the agent's authorization.
#[test]
fn test_rebalance_requires_agent_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_rebalance(&symbol_short!("none"), &500_i128, &0_i128);
    assert!(
        result.is_err(),
        "rebalance must fail without the agent's authorization"
    );
}

/// Negative: `update_total_assets` must error without the agent's authorization.
/// The real agent is passed so the identity check passes and the failure is due
/// to the missing `agent.require_auth()`.
#[test]
fn test_update_total_assets_requires_agent_auth() {
    let env = Env::default();
    let (contract_id, agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_update_total_assets(&agent, &0_i128, &false, &0_u32);
    assert!(
        result.is_err(),
        "update_total_assets must fail without the agent's authorization"
    );
}

// ============================================================================
// USER ROLE
// ============================================================================

/// Positive: a user deposit succeeds when auth is scoped to the user for
/// `deposit` *and* its `transfer` sub-invocation on the token contract. This
/// proves both the entrypoint and the funds-pull are authorized by the user.
#[test]
fn test_user_deposit_with_scoped_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;
    token_client.mint(&user, &amount); // mint needs no auth

    env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "deposit",
            args: (user.clone(), amount).into_val(&env),
            sub_invokes: &[MockAuthInvoke {
                contract: &usdc_token,
                fn_name: "transfer",
                args: (user.clone(), contract_id.clone(), amount).into_val(&env),
                sub_invokes: &[],
            }],
        },
    }]);

    client.deposit(&user, &amount);
    // Bootstrap deposit mints shares 1:1 with assets.
    assert_eq!(client.get_shares(&user), amount);
}

/// Negative: `deposit` must error without the depositing user's authorization,
/// even when funds are available.
#[test]
fn test_deposit_requires_user_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;
    token_client.mint(&user, &amount);

    env.mock_auths(&[]);

    let result = client.try_deposit(&user, &amount);
    assert!(
        result.is_err(),
        "deposit must fail without the user's authorization"
    );
}

/// Negative: another address holding its own valid signature cannot deposit on
/// the victim's behalf — `deposit` requires the *acting user's* auth.
#[test]
fn test_deposit_rejects_wrong_signer() {
    let env = Env::default();
    let (contract_id, _agent, _owner, usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let attacker = Address::generate(&env);
    let amount = 5_000_000_i128;
    token_client.mint(&user, &amount);

    // Authorize the attacker for a deposit call, but the call names `user`.
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "deposit",
            args: (user.clone(), amount).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_deposit(&user, &amount);
    assert!(
        result.is_err(),
        "deposit must require the named user's authorization, not another signer's"
    );
}

/// Positive: a user withdrawal succeeds with auth scoped to the user for
/// `withdraw`. The vault's outbound token transfer is authorized automatically
/// as the current contract, so no token sub-invoke is needed from the user.
#[test]
fn test_user_withdraw_with_scoped_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;
    // Fund and deposit under full mock; the withdraw is what we scope.
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    env.mock_auths(&[MockAuth {
        address: &user,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "withdraw",
            args: (user.clone(), amount).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.withdraw(&user, &amount, &None);
    assert_eq!(client.get_shares(&user), 0);
}

/// Negative: `withdraw` must error without the user's authorization.
#[test]
fn test_withdraw_requires_user_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    env.mock_auths(&[]);

    let result = client.try_withdraw(&user, &amount, &None);
    assert!(
        result.is_err(),
        "withdraw must fail without the user's authorization"
    );
}

// ============================================================================
// LOSS REPORTING — OWNER CO-SIGN REQUIREMENT
// ============================================================================

/// Negative: with only the agent's auth present, a decrease must fail because
/// `require_is_owner` cannot be satisfied. Guards against a removed auth check.
#[test]
fn test_decrease_requires_owner_cosign() {
    let env = Env::default();
    let (contract_id, agent, _owner, usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    let new_total = deposit_amount - 500_000_i128; // 5% loss, within 10% cap

    // Only the agent's auth is available — the owner has NOT signed.
    env.mock_auths(&[MockAuth {
        address: &agent,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "update_total_assets",
            args: (agent.clone(), new_total, true, 1000u32).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_update_total_assets(&agent, &new_total, &true, &1000u32);
    assert!(
        result.is_err(),
        "decrease must fail when owner has not co-signed"
    );
    // Total assets must be unchanged.
    assert_eq!(client.get_total_assets(), deposit_amount);
}

// ============================================================================
// OWNER ROLE — REMAINING ENTRYPOINTS (negative empty-auth guards)
// ============================================================================
//
// Each test below drops all authorization with `mock_auths(&[])` and asserts the
// call errors. A present `require_auth()` (directly or via `require_is_owner`)
// makes the call fail; if that check were ever dropped, the call would succeed
// and the assertion would catch it. These cover the owner-only entrypoints that
// `test_access_control.rs` otherwise exercises only under `mock_all_auths`.

/// Negative: `unpause` must error without the owner's authorization.
#[test]
fn test_unpause_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_unpause(&owner);
    assert!(
        result.is_err(),
        "unpause must fail without the owner's authorization"
    );
}

/// Negative: `emergency_pause` must error without the owner's authorization.
#[test]
fn test_emergency_pause_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_emergency_pause(&owner);
    assert!(
        result.is_err(),
        "emergency_pause must fail without the owner's authorization"
    );
}

/// Negative: `set_user_deposit_cap` must error without the owner's authorization.
#[test]
fn test_set_user_deposit_cap_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_set_user_deposit_cap(&10_000_000_000_i128);
    assert!(
        result.is_err(),
        "set_user_deposit_cap must fail without the owner's authorization"
    );
}

/// Negative: `set_deposit_limits` must error without the owner's authorization.
#[test]
fn test_set_deposit_limits_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_set_deposit_limits(&1_000_000_i128, &100_000_000_i128);
    assert!(
        result.is_err(),
        "set_deposit_limits must fail without the owner's authorization"
    );
}

/// Negative: `update_agent` must error without the owner's authorization.
#[test]
fn test_update_agent_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_update_agent(&new_agent);
    assert!(
        result.is_err(),
        "update_agent must fail without the owner's authorization"
    );
}

/// Negative: `set_blend_pool` must error without the owner's authorization, even
/// when the real owner address is named (so the failure is the missing
/// `owner.require_auth()`, not the identity check).
#[test]
fn test_set_blend_pool_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let pool = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_set_blend_pool(&owner, &pool);
    assert!(
        result.is_err(),
        "set_blend_pool must fail without the owner's authorization"
    );
}

// ============================================================================
// OWNERSHIP TRANSFER — two-step handshake auth
// ============================================================================

/// Positive: `transfer_ownership` succeeds with auth scoped to the current owner
/// for *only* that call. `require_is_owner` performs `owner.require_auth()`.
#[test]
fn test_transfer_ownership_with_scoped_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &owner,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "transfer_ownership",
            args: (new_owner.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.transfer_ownership(&new_owner);
    // Ownership does not change until accepted; owner-only calls still work.
    assert_eq!(client.get_owner(), owner);
}

/// Negative: `transfer_ownership` must error without the current owner's auth.
#[test]
fn test_transfer_ownership_requires_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    env.mock_auths(&[]);

    let result = client.try_transfer_ownership(&new_owner);
    assert!(
        result.is_err(),
        "transfer_ownership must fail without the current owner's authorization"
    );
}

/// Negative: `accept_ownership` must error without the pending owner's auth, even
/// after a valid transfer was initiated. Guards `new_owner.require_auth()`.
#[test]
fn test_accept_ownership_requires_pending_owner_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, _usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);
    // Initiate the transfer while full mock is still active (incidental setup).
    client.transfer_ownership(&new_owner);

    // The pending owner must still sign to accept — drop all auth.
    env.mock_auths(&[]);

    let result = client.try_accept_ownership(&new_owner);
    assert!(
        result.is_err(),
        "accept_ownership must fail without the pending owner's authorization"
    );
}

// ============================================================================
// USER ROLE — REMAINING ENTRYPOINTS
// ============================================================================

/// Negative: `withdraw_all` must error without the user's authorization, even
/// with a funded position.
#[test]
fn test_withdraw_all_requires_user_auth() {
    let env = Env::default();
    let (contract_id, _agent, _owner, usdc_token) = setup(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    env.mock_auths(&[]);

    let result = client.try_withdraw_all(&user, &None);
    assert!(
        result.is_err(),
        "withdraw_all must fail without the user's authorization"
    );
}
