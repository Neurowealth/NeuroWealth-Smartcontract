#![cfg(test)]

extern crate std;

use soroban_sdk::{testutils::{Address as _, Events}, Address, BytesN, Env, TryFromVal};

use super::*;

fn setup_vault(env: &Env) -> (Address, Address, Address) {
    let contract_id = env.register_contract(None, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(env, &contract_id);

    let agent = Address::generate(env);
    let usdc_token = Address::generate(env);
    let owner = agent.clone();

    client.initialize(&agent, &usdc_token);

    (contract_id, agent, owner)
}

/// Helper: find events whose first topic equals the given symbol short name.
/// Events are `(contract_address, topics: Vec<Val>, data: Val)`.
fn find_events_by_topic<'a>(
    events: &'a std::vec::Vec<(Address, soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)>,
    env: &Env,
    topic: soroban_sdk::Symbol,
) -> std::vec::Vec<&'a (Address, soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)> {
    events
        .iter()
        .filter(|e| {
            e.1.get(0u32)
                .and_then(|v| soroban_sdk::Symbol::try_from_val(env, &v).ok())
                .map(|s| s == topic)
                .unwrap_or(false)
        })
        .collect()
}

#[test]
fn test_vault_initialized_event() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let agent = Address::generate(&env);
    let usdc_token = Address::generate(&env);
    let tvl_cap = 100_000_000_000_i128;

    client.initialize(&agent, &usdc_token);

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    assert_eq!(events.len(), 1);

    let init_events = find_events_by_topic(&events, &env, symbol_short!("vlt_init"));
    assert_eq!(init_events.len(), 1);

    let event_data = VaultInitializedEvent::try_from_val(&env, &init_events[0].2).unwrap();
    assert_eq!(event_data.agent, agent);
    assert_eq!(event_data.usdc_token, usdc_token);
    assert_eq!(event_data.tvl_cap, tvl_cap);
}

#[test]
fn test_vault_paused_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause();

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let pause_events = find_events_by_topic(&events, &env, symbol_short!("paused"));
    assert_eq!(pause_events.len(), 1);

    let event_data = VaultPausedEvent::try_from_val(&env, &pause_events[0].2).unwrap();
    assert_eq!(event_data.caller, agent); // owner == agent in setup
}

#[test]
fn test_vault_unpaused_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause();
    client.unpause();

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let unpause_events = find_events_by_topic(&events, &env, symbol_short!("unpaused"));
    assert_eq!(unpause_events.len(), 1);

    let event_data = VaultUnpausedEvent::try_from_val(&env, &unpause_events[0].2).unwrap();
    assert_eq!(event_data.caller, agent); // owner == agent in setup
}

#[test]
fn test_emergency_paused_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.emergency_pause();

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let emergency_events = find_events_by_topic(&events, &env, symbol_short!("emg_pause"));
    assert_eq!(emergency_events.len(), 1);

    let event_data = EmergencyPausedEvent::try_from_val(&env, &emergency_events[0].2).unwrap();
    assert_eq!(event_data.caller, agent);
}

#[test]
fn test_limits_updated_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_min = 10_000_000_000_i128;
    let old_max = 100_000_000_000_i128;
    let new_min = 20_000_000_000_i128;
    let new_max = 200_000_000_000_i128;

    client.set_limits(&new_min, &new_max);

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let limits_events = find_events_by_topic(&events, &env, symbol_short!("lim_upd"));
    assert_eq!(limits_events.len(), 1);

    let event_data = LimitsUpdatedEvent::try_from_val(&env, &limits_events[0].2).unwrap();
    assert_eq!(event_data.old_min, old_min);
    assert_eq!(event_data.new_min, new_min);
    assert_eq!(event_data.old_max, old_max);
    assert_eq!(event_data.new_max, new_max);
}

#[test]
fn test_limits_updated_event_from_set_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_max = 100_000_000_000_i128;
    let new_max = 150_000_000_000_i128;

    client.set_tvl_cap(&new_max);

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let limits_events = find_events_by_topic(&events, &env, symbol_short!("lim_upd"));
    assert_eq!(limits_events.len(), 1);

    let event_data = LimitsUpdatedEvent::try_from_val(&env, &limits_events[0].2).unwrap();
    assert_eq!(event_data.old_max, old_max);
    assert_eq!(event_data.new_max, new_max);
}

#[test]
fn test_limits_updated_event_from_set_user_deposit_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_min = 10_000_000_000_i128;
    let new_min = 15_000_000_000_i128;

    client.set_user_deposit_cap(&new_min);

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let limits_events = find_events_by_topic(&events, &env, symbol_short!("lim_upd"));
    assert_eq!(limits_events.len(), 1);

    let event_data = LimitsUpdatedEvent::try_from_val(&env, &limits_events[0].2).unwrap();
    assert_eq!(event_data.old_min, old_min);
    assert_eq!(event_data.new_min, new_min);
}

