//! Tests for pool address rotation while funds are deployed (Issue #383).
//!
//! Verify that `set_blend_pool()` / `set_dex_pool()` succeeds even when the
//! vault currently has funds deployed in the old pool, and that the next
//! rebalance correctly exits the old pool before entering the new one.
//!
//! This is a realistic operational scenario: an owner might need to rotate
//! a pool address (e.g., upgrading the pool contract itself). The vault must
//! ensure the old pool is fully exited during the next rebalance before funds
//! are deployed to the new pool.

use super::utils::*;
use soroban_sdk::{
    symbol_short,
    testutils::Address as _,
    Address, Env,
};

/// Test that set_blend_pool() succeeds while funds are deployed, and the
/// next rebalance exits the old pool and enters the new one.
#[test]
fn test_blend_pool_rotation_while_deployed_exits_old_before_entering_new() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, old_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let old_pool_client = MockBlendPoolClient::new(&env, &old_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // SETUP: Deploy funds to old pool
    client.set_blend_pool(&owner, &old_pool);
    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Rebalance to Blend — funds deployed to old pool
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(old_pool_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(token_client.balance(&contract_id), 0_i128); // All deployed

    // ROTATE: Create new pool and point to it
    let new_pool = env.register_contract(None, MockBlendPool);
    let new_pool_client = MockBlendPoolClient::new(&env, &new_pool);

    // set_blend_pool() should succeed even with funds deployed to old pool
    client.set_blend_pool(&owner, &new_pool);
    assert_eq!(client.get_blend_pool(), Some(new_pool.clone()));

    // Verify vault still points to Blend protocol, but on next rebalance will use new pool
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    // REBALANCE: Exit old pool, enter new pool
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    // Verify the rotation: funds exited old pool, entered new pool
    assert_eq!(
        old_pool_client.supplied(&usdc_token),
        0,
        "old pool should have zero funds after rebalance"
    );
    assert_eq!(
        new_pool_client.supplied(&usdc_token),
        deposit_amount,
        "new pool should have all funds after rebalance"
    );
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "protocol should still be blend"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        0_i128,
        "vault idle balance should still be zero"
    );
}

/// Test that set_dex_pool() succeeds while funds are deployed, and the next
/// rebalance exits the old pool and enters the new one.
#[test]
fn test_dex_pool_rotation_while_deployed_exits_old_before_entering_new() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, old_pool) =
        setup_vault_with_token_and_dex(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let old_pool_client = MockDexPoolClient::new(&env, &old_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // SETUP: Deploy funds to old DEX pool
    client.set_dex_pool(&owner, &old_pool);
    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Rebalance to DEX — funds deployed to old pool
    client.rebalance(&symbol_short!("dex"), &500_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("dex"));
    assert_eq!(old_pool_client.balance(&usdc_token, &contract_id), deposit_amount);
    assert_eq!(token_client.balance(&contract_id), 0_i128); // All deployed

    // ROTATE: Create new pool and point to it
    let new_pool = env.register_contract(None, MockDexPool);
    let new_pool_client = MockDexPoolClient::new(&env, &new_pool);

    // set_dex_pool() should succeed even with funds deployed to old pool
    client.set_dex_pool(&owner, &new_pool);
    assert_eq!(client.get_dex_pool(), Some(new_pool.clone()));

    // Verify vault still points to DEX protocol, but on next rebalance will use new pool
    assert_eq!(client.get_current_protocol(), symbol_short!("dex"));

    // REBALANCE: Exit old pool, enter new pool
    client.rebalance(&symbol_short!("dex"), &500_i128, &0_i128);

    // Verify the rotation: funds exited old pool, entered new pool
    assert_eq!(
        old_pool_client.balance(&usdc_token, &contract_id),
        0,
        "old pool should have zero funds after rebalance"
    );
    assert_eq!(
        new_pool_client.balance(&usdc_token, &contract_id),
        deposit_amount,
        "new pool should have all funds after rebalance"
    );
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("dex"),
        "protocol should still be dex"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        0_i128,
        "vault idle balance should still be zero"
    );
}

/// Edge case: Set pool to the same address (idempotent, no-op).
#[test]
fn test_blend_pool_rotation_to_same_address_is_noop() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let pool_client = MockBlendPoolClient::new(&env, &pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Deploy funds to pool
    client.set_blend_pool(&owner, &pool);
    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(pool_client.supplied(&usdc_token), deposit_amount);

    // Set pool to the same address — should be a silent no-op
    client.set_blend_pool(&owner, &pool);
    assert_eq!(client.get_blend_pool(), Some(pool.clone()));

    // Rebalance should work normally (no-op rebalance to same pool)
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    // Verify funds remain in the pool (no exit/re-enter occurred)
    assert_eq!(
        pool_client.supplied(&usdc_token),
        deposit_amount,
        "funds should remain in same pool after idempotent set + rebalance"
    );
}

