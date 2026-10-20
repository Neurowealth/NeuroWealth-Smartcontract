//! DEX production-interface tests (enabled with `--features dex-devnet`, #228).
//!
//! Uses the in-env mock pool that implements the same entrypoints expected from a
//! Stellar DEX liquidity pool (`add_liquidity`, `remove_liquidity`, `balance`).
//! For a live network smoke test, invoke these against `DEX_POOL_ADDRESS` via the
//! Soroban CLI (see `docs/DEX_INTEGRATION.md`).

#![cfg(all(test, feature = "dex-devnet"))]

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

#[test]
fn test_dex_production_entrypoints_on_mock_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    client.rebalance(&agent, &symbol_short!("dex"), &850_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("dex"));

    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
}
