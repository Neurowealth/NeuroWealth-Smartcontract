//! Tests for multi-asset support (#646).
//!
//! Verifies:
//! - Owner-managed supported-asset list (`add_supported_asset` /
//!   `update_asset_limits` / `remove_supported_asset` / `get_supported_assets`).
//! - Per-asset share pools with independent accounting
//!   (`deposit_asset` / `withdraw_asset`).
//! - Per-asset TVL caps and deposit limits.
//! - Events include the asset identifier (`AssetDepositEvent` /
//!   `AssetWithdrawEvent` / `SupportedAssetsUpdatedEvent`).

use super::utils::*;
extern crate std;
use crate::{
    AssetConfig, AssetDepositEvent, AssetWithdrawEvent, SupportedAssetsUpdatedEvent,
    TOPIC_ASSET_DEPOSIT, TOPIC_ASSET_WITHDRAW, TOPIC_SUPPORTED_ASSETS_UPDATED,
};
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, TryFromVal};

const USDC: &str = "USDC";
const USDT: &str = "USDT";

fn asset(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

/// Registers a second test token and adds both assets to the vault.
/// Returns `(usdc_token, usdt_token)`.
fn setup_two_assets(
    env: &Env,
    client: &NeuroWealthVaultClient,
    usdc_token: &Address,
) -> (Address, Address) {
    let usdt_token = env.register_contract(None, TestToken);
    client.add_supported_asset(&asset(env, USDC), usdc_token, &0, &0, &0);
    client.add_supported_asset(&asset(env, USDT), &usdt_token, &0, &0, &0);
    (usdc_token.clone(), usdt_token)
}

#[test]
fn test_add_supported_asset_registers_and_emits_event() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.add_supported_asset(
        &asset(&env, USDC),
        &usdc_token,
        &1_000_000,
        &1_000_000_000,
        &500_000_000_000,
    );

    let supported = client.get_supported_assets();
    assert_eq!(supported.len(), 1);
    assert_eq!(supported.get(0).unwrap(), asset(&env, USDC));

    let config: Option<AssetConfig> = client.get_asset_config(&asset(&env, USDC));
    let config = config.expect("config should exist for a supported asset");
    assert_eq!(config.token_address, usdc_token);
    assert_eq!(config.min_deposit, 1_000_000);
    assert_eq!(config.deposit_limit, 1_000_000_000);
    assert_eq!(config.tvl_cap, 500_000_000_000);

    assert_eq!(client.get_asset_tvl_cap(&asset(&env, USDC)), 500_000_000_000);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_SUPPORTED_ASSETS_UPDATED);
    assert_eq!(events.len(), 1);
    let (_, _, data) = &events[0];
    let ev = SupportedAssetsUpdatedEvent::try_from_val(&env, data).expect("supported assets event");
    assert_eq!(ev.asset, asset(&env, USDC));
    assert!(ev.added);
}

#[test]
fn test_add_supported_asset_duplicate_rejected() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.add_supported_asset(&asset(&env, USDC), &usdc_token, &0, &0, &0);
    // Second registration of the same asset must panic.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.add_supported_asset(&asset(&env, USDC), &usdc_token, &0, &0, &0);
    }));
    assert!(res.is_err(), "duplicate supported asset must panic");
}

