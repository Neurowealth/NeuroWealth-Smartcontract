//! Regression coverage for compounding externally supplied rewards.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_liquidity_mining_rewards_claim_and_compound() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, agent, _owner, usdc_id) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let user = Address::generate(&env);

    // Initial deposit.
    mint_and_deposit(&env, &client, &usdc_id, &user, 10_000_000);
    assert_eq!(client.get_total_assets(), 10_000_000);

    // Simulate reward distribution and report the new backing value.
    let token = TestTokenClient::new(&env, &usdc_id);
    token.mint(&vault_id, &500_000);
    client.update_total_assets(&agent, &10_500_000, &false, &0);

    assert_eq!(client.get_total_assets(), 10_500_000);
}
