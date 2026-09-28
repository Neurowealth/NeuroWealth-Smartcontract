//! Storage TTL and rent-extension failure paths (Issue #844).
//!
//! `touch_user_ttl` is the only user-facing TTL maintenance entrypoint, and
//! rent (TTL) extension is the one operation whose funding can run out. These
//! tests cover the three funding situations - fully funded, partially funded,
//! and unfunded - and prove that a failed extension never corrupts balances or
//! shares, plus the recovery path for entries that have already lapsed.
//!
//! # What "funded" means for a TTL bump
//!
//! A persistent entry stays live until `live_until`, which the network caps at
//! `max_entry_ttl` ledgers from the current ledger. `touch_user_ttl` asks to
//! extend the `Shares(user)` entry to
//! `USER_SHARES_TTL_EXTEND_TO` (= 100) ledgers when its remaining TTL is below
//! `USER_SHARES_TTL_THRESHOLD` (= 100):
//!
//! - **funded** - the network window is wide enough, the entry is bumped to the
//!   requested target;
//! - **partially funded** - the remaining network window is narrower than the
//!   requested target, so the bump is clamped to the network cap. The entry
//!   stays alive but the operator must fund the contract again;
//! - **unfunded** - the entry has already lapsed. There is nothing left to
//!   extend, `touch_user_ttl` reports `false`, and recovery requires an
//!   operator- or user-driven write (a new deposit) rather than a bump.
//!
//! # Which operations extend TTLs
//!
//! - `deposit`, `withdraw`, `withdraw_all` refresh `Shares(user)` as part of
//!   the normal write, which is why routine activity keeps entries alive.
//! - `touch_user_ttl` is the only explicit bump and is the only path that can
//!   fail without failing the caller's transaction.
//! - Read-only getters (`get_shares`, `get_balance`) never extend TTLs.

use super::utils::*;
use crate::DataKey;
use soroban_sdk::testutils::storage::Persistent as _;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env,
};

/// Current TTL (ledgers remaining, excluding the current ledger) of a user's
/// `Shares` entry.
fn shares_ttl(env: &Env, contract_id: &Address, user: &Address) -> u32 {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::Shares(user.clone()))
    })
}

/// Removes the persistent entry the way a lapsed TTL does, and reports whether
/// the vault still accounts for the user's claim.
fn expire_shares_entry(env: &Env, contract_id: &Address, user: &Address) {
    env.as_contract(contract_id, || {
        env.storage()
            .persistent()
            .remove(&DataKey::Shares(user.clone()))
    });
}

// ============================================================================
// Funded extension
// ============================================================================

/// The happy path: a funded extension raises the TTL of the intended entry and
/// leaves the vault's accounting untouched.
#[test]
fn test_funded_extension_updates_entry_and_preserves_balances() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let shares_before = client.get_shares(&user);
    let balance_before = client.get_balance(&user);
    let total_assets_before = client.get_total_assets();
    let total_shares_before = client.get_total_shares();

    // Force the entry under the threshold so the extension path is exercised.
    env.as_contract(&contract_id, || {
        env.storage().persistent().extend_ttl(
            &DataKey::Shares(user.clone()),
            0,
            1,
        )
    });
    let ttl_before = shares_ttl(&env, &contract_id, &user);

    assert!(
        client.touch_user_ttl(&user),
        "funded extension must report success for an existing entry"
    );

    let ttl_after = shares_ttl(&env, &contract_id, &user);
    assert!(
        ttl_after > ttl_before,
        "funded extension must raise the TTL (was {ttl_before}, now {ttl_after})"
    );
    assert!(
        ttl_after >= 100,
        "funded extension must reach USER_SHARES_TTL_EXTEND_TO (100), got {ttl_after}"
    );

    // A TTL bump is pure bookkeeping: no balance or share accounting moves.
    assert_eq!(client.get_shares(&user), shares_before);
    assert_eq!(client.get_balance(&user), balance_before);
    assert_eq!(client.get_total_assets(), total_assets_before);
    assert_eq!(client.get_total_shares(), total_shares_before);
}

