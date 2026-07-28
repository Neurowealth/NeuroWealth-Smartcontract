//! Ledger resource usage benchmarks for key vault operations.
//!
//! These tests measure CPU instruction counts and memory bytes consumed by
//! deposit, withdraw-with-Blend-pull, and rebalance.  They establish baseline
//! costs documented in ARCHITECTURE.md and will fail if a change causes
//! resource usage to grow beyond the recorded upper bounds.
//!
//! Upper bounds are intentionally loose (+50 % headroom over the first
//! measured values) so that minor SDK or optimisation changes do not cause
//! spurious failures.  Tighten them if you want stricter regression detection.

extern crate std;

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

// ============================================================================
// Helpers
// ============================================================================

/// Resets the env budget to zero, runs `f`, and returns (cpu, mem).
fn measure<F: FnOnce()>(env: &Env, f: F) -> (u64, u64) {
    let mut budget = env.budget();
    budget.reset_unlimited();
    f();
    (
        env.budget().cpu_instruction_cost(),
        env.budget().memory_bytes_cost(),
    )
}

// ============================================================================
// Issue #203 – deposit budget
// ============================================================================

#[test]
fn test_budget_deposit() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128; // 10 USDC
    token_client.mint(&user, &amount);

    let (cpu, mem) = measure(&env, || {
        client.deposit(&user, &amount);
    });

    std::println!("[budget] deposit  cpu={cpu}  mem={mem}");

    // Soft upper bounds — tighten after profiling stabilises
    assert!(cpu < 5_000_000, "deposit CPU cost regressed: {cpu}");
    assert!(mem < 300_000, "deposit memory cost regressed: {mem}");
}

// ============================================================================
// Issue #203 – withdraw (no Blend) budget
// ============================================================================

#[test]
fn test_budget_withdraw_no_blend() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    let (cpu, mem) = measure(&env, || {
        client.withdraw(&user, &amount, &None);
    });

    std::println!("[budget] withdraw (no Blend)  cpu={cpu}  mem={mem}");

    assert!(cpu < 5_000_000, "withdraw CPU cost regressed: {cpu}");
    assert!(mem < 300_000, "withdraw memory cost regressed: {mem}");
}

// ============================================================================
// Issue #203 – withdraw with Blend pull budget
// ============================================================================

#[test]
fn test_budget_withdraw_with_blend_pull() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Move funds into Blend so the withdraw path pulls from it
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);

    let (cpu, mem) = measure(&env, || {
        client.withdraw(&user, &amount, &None);
    });

    std::println!("[budget] withdraw (Blend pull)  cpu={cpu}  mem={mem}");

    // Cross-contract calls are more expensive; allow a wider bound
    assert!(
        cpu < 15_000_000,
        "withdraw-with-Blend CPU cost regressed: {cpu}"
    );
    assert!(
        mem < 600_000,
        "withdraw-with-Blend memory cost regressed: {mem}"
    );
}

// ============================================================================
// Issue #203 – rebalance budget
// ============================================================================

#[test]
fn test_budget_rebalance_to_blend() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    let (cpu, mem) = measure(&env, || {
        client.rebalance(&symbol_short!("blend"), &850_i128, &0_i128);
    });

    std::println!("[budget] rebalance → blend  cpu={cpu}  mem={mem}");

    assert!(cpu < 15_000_000, "rebalance CPU cost regressed: {cpu}");
    assert!(mem < 600_000, "rebalance memory cost regressed: {mem}");
}

#[test]
fn test_budget_rebalance_to_none() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);
    client.rebalance(&symbol_short!("blend"), &850_i128, &0_i128);

    let (cpu, mem) = measure(&env, || {
        client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);
    });

    std::println!("[budget] rebalance → none  cpu={cpu}  mem={mem}");

    assert!(
        cpu < 15_000_000,
        "rebalance-to-none CPU cost regressed: {cpu}"
    );
    assert!(
        mem < 600_000,
        "rebalance-to-none memory cost regressed: {mem}"
    );
}
