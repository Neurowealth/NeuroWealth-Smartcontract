//! Blend production-interface tests (enabled with `--features blend-devnet`, #152).
//!
//! Uses the in-env mock pool that implements the same entrypoints as mainnet Blend v2.
//! For a live network smoke test, invoke `balance`, `submit_with_allowance`, and `submit`
//! against `BLEND_POOL_ADDRESS` via the Soroban CLI (see `docs/BLEND_INTEGRATION_RESEARCH.md`).

#![cfg(all(test, feature = "blend-devnet"))]

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

#[test]
fn test_blend_production_entrypoints_on_mock_pool() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 5_000_000_i128);

    client.rebalance(&agent, &symbol_short!("blend"), &850_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
}