/// Only the `Shares` entry is touched by the bump.
#[test]
fn test_funded_extension_does_not_disturb_other_entries() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let bystander = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);
    mint_and_deposit(&env, &client, &usdc_token, &bystander, 1_000_000);

    let bystander_ttl_before = shares_ttl(&env, &contract_id, &bystander);
    client.touch_user_ttl(&user);
    let bystander_ttl_after = shares_ttl(&env, &contract_id, &bystander);

    assert_eq!(
        bystander_ttl_before, bystander_ttl_after,
        "touch_user_ttl must only bump the entry of the address it was called for"
    );
    assert!(client.get_shares(&bystander) > 0);
}

// ============================================================================
// Partially funded extension
// ============================================================================

/// A partially funded extension is clamped to the network's remaining entry
/// window instead of failing: the entry survives, and the shortfall is the
/// operator's signal to fund the contract.
#[test]
fn test_partially_funded_extension_is_clamped_and_keeps_entry_alive() {
    let env = Env::default();
    env.mock_all_auths();

    // Network allows far fewer ledgers than the contract asks for, so the bump
    // can only partially succeed.
    env.ledger().set_max_entry_ttl(30);

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let shares_before = client.get_shares(&user);
    let total_assets_before = client.get_total_assets();
    let ttl_before = shares_ttl(&env, &contract_id, &user);

    assert!(
        client.touch_user_ttl(&user),
        "a clamped extension still reports success for a live entry"
    );

    let ttl_after = shares_ttl(&env, &contract_id, &user);
    assert!(
        ttl_after >= ttl_before,
        "a partially funded extension must never shorten the window (was {ttl_before}, now {ttl_after})"
    );
    assert!(
        ttl_after < 100,
        "with a 30-ledger network window the bump cannot reach the 100-ledger target, got {ttl_after}"
    );

    // Nothing is lost: shares and vault accounting are unchanged.
    assert_eq!(client.get_shares(&user), shares_before);
    assert_eq!(client.get_total_assets(), total_assets_before);
    assert!(
        client.get_balance(&user) > 0,
        "a partially funded extension must not strand the user's assets"
    );
}

/// Repeated partial extension attempts are stable: they never corrupt shares or
/// drift the user's balance.
#[test]
fn test_repeated_partial_extensions_are_stable() {
    let env = Env::default();
    env.mock_all_auths();

    env.ledger().set_max_entry_ttl(30);

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);

    let shares_before = client.get_shares(&user);
    let balance_before = client.get_balance(&user);
    let total_shares_before = client.get_total_shares();

    for _ in 0..5 {
        let _ = client.touch_user_ttl(&user);
    }

    assert_eq!(client.get_shares(&user), shares_before);
    assert_eq!(client.get_balance(&user), balance_before);
    assert_eq!(client.get_total_shares(), total_shares_before);
}

// ============================================================================
// Unfunded extension
// ============================================================================

/// Once the entry has lapsed there is nothing to extend. The call reports
/// `false` instead of panicking, and the vault's own accounting - which lives
/// in instance storage and does not expire - is untouched.
#[test]
fn test_unfunded_extension_of_lapsed_entry_fails_without_corrupting_state() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);
    let total_assets_before = client.get_total_assets();
    let total_shares_before = client.get_total_shares();
    let usdc_balance_before = client.get_balance(&user);

    // The rent for this entry was never topped up and it expired.
    expire_shares_entry(&env, &contract_id, &user);

    assert_eq!(client.get_shares(&user), 0, "lapsed entry reads as zero");
    assert!(
        !client.touch_user_ttl(&user),
        "an unfunded, lapsed entry must report false rather than panic"
    );

    // Failed extension paths must not disturb vault accounting.
    assert_eq!(client.get_total_assets(), total_assets_before);
    assert_eq!(client.get_total_shares(), total_shares_before);
    assert_eq!(client.get_balance(&user), 0);
    assert!(usdc_balance_before > 0);
}