#[test]
fn test_deposit_asset_unsupported_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let usdt_token = env.register_contract(None, TestToken);
    let usdt_client = TestTokenClient::new(&env, &usdt_token);
    usdt_client.mint(&user, &100_000_000);

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.deposit_asset(&user, &asset(&env, USDT), &100_000_000);
    }));
    assert!(res.is_err(), "deposit into an unsupported asset must panic");
}#[test]
fn test_deposit_asset_mints_independent_shares() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let (usdc_token, usdt_token) = setup_two_assets(&env, &client, &usdc_token);

    let user = Address::generate(&env);
    let amount = 100_000_000_i128;

    let usdc_client = TestTokenClient::new(&env, &usdc_token);
    usdc_client.mint(&user, &amount);
    let usdt_client = TestTokenClient::new(&env, &usdt_token);
    usdt_client.mint(&user, &amount);

    client.deposit_asset(&user, &asset(&env, USDC), &amount);
    client.deposit_asset(&user, &asset(&env, USDT), &amount);

    // Per-asset share pools are independent: same user gets full shares in each.
    assert_eq!(client.get_balance_asset(&user, &asset(&env, USDC)), amount);
    assert_eq!(client.get_balance_asset(&user, &asset(&env, USDT)), amount);
    assert_eq!(client.get_total_assets_by_asset(&asset(&env, USDC)), amount);
    assert_eq!(client.get_total_assets_by_asset(&asset(&env, USDT)), amount);

    // A second user depositing only USDC must not affect the USDT pool.
    let user2 = Address::generate(&env);
    usdc_client.mint(&user2, &amount);
    client.deposit_asset(&user2, &asset(&env, USDC), &amount);
    assert_eq!(client.get_total_assets_by_asset(&asset(&env, USDC)), amount * 2);
    assert_eq!(client.get_total_assets_by_asset(&asset(&env, USDT)), amount);
    assert_eq!(client.get_balance_asset(&user2, &asset(&env, USDT)), 0);

    let totals_usdc = client.get_asset_totals(&asset(&env, USDC));
    let totals_usdt = client.get_asset_totals(&asset(&env, USDT));
    assert_eq!(totals_usdc.assets, amount * 2);
    assert_eq!(totals_usdc.shares, amount * 2);
    assert_eq!(totals_usdt.assets, amount);
    assert_eq!(totals_usdt.shares, amount);

    // Events carry the asset identifier.
    let events = find_events_by_topic(env.events().all(), &env, TOPIC_ASSET_DEPOSIT);
    assert_eq!(events.len(), 3);
    let (_, _, data) = &events[0];
    let ev = AssetDepositEvent::try_from_val(&env, data).expect("asset deposit event");
    assert_eq!(ev.asset, asset(&env, USDC));
    assert_eq!(ev.amount, amount);
    assert_eq!(ev.shares, amount);
}

#[test]
fn test_deposit_asset_respects_tvl_cap() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.add_supported_asset(&asset(&env, USDC), &usdc_token, &0, &0, &500_000_000);

    let user = Address::generate(&env);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &1_000_000_000);
    client.deposit_asset(&user, &asset(&env, USDC), &500_000_000);

    // Next deposit would push the per-asset TVL past the cap.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.deposit_asset(&user, &asset(&env, USDC), &1);
    }));
    assert!(res.is_err(), "deposit past per-asset TVL cap must panic");
}

#[test]
fn test_deposit_asset_respects_deposit_limit_and_minimum() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.add_supported_asset(&asset(&env, USDC), &usdc_token, &1_000_000, &50_000_000, &0);

    let user = Address::generate(&env);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Below the per-asset minimum.
    token_client.mint(&user, &100_000_000);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.deposit_asset(&user, &asset(&env, USDC), &999_999);
    }));
    assert!(res.is_err(), "deposit below per-asset minimum must panic");

    // Above the per-asset single-deposit limit.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.deposit_asset(&user, &asset(&env, USDC), &50_000_001);
    }));
    assert!(res.is_err(), "deposit above per-asset limit must panic");

    // Exactly at the limit works.
    client.deposit_asset(&user, &asset(&env, USDC), &50_000_000);
    assert_eq!(client.get_balance_asset(&user, &asset(&env, USDC)), 50_000_000);
}#[test]
fn test_withdraw_asset_burns_shares_and_transfers() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let (usdc_token, usdt_token) = setup_two_assets(&env, &client, &usdc_token);

    let user = Address::generate(&env);
    let amount = 100_000_000_i128;
    let usdc_client = TestTokenClient::new(&env, &usdc_token);
    usdc_client.mint(&user, &amount);
    let usdt_client = TestTokenClient::new(&env, &usdt_token);
    usdt_client.mint(&user, &amount);

    client.deposit_asset(&user, &asset(&env, USDC), &amount);
    client.deposit_asset(&user, &asset(&env, USDT), &amount);

    client.withdraw_asset(&user, &asset(&env, USDC), &40_000_000);

    assert_eq!(client.get_balance_asset(&user, &asset(&env, USDC)), 60_000_000);
    assert_eq!(client.get_balance_asset(&user, &asset(&env, USDT)), amount);
    assert_eq!(client.get_total_assets_by_asset(&asset(&env, USDC)), 60_000_000);
    assert_eq!(client.get_total_assets_by_asset(&asset(&env, USDT)), amount);
    assert_eq!(usdc_client.balance(&user), 40_000_000);

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_ASSET_WITHDRAW);
    assert_eq!(events.len(), 1);
    let (_, _, data) = &events[0];
    let ev = AssetWithdrawEvent::try_from_val(&env, data).expect("asset withdraw event");
    assert_eq!(ev.asset, asset(&env, USDC));
    assert_eq!(ev.amount, 40_000_000);
    assert_eq!(ev.shares, 40_000_000);
}

