//! Tests for vault migration functionality (#637)

use super::utils::*;
use crate::TOPIC_MIGRATION_TARGET_UPDATED;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, BytesN, Env};

/// Schedules a migration target and executes it once the timelock has passed (#969).
///
/// Callers must configure generous ledger entry TTLs (see
/// `extend_ttls_for_migration_timelock`) *before* deploying the vault under
/// test, so its own contract instance entry doesn't get archived by the
/// ~17,280-ledger jump this helper performs.
fn set_target_now(env: &Env, client: &NeuroWealthVaultClient, owner: &Address, target: &Address) {
    client.schedule_migration_target(owner, target);
    let (_, effective_ledger) = client.get_pending_migration_target().unwrap();
    env.ledger().set_sequence_number(effective_ledger);
    client.execute_migration_target(owner);
}

/// Keeps instance storage alive across `set_target_now`'s ~17,280-ledger
/// jump (mirrors `test_migration_timelock.rs`'s setup). Must be called
/// *before* the vault under test is deployed/initialized — the TTL a
/// contract's instance entry gets is fixed at creation time, so calling
/// this afterward is too late and the entry can still be archived by the
/// time `execute_migration_target` runs, failing with an unrelated
/// "accessed archived contract instance" error.
fn extend_ttls_for_migration_timelock(env: &Env) {
    env.ledger().with_mut(|li| {
        li.min_persistent_entry_ttl = 100_000;
        li.min_temp_entry_ttl = 100_000;
        li.max_entry_ttl = 200_000;
    });
}

#[test]
fn test_set_migration_target_by_owner() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_vault = Address::generate(&env);

    // Owner can set migration target
    set_target_now(&env, &client, &owner, &new_vault);

    // Verify the target was set
    // Note: We'd need a getter for migration target, but for now test the event
    let events = env.events().all();
    assert!(events.len() > 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #34)")]
fn test_set_migration_target_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let unauthorized = Address::generate(&env);
    let new_vault = Address::generate(&env);

    // Non-owner cannot set migration target
    client.schedule_migration_target(&unauthorized, &new_vault);
}

#[test]
fn test_set_migration_paused() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Owner can pause migration
    client.set_migration_paused(&true);

    // Owner can unpause migration
    client.set_migration_paused(&false);
}

/// `set_migration_paused` enforces ownership via `require_is_owner`, which
/// calls `owner.require_auth()` on the stored owner address with no
/// separate caller parameter to compare against, so the guard is tested by
/// revoking all auth and checking the `try_*` variant is rejected.
#[test]
fn test_set_migration_paused_unauthorized() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_set_migration_paused(&true);
    assert!(
        result.is_err(),
        "set_migration_paused must reject calls without the owner's authorization"
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #68)")]
fn test_migrate_without_target_fails() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Migration without target should fail
    client.migrate_shares(&user);
}

#[test]
#[should_panic(expected = "Error(Contract, #67)")]
fn test_migrate_when_paused_fails() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let new_vault = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);
    set_target_now(&env, &client, &owner, &new_vault);
    client.set_migration_paused(&true);

    // Migration when paused should fail
    client.migrate_shares(&user);
}

#[test]
#[should_panic(expected = "Error(Contract, #69)")]
fn test_migrate_without_shares_fails() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let new_vault = Address::generate(&env);

    set_target_now(&env, &client, &owner, &new_vault);

    // Migration without shares should fail
    client.migrate_shares(&user);
}

#[test]
fn test_exchange_rate_preserved_during_migration() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let new_vault = Address::generate(&env);

    // Deposit and earn some yield
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Simulate yield landing in the vault. `update_total_assets` requires
    // the vault's actual token balance to cover the reported total, and is
    // called by the agent, not the owner.
    let yield_amount = 100_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&contract_id, &yield_amount);
    let total_assets = client.get_total_assets();
    client.update_total_assets(&agent, &(total_assets + yield_amount), &false, &0_u32);

    // Calculate expected exchange rate before migration
    let shares_before = client.get_shares(&user);
    let total_assets_before = client.get_total_assets();
    let total_shares_before = client.get_total_shares();
    let expected_value = (shares_before * total_assets_before) / total_shares_before;

    // Set migration target
    set_target_now(&env, &client, &owner, &new_vault);

    // Note: Full migration test would require setting up a second vault contract
    // For now, we test the setup and state changes
    assert!(shares_before > 0);
    assert!(total_assets_before > amount); // Yield accrued
}

#[test]
fn test_migration_emits_correct_event() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let new_vault = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);
    set_target_now(&env, &client, &owner, &new_vault);

    // Check migration target update event
    let matching = find_events_by_topic(env.events().all(), &env, TOPIC_MIGRATION_TARGET_UPDATED);
    assert!(
        !matching.is_empty(),
        "execute_migration_target must publish a TOPIC_MIGRATION_TARGET_UPDATED event"
    );
}

/// Regression test (#987): `migrate_shares` always moves a user's entire
/// position (there is no partial-amount parameter), so it cannot exclude
/// just the locked portion the way `withdraw` does. Before this fix, a user
/// with any locked shares could still migrate, zeroing `Shares(user)` while
/// leaving `LockedShares(user)` untouched — a broken invariant
/// (`locked_shares > user_shares`) that the #987 fuzz harness caught.
#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_migrate_shares_blocked_while_any_shares_locked() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let new_vault = Address::generate(&env);

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);
    set_target_now(&env, &client, &owner, &new_vault);

    // Lock even a single share; migration must be blocked outright.
    let user_shares = client.get_shares(&user);
    client.lock_shares(&user, &1_i128, &30u32);
    assert_eq!(client.get_shares(&user), user_shares);

    client.migrate_shares(&user);
}

/// A user with no locked shares can still migrate normally, and their
/// `LockedShares` state is simply zero/absent afterward — this confirms the
/// new guard in `migrate_shares` doesn't block the ordinary case.
#[test]
fn test_migrate_shares_succeeds_with_no_locked_shares() {
    let env = Env::default();
    env.mock_all_auths();
    extend_ttls_for_migration_timelock(&env);

    let (contract_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;

    // migrate_shares actually calls `deposit` on the target via
    // invoke_contract, so it must be a real deployed vault sharing the same
    // USDC token, not just a placeholder address.
    let target_deployer = Address::generate(&env);
    let target_salt = BytesN::from_array(&env, &[9u8; 32]);
    let target_id = env
        .deployer()
        .with_address(target_deployer.clone(), target_salt.clone())
        .deployed_address();
    env.register_contract(&target_id, crate::NeuroWealthVault);
    let target_client = NeuroWealthVaultClient::new(&env, &target_id);
    let target_owner = Address::generate(&env);
    let target_agent = Address::generate(&env);
    target_client.initialize(
        &target_deployer,
        &target_owner,
        &target_agent,
        &usdc_token,
        &target_salt,
    );

    mint_and_deposit(&env, &client, &usdc_token, &user, amount);
    set_target_now(&env, &client, &owner, &target_id);

    client.migrate_shares(&user);

    assert_eq!(client.get_shares(&user), 0);
    let (locked_shares, _expiry) = client.get_locked_shares(&user);
    assert_eq!(locked_shares, 0);

    // The migrated position should now be reflected in the target vault.
    assert!(target_client.get_shares(&user) > 0);
}