/// Several failed extension attempts in a row are equally harmless: the vault
/// keeps reporting the same totals and the user can still transact.
#[test]
fn test_repeated_unfunded_extension_attempts_do_not_corrupt_balances() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);
    let total_assets_before = client.get_total_assets();
    let total_shares_before = client.get_total_shares();

    expire_shares_entry(&env, &contract_id, &user);

    for _ in 0..5 {
        assert!(!client.touch_user_ttl(&user));
        assert_eq!(client.get_shares(&user), 0);
    }

    assert_eq!(client.get_total_assets(), total_assets_before);
    assert_eq!(client.get_total_shares(), total_shares_before);
}

/// Recovery path for a lapsed entry: a normal deposit recreates the entry with
/// a healthy TTL. This is the documented route - an unfunded bump is not, and
/// never was, sufficient to restore an expired entry.
#[test]
fn test_recovery_after_lapse_is_a_normal_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000);
    expire_shares_entry(&env, &contract_id, &user);
    assert!(!client.touch_user_ttl(&user), "bump cannot restore a lapsed entry");

    // The user is not locked out: a new deposit re-creates the entry.
    let amount = 1_000_000_i128;
    token_client.mint(&user, &amount);
    client.deposit(&user, &amount);

    assert!(
        client.get_shares(&user) > 0,
        "a new deposit must restore the user's share entry"
    );
    let ttl = shares_ttl(&env, &contract_id, &user);
    assert!(
        ttl >= 100,
        "the restored entry must carry a healthy TTL, got {ttl}"
    );
    assert!(client.touch_user_ttl(&user), "and must be maintainable again");
}

// ============================================================================
// Protocol (approval) TTL: exact-threshold expiry
// ============================================================================

/// Protocol approvals expire at exactly `sequence + ApprovalTtl`. The
/// allowance is live on that final ledger and gone one ledger later, which is
/// the threshold the vault's TTL constants are measured against.
#[test]
fn test_protocol_approval_is_live_until_and_including_the_expiry_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    let ttl = 20_000_u32;
    client.set_approval_ttl(&ttl);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000);

    let sequence = env.ledger().sequence();
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);
    let expiry = token_client.allowance_expiration(&contract_id, &blend_pool);
    assert_eq!(expiry, sequence + ttl);

    // On the expiry ledger itself the allowance is still valid.
    env.ledger().set_sequence_number(expiry);
    assert!(
        token_client.allowance(&contract_id, &blend_pool) > 0,
        "the approval must stay usable through the expiry ledger"
    );

    // One ledger later it is gone, and the vault has to re-approve - which is
    // exactly what ProtocolApprovalScheduledEvent tells operators to schedule.
    env.ledger().set_sequence_number(expiry + 1);
    assert_eq!(
        token_client.allowance(&contract_id, &blend_pool),
        0,
        "the approval must be unusable one ledger after expiry"
    );
}

/// A TTL extension failure must not leave a protocol approval half-updated:
/// the allowance and its expiration move together or not at all.
#[test]
fn test_protocol_approval_state_is_consistent_after_failed_maintenance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);
    client.set_approval_ttl(&30_000_u32);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000);
    client.rebalance(&symbol_short!("blend"), &700_i128, &0_i128);

    let expiration = token_client.allowance_expiration(&contract_id, &blend_pool);
    let allowance = token_client.allowance(&contract_id, &blend_pool);
    assert!(allowance > 0);
    assert_eq!(expiration, env.ledger().sequence() + 30_000);

    // User-space maintenance that cannot be funded must not disturb the
    // protocol approval that was already granted.
    expire_shares_entry(&env, &contract_id, &user);
    assert!(!client.touch_user_ttl(&user));

    assert_eq!(
        token_client.allowance(&contract_id, &blend_pool),
        allowance,
        "a failed user-side TTL bump must not alter the protocol allowance"
    );
    assert_eq!(
        token_client.allowance_expiration(&contract_id, &blend_pool),
        expiration,
        "a failed user-side TTL bump must not extend the protocol approval"
    );
}