#[test]
fn test_withdraw_asset_more_than_balance_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let _ = setup_two_assets(&env, &client, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &amount);
    client.deposit_asset(&user, &asset(&env, USDC), &amount);

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.withdraw_asset(&user, &asset(&env, USDC), &(amount + 1));
    }));
    assert!(res.is_err(), "withdrawing more than the per-asset balance must panic");
}

#[test]
fn test_update_asset_limits_changes_caps() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.add_supported_asset(&asset(&env, USDC), &usdc_token, &0, &0, &0);

    client.update_asset_limits(&asset(&env, USDC), &1_000_000, &1_000_000_000, &100_000_000_000);

    let config: Option<AssetConfig> = client.get_asset_config(&asset(&env, USDC));
    let config = config.expect("config should exist");
    assert_eq!(config.min_deposit, 1_000_000);
    assert_eq!(config.deposit_limit, 1_000_000_000);
    assert_eq!(config.tvl_cap, 100_000_000_000);

    // Owner lowering the TVL cap below the current pool is allowed; only new
    // deposits past the cap are rejected.
    let user = Address::generate(&env);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &50_000_000);
    client.deposit_asset(&user, &asset(&env, USDC), &50_000_000);
    client.update_asset_limits(&asset(&env, USDC), &0, &0, &10_000_000);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.deposit_asset(&user, &asset(&env, USDC), &1);
    }));
    assert!(res.is_err(), "deposit past lowered TVL cap must panic");
}#[test]
fn test_remove_supported_asset_requires_empty_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let _ = setup_two_assets(&env, &client, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &amount);
    client.deposit_asset(&user, &asset(&env, USDC), &amount);

    // Pool is non-empty: removal must be rejected.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.remove_supported_asset(&asset(&env, USDC));
    }));
    assert!(res.is_err(), "removing a supported asset with an open pool must panic");

    // After a full withdrawal the pool is empty and removal succeeds.
    client.withdraw_asset(&user, &asset(&env, USDC), &amount);
    client.remove_supported_asset(&asset(&env, USDC));

    let supported = client.get_supported_assets();
    assert_eq!(supported.len(), 1);
    assert_eq!(supported.get(0).unwrap(), asset(&env, USDT));
    let config: Option<AssetConfig> = client.get_asset_config(&asset(&env, USDC));
    assert!(config.is_none(), "config must be deleted on removal");
}

#[test]
fn test_get_asset_balance_view() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let (usdc_token, _usdt_token) = setup_two_assets(&env, &client, &usdc_token);

    let user = Address::generate(&env);
    let amount = 25_000_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &amount);
    client.deposit_asset(&user, &asset(&env, USDC), &amount);

    let balance = client.get_asset_balance(&user, &asset(&env, USDC));
    assert_eq!(balance.shares, amount);
    assert_eq!(balance.assets, amount);
    assert_eq!(balance.pool_assets, amount);
    assert_eq!(balance.pool_shares, amount);

    // Unsupported asset: empty view, not a panic.
    let zero_balance = client.get_asset_balance(&user, &asset(&env, "EURC"));
    assert_eq!(zero_balance.shares, 0);
    assert_eq!(zero_balance.assets, 0);
    assert_eq!(zero_balance.pool_assets, 0);
}

#[test]
fn test_global_totals_track_multi_asset_deposits() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let (usdc_token, usdt_token) = setup_two_assets(&env, &client, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    let usdc_client = TestTokenClient::new(&env, &usdc_token);
    usdc_client.mint(&user, &amount);
    let usdt_client = TestTokenClient::new(&env, &usdt_token);
    usdt_client.mint(&user, &amount);

    client.deposit_asset(&user, &asset(&env, USDC), &amount);
    client.deposit_asset(&user, &asset(&env, USDT), &amount);

    // Legacy global counters reflect both assets.
    assert_eq!(client.get_total_assets(), amount * 2);
    assert_eq!(client.get_total_deposits(), amount * 2);
}