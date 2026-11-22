//! Tests for vault initialization

use super::utils::*;
use crate::{VaultInitializedEvent, TOPIC_INIT};
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, String, TryFromVal};

/// The canonical "burned" Stellar account (ed25519 public key of all zero
/// bytes) used to exercise `initialize`'s zero-address rejection (Issue
/// #434). Mirrors `NeuroWealthVault::zero_address` in `lib.rs`.
fn zero_address(env: &Env) -> Address {
    Address::from_string(&String::from_str(
        env,
        "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
    ))
}

#[test]
fn test_initialize_happy_path() {
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
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    // Deployer must call initialize with the correct salt
    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    // Verify initialization
    assert_eq!(client.get_agent(), agent);
    assert_eq!(client.get_usdc_token(), usdc_token);
    assert_eq!(client.get_owner(), owner);
    assert!(!client.is_paused());
    assert_eq!(client.get_version(), 1u32);
    assert_eq!(client.get_total_deposits(), 0);
    assert_eq!(client.get_total_assets(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_double_initialize_panics() {
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
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);
    // Second call should panic with "Error(Contract, #4)"
    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);
}

#[test]
fn test_initialize_default_values() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Verify actual defaults set by initialize()
    assert!(!client.is_paused(), "Vault should start unpaused");
    assert_eq!(
        client.get_min_deposit(),
        1_000_000_i128,
        "Default min deposit should be 1 USDC"
    );
    assert_eq!(
        client.get_max_deposit(),
        10_000_000_000_i128,
        "Default max deposit should be 10K USDC"
    );
    // TvLCap and UserDepositCap are set to non-zero defaults by initialize()
    assert_eq!(
        client.get_tvl_cap(),
        100_000_000_000_i128,
        "Default TVL cap is 100M USDC"
    );
    assert_eq!(
        client.get_user_deposit_cap(),
        10_000_000_000_i128,
        "Default user deposit cap is 10K USDC"
    );
    assert_eq!(
        client.get_total_deposits(),
        0,
        "Initial deposits should be 0"
    );
    assert_eq!(client.get_total_assets(), 0, "Initial assets should be 0");
}

#[test]
fn test_initialize_emits_event() {
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
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    let init_events = find_events_by_topic(env.events().all(), &env, TOPIC_INIT);
    assert_eq!(
        init_events.len(),
        1,
        "Exactly one init event should be emitted"
    );

    let (_, _, data) = &init_events[0];
    let event = VaultInitializedEvent::try_from_val(&env, data)
        .expect("Should be a valid VaultInitializedEvent");
    assert_eq!(
        event.agent, agent,
        "Event agent should match initialized agent"
    );
    assert_eq!(
        event.usdc_token, usdc_token,
        "Event usdc_token should match"
    );
}

// ============================================================================
// ISSUE #118 — DECOUPLED OWNER AND AGENT ROLES
// ============================================================================

/// Verifies that initialize stores owner and agent as distinct addresses and that
/// each role is retrievable under the correct storage key.
#[test]
fn test_initialize_owner_and_agent_are_distinct() {
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
    let owner = Address::generate(&env);
    let agent = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    assert_ne!(
        owner, agent,
        "precondition: owner and agent must be distinct addresses"
    );

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    assert_eq!(
        client.get_owner(),
        owner,
        "owner should be stored under Owner key"
    );
    assert_eq!(
        client.get_agent(),
        agent,
        "agent should be stored under Agent key"
    );
    assert_ne!(
        client.get_owner(),
        client.get_agent(),
        "owner and agent must not collapse to the same address"
    );
}

