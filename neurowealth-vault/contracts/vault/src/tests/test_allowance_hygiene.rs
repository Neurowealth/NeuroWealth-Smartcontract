//! Tests that `supply_to_blend` / `supply_to_dex` revoke any leftover token
//! allowance after a pool pulls less than it was approved for (#985).
//!
//! Without this, a partial pull leaves the pool's allowance live until
//! `approval_ledger` (often ~5.7 days of ledgers, see `DEFAULT_APPROVAL_TTL`),
//! during which the pool — or a compromised or upgraded pool at the same
//! address — could pull the remaining approved amount later, unrelated to
//! the supply call that originally approved it.

#![cfg(test)]

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

#[test]
fn test_blend_partial_supply_revokes_leftover_allowance() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit = 20_000_000_i128; // 20 USDC
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Cap the pool so it only pulls half of what the vault approves. `0` bps
    // slippage tolerance means "require the full amount" (see
    // minimum_out_for_leg), so tolerate up to 50% shortfall explicitly.
    let half = deposit / 2;
    blend_client.set_max_supply_limit(&half);

    client.rebalance(&agent, &symbol_short!("blend"), &600_i128, &5_000_i128);

    // The pool only pulled `half`, leaving `deposit - half` of the approval
    // unspent. That leftover must be revoked, not left live.
    assert_eq!(
        token_client.allowance(&vault_id, &blend_pool),
        0,
        "leftover Blend allowance must be revoked after a partial pull"
    );
}

#[test]
fn test_blend_full_supply_leaves_no_leftover_allowance() {
    // A full pull naturally drains the allowance to 0 via transfer_from,
    // so the explicit revoke path should be a no-op here, not a second
    // unnecessary approve call. Covered by asserting the end state matches
    // the partial-pull test's: either way, nothing should be outstanding.
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, agent, owner, usdc_token, blend_pool) = setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_blend_pool(&owner, &blend_pool);

    let deposit = 20_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    client.rebalance(&agent, &symbol_short!("blend"), &600_i128, &0_i128);

    assert_eq!(
        token_client.allowance(&vault_id, &blend_pool),
        0,
        "a full pull already drains the allowance; nothing should be left outstanding"
    );
}

#[test]
fn test_dex_partial_supply_revokes_leftover_allowance() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, agent, owner, usdc_token, dex_pool) = setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    client.set_dex_pool(&owner, &dex_pool);

    let deposit = 20_000_000_i128;
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    let half = deposit / 2;
    dex_client.set_max_supply_limit(&half);

    client.rebalance(&agent, &symbol_short!("dex"), &600_i128, &5_000_i128);

    assert_eq!(
        token_client.allowance(&vault_id, &dex_pool),
        0,
        "leftover DEX allowance must be revoked after a partial pull"
    );
}
