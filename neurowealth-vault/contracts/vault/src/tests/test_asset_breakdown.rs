//! Dedicated tests for idle/deployed asset getter consistency.

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn assert_breakdown_matches_getters(client: &NeuroWealthVaultClient) {
    let (idle, deployed) = client.get_asset_breakdown();
    assert_eq!(idle, client.get_idle_balance());
    assert_eq!(deployed, client.get_deployed_assets());
}

#[test]
fn asset_breakdown_matches_getters_for_idle_only_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;

    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    assert_eq!(client.get_idle_balance(), deposit_amount);
    assert_eq!(client.get_deployed_assets(), 0);
    assert_breakdown_matches_getters(&client);
}

#[test]
fn asset_breakdown_matches_getters_for_fully_deployed_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);

    assert_eq!(client.get_idle_balance(), 0);
    assert_eq!(client.get_deployed_assets(), deposit_amount);
    assert_breakdown_matches_getters(&client);
}

#[test]
fn asset_breakdown_matches_getters_for_partially_deployed_assets() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    let supply_limit = 4_000_000_i128;
    blend_client.set_max_supply_limit(&supply_limit);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);

    assert_eq!(client.get_idle_balance(), deposit_amount - supply_limit);
    assert_eq!(client.get_deployed_assets(), supply_limit);
    assert_breakdown_matches_getters(&client);
}