/// Edge case: Set pool while no funds are deployed.
#[test]
fn test_blend_pool_rotation_with_zero_deployed_funds() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, pool_a) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Set initial pool but don't deploy any funds
    client.set_blend_pool(&owner, &pool_a);
    assert_eq!(client.get_blend_pool(), Some(pool_a));

    // Rotate to a new pool while idle (no funds deployed)
    let pool_b = env.register_contract(None, MockBlendPool);
    client.set_blend_pool(&owner, &pool_b);
    assert_eq!(client.get_blend_pool(), Some(pool_b));

    // Protocol should be "none" (no deployment has happened)
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
}

/// Edge case: Set pool, rotate back immediately before rebalance.
#[test]
fn test_blend_pool_rotation_back_and_forth_before_rebalance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, pool_a) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let pool_a_client = MockBlendPoolClient::new(&env, &pool_a);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Deploy funds to pool A
    client.set_blend_pool(&owner, &pool_a);
    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(pool_a_client.supplied(&usdc_token), deposit_amount);

    // Rotate: Pool A → Pool B
    let pool_b = env.register_contract(None, MockBlendPool);
    client.set_blend_pool(&owner, &pool_b);
    assert_eq!(client.get_blend_pool(), Some(pool_b));

    // Immediately rotate back: Pool B → Pool A
    client.set_blend_pool(&owner, &pool_a);
    assert_eq!(client.get_blend_pool(), Some(pool_a));

    // Rebalance should still work correctly (detect funds already in pool A, no-op)
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    // Verify funds remain in pool A (no unnecessary exit/re-entry)
    assert_eq!(
        pool_a_client.supplied(&usdc_token),
        deposit_amount,
        "funds should remain in pool A"
    );
}

/// Edge case: Cross-protocol rotation (Blend → DEX while deployed).
#[test]
fn test_cross_protocol_rotation_blend_to_dex_with_deployed_funds() {
    let env = Env::default();
    env.mock_all_auths();

    // Setup with Blend
    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let blend_pool_client = MockBlendPoolClient::new(&env, &blend_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Deploy funds to Blend
    client.set_blend_pool(&owner, &blend_pool);
    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(blend_pool_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    // Now configure DEX (this doesn't automatically exit Blend)
    let dex_pool = env.register_contract(None, MockDexPool);
    client.set_dex_pool(&owner, &dex_pool);

    // Verify Blend still holds funds (DEX setup doesn't auto-exit)
    assert_eq!(blend_pool_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    // Rebalance to DEX — should exit Blend first, then enter DEX
    let dex_pool_client = MockDexPoolClient::new(&env, &dex_pool);
    client.rebalance(&symbol_short!("dex"), &500_i128, &0_i128);

    // Verify transition: Blend exited, DEX entered
    assert_eq!(
        blend_pool_client.supplied(&usdc_token),
        0,
        "funds should be fully exited from Blend"
    );
    assert_eq!(
        dex_pool_client.balance(&usdc_token, &contract_id),
        deposit_amount,
        "funds should be deployed to DEX"
    );
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("dex"),
        "protocol should now be dex"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        0_i128,
        "vault idle balance should be zero"
    );
}

/// Edge case: Partial exit during pool rotation (withdrawal mid-rotation).
#[test]
fn test_blend_pool_rotation_with_partial_user_withdrawal() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, old_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let old_pool_client = MockBlendPoolClient::new(&env, &old_pool);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    // Deploy funds to old pool
    client.set_blend_pool(&owner, &old_pool);
    let user = Address::generate(&env);
    let deposit_amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(old_pool_client.supplied(&usdc_token), deposit_amount);

    // User withdraws some funds (requires exit from old pool)
    let withdraw_amount = 3_000_000_i128;
    client.withdraw(&user, &withdraw_amount, &None);

    // Verify funds were exited from old pool to cover withdrawal
    let remaining_deployed = old_pool_client.supplied(&usdc_token);
    assert!(
        remaining_deployed < deposit_amount,
        "some funds should have been withdrawn from old pool"
    );

    // Now rotate the pool while remaining funds are still deployed
    let new_pool = env.register_contract(None, MockBlendPool);
    client.set_blend_pool(&owner, &new_pool);

    // Rebalance to new pool
    let new_pool_client = MockBlendPoolClient::new(&env, &new_pool);
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    // Verify transition happened correctly with reduced balance
    assert_eq!(
        old_pool_client.supplied(&usdc_token),
        0,
        "all remaining funds should be exited from old pool"
    );
    assert_eq!(
        new_pool_client.supplied(&usdc_token),
        remaining_deployed,
        "remaining funds should be deployed to new pool"
    );
}