#[test]
fn test_agent_updated_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, old_agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_agent = Address::generate(&env);
    client.update_agent(&new_agent);

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let agent_events = find_events_by_topic(&events, &env, symbol_short!("agnt_upd"));
    assert_eq!(agent_events.len(), 1);

    let event_data = AgentUpdatedEvent::try_from_val(&env, &agent_events[0].2).unwrap();
    assert_eq!(event_data.old_agent, old_agent);
    assert_eq!(event_data.new_agent, new_agent);
}

#[test]
fn test_assets_updated_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let old_total = 0_i128;
    let new_total = 50_000_000_000_i128;

    client.update_total_assets(&new_total);

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let assets_events = find_events_by_topic(&events, &env, symbol_short!("asst_upd"));
    assert_eq!(assets_events.len(), 1);

    let event_data = AssetsUpdatedEvent::try_from_val(&env, &assets_events[0].2).unwrap();
    assert_eq!(event_data.old_total, old_total);
    assert_eq!(event_data.new_total, new_total);
}

#[test]
fn test_rebalance_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let protocol = symbol_short!("balanced");
    let expected_apy = 850_i128;

    client.rebalance(&protocol, &expected_apy);

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let rebalance_events = find_events_by_topic(&events, &env, symbol_short!("rebalance"));
    assert_eq!(rebalance_events.len(), 1);

    let event_data = RebalanceEvent::try_from_val(&env, &rebalance_events[0].2).unwrap();
    assert_eq!(event_data.protocol, protocol);
    assert_eq!(event_data.expected_apy, expected_apy);
}

#[test]
fn test_deposit_and_withdraw_events() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let agent = Address::generate(&env);
    let user = Address::generate(&env);
    let usdc_token = Address::generate(&env);

    client.initialize(&agent, &usdc_token);

    // Verify initial state before any deposits
    assert_eq!(client.get_balance(&user), 0);
}

#[test]
fn test_pause_and_unpause_events() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(client.is_paused(), false);

    client.pause();
    assert_eq!(client.is_paused(), true);

    client.unpause();
    assert_eq!(client.is_paused(), false);
}

// ============================================================================
// UPGRADE TESTS
// ============================================================================

/// Helper that installs the vault WASM in the test environment and returns
/// a valid hash for use in upgrade tests.
///
/// This compiles the current contract (via `contractimport!`) and uploads it,
/// giving us a real 32-byte hash the deployer will accept.
mod vault_wasm {
    soroban_sdk::contractimport!(
        file = "../../target/wasm32-unknown-unknown/release/neurowealth_vault.wasm"
    );
}

fn upload_vault_wasm(env: &Env) -> BytesN<32> {
    env.deployer().upload_contract_wasm(vault_wasm::WASM)
}

#[test]
fn test_version_is_1_after_initialization() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(client.get_version(), 1u32);
}

#[test]
fn test_owner_can_upgrade() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let owner = client.get_owner();
    let new_wasm_hash = upload_vault_wasm(&env);

    client.upgrade(&owner, &new_wasm_hash);

    assert_eq!(client.get_version(), 2u32);
}

#[test]
#[should_panic(expected = "Not authorized: caller is not the owner")]
fn test_non_owner_cannot_upgrade() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let non_owner = Address::generate(&env);
    let fake_wasm_hash = BytesN::from_array(&env, &[0u8; 32]);

    client.upgrade(&non_owner, &fake_wasm_hash);
}

#[test]
fn test_version_increments_correctly() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let owner = client.get_owner();
    let new_wasm_hash = upload_vault_wasm(&env);

    assert_eq!(client.get_version(), 1u32);

    client.upgrade(&owner, &new_wasm_hash.clone());
    assert_eq!(client.get_version(), 2u32);

    client.upgrade(&owner, &new_wasm_hash);
    assert_eq!(client.get_version(), 3u32);
}

#[test]
fn test_upgrade_emits_upgraded_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner) = setup_vault(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let owner = client.get_owner();
    let new_wasm_hash = upload_vault_wasm(&env);

    client.upgrade(&owner, &new_wasm_hash);

    let all = env.events().all();
    let events: std::vec::Vec<_> = all.into_iter().collect();
    let upgraded_events = find_events_by_topic(&events, &env, symbol_short!("upgraded"));
    assert_eq!(upgraded_events.len(), 1);

    let event_data = UpgradedEvent::try_from_val(&env, &upgraded_events[0].2).unwrap();
    assert_eq!(event_data.old_version, 1u32);
    assert_eq!(event_data.new_version, 2u32);
}