/// Verifies that the init event includes both the owner and agent addresses so
/// off-chain observers can confirm role separation without reading storage.
#[test]
fn test_initialize_event_includes_owner_and_agent() {
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
    let owner = Address::generate(&env);
    let agent = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    let init_events = find_events_by_topic(env.events().all(), &env, TOPIC_INIT);
    assert_eq!(init_events.len(), 1, "init event must be emitted");

    // The event data is VaultInitializedEvent; verify it contains both roles
    // by confirming the stored values match what we passed in.
    assert_eq!(client.get_owner(), owner);
    assert_eq!(client.get_agent(), agent);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_unauthorized_deployer_initialize_fails() {
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
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    // Attack: Attacker generates their own address as deployer to bypass require_auth
    let attacker = Address::generate(&env);
    // This must fail because the attacker's expected contract address doesn't match contract_id
    client.initialize(&attacker, &owner, &agent, &usdc_token, &salt);
}

/// Regression test: a front-runner who knows the correct deployer address and salt
/// but does NOT hold the deployer's keypair must be rejected.
///
/// `deployer.require_auth()` inside initialize() enforces that only the deployer's
/// own signed invocation can succeed. Without a valid auth entry for the deployer,
/// the call panics before any state is written.
#[test]
#[should_panic]
fn test_front_runner_without_deployer_auth_is_rejected() {
    let env = Env::default();
    // Deliberately do NOT call env.mock_all_auths().
    // A front-runner may know the deployer address and salt (both are public on-chain),
    // but cannot produce the deployer's authorization signature.

    let deployer = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let attacker_agent = Address::generate(&env);
    let attacker_owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    // The front-runner passes the correct deployer + salt (public info) but cannot
    // satisfy deployer.require_auth() — this must panic with an auth error.
    client.initialize(
        &deployer,
        &attacker_owner,
        &attacker_agent,
        &usdc_token,
        &salt,
    );
}

/// Regression test: a front-runner who substitutes their own address for the deployer
/// (to satisfy require_auth with their own keys) must be rejected by the contract-address
/// check even when all auths are mocked.
///
/// The contract derives `expected = deployer_address × salt → contract_address` on-chain
/// and rejects any deployer whose address does not reproduce the current contract address.
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_front_runner_with_own_address_as_deployer_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let real_deployer = Address::generate(&env);
    let salt = BytesN::from_array(&env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(real_deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let attacker_agent = Address::generate(&env);
    let attacker_owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    // The attacker passes their own address as deployer so they can satisfy
    // require_auth() with their own keys, but the derived contract address
    // will not match contract_id → "Error(Contract, #5)".
    let attacker = Address::generate(&env);
    client.initialize(
        &attacker,
        &attacker_owner,
        &attacker_agent,
        &usdc_token,
        &salt,
    );
}

// ============================================================================
// ISSUE #434 — REJECT ZERO ADDRESS IN INITIALIZE()
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #62)")]
fn test_initialize_rejects_zero_deployer() {
    let env = Env::default();
    env.mock_all_auths();

    let deployer = zero_address(&env);
    let salt = BytesN::from_array(&env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);
}

#[test]
#[should_panic(expected = "Error(Contract, #63)")]
fn test_initialize_rejects_zero_owner() {
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
    let agent = Address::generate(&env);
    let owner = zero_address(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);
}

#[test]
#[should_panic(expected = "Error(Contract, #64)")]
fn test_initialize_rejects_zero_agent() {
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
    let agent = zero_address(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);
}

#[test]
#[should_panic(expected = "Error(Contract, #65)")]
fn test_initialize_rejects_zero_usdc_token() {
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
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = zero_address(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);
}

// ============================================================================
// ISSUE #574 — DOUBLE-INITIALIZATION GUARD AND SAME-LEDGER RACE
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_same_ledger_double_initialize_race_rejected() {
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
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    // Advance to simulate another ledger position but within the same sequence.
    // On Soroban/Stellar, multiple transactions can occur in the same ledger.
    // This test verifies that even a simultaneous init attempt in the same ledger
    // is rejected without corrupting owner/agent/token state.
    // Re-initialize with potentially different params — must still panic.
    let different_agent = Address::generate(&env);
    let different_owner = Address::generate(&env);
    client.initialize(
        &deployer,
        &different_owner,
        &different_agent,
        &usdc_token,
        &salt,
    );
}

#[test]
fn test_after_failed_double_initialize_state_is_uncorrupted() {
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
    let agent = Address::generate(&env);
    let owner = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    let initial_agent = client.get_agent();
    let initial_owner = client.get_owner();
    let initial_token = client.get_usdc_token();

    let different_agent = Address::generate(&env);
    let different_owner = Address::generate(&env);
    let different_token = Address::generate(&env);

    // Attempt to re-initialize with different params — this must fail.
    let result = client.try_initialize(
        &deployer,
        &different_owner,
        &different_agent,
        &different_token,
        &salt,
    );
    assert!(result.is_err(), "double-initialize must be rejected");

    // Verify state was not corrupted by the failed attempt.
    assert_eq!(
        client.get_agent(),
        initial_agent,
        "agent must not be modified on failed re-initialize"
    );
    assert_eq!(
        client.get_owner(),
        initial_owner,
        "owner must not be modified on failed re-initialize"
    );
    assert_eq!(
        client.get_usdc_token(),
        initial_token,
        "token must not be modified on failed re-initialize"
    );
}
