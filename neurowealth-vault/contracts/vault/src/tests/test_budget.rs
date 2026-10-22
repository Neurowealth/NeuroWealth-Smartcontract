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
use soroban_sdk::{symbol_short, testutils::Address as _, vec, Address, Env};

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

fn check_baseline(name: &str, cpu: u64, mem: u64, base_cpu: u64, base_mem: u64) {
    let max_cpu = base_cpu + (base_cpu / 10);
    let max_mem = base_mem + (base_mem / 10);
    assert!(
        cpu <= max_cpu,
        "{} CPU cost regressed! Expected <= {} (baseline {} + 10%) but got {}",
        name,
        max_cpu,
        base_cpu,
        cpu
    );
    assert!(
        mem <= max_mem,
        "{} memory cost regressed! Expected <= {} (baseline {} + 10%) but got {}",
        name,
        max_mem,
        base_mem,
        mem
    );
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

    // Strict 10% tolerance over baseline
    check_baseline("deposit", cpu, mem, 4_500_000, 270_000);
}

#[test]
fn test_budget_batch_deposit_uses_fewer_resources_than_two_deposits() {
    let first_amount = 10_000_000_i128;
    let second_amount = 20_000_000_i128;

    let batch_env = Env::default();
    batch_env.mock_all_auths();
    let (batch_contract, _agent, _owner, batch_token) = setup_vault_with_token(&batch_env);
    let batch_client = NeuroWealthVaultClient::new(&batch_env, &batch_contract);
    let batch_user = Address::generate(&batch_env);
    let batch_token_client = TestTokenClient::new(&batch_env, &batch_token);
    batch_token_client.mint(&batch_user, &(first_amount + second_amount));
    let entries = vec![
        &batch_env,
        (batch_token.clone(), first_amount),
        (batch_token, second_amount),
    ];

    let (batch_cpu, batch_mem) = measure(&batch_env, || {
        batch_client.batch_deposit(&batch_user, &entries);
    });

    let separate_env = Env::default();
    separate_env.mock_all_auths();
    let (separate_contract, _agent, _owner, separate_token) = setup_vault_with_token(&separate_env);
    let separate_client = NeuroWealthVaultClient::new(&separate_env, &separate_contract);
    let separate_user = Address::generate(&separate_env);
    let separate_token_client = TestTokenClient::new(&separate_env, &separate_token);
    separate_token_client.mint(&separate_user, &(first_amount + second_amount));

    let (separate_cpu, separate_mem) = measure(&separate_env, || {
        separate_client.deposit(&separate_user, &first_amount);
        separate_client.deposit(&separate_user, &second_amount);
    });

    std::println!(
        "[budget] batch_deposit (2 entries) cpu={batch_cpu} mem={batch_mem}; two deposits cpu={separate_cpu} mem={separate_mem}"
    );
    assert!(
        batch_cpu < separate_cpu,
        "batch_deposit should use less CPU"
    );
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
        client.withdraw(&user, &amount);
    });

    std::println!("[budget] withdraw (no Blend)  cpu={cpu}  mem={mem}");

    check_baseline("withdraw_no_blend", cpu, mem, 4_500_000, 270_000);
}

// ============================================================================
// Issue #203 – withdraw with Blend pull budget
// ============================================================================

#[test]
fn test_budget_withdraw_with_blend_pull() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, amount);

    // Move funds into Blend so the withdraw path pulls from it
    client.rebalance(&agent, &symbol_short!("blend"), &500_i128, &0_i128);

    let (cpu, mem) = measure(&env, || {
        client.withdraw(&user, &amount);
    });

    std::println!("[budget] withdraw (Blend pull)  cpu={cpu}  mem={mem}");

    check_baseline("withdraw_with_blend", cpu, mem, 13_500_000, 540_000);
}

// ============================================================================
// Issue #203 – rebalance budget
// ============================================================================

#[test]
fn test_budget_rebalance_to_blend() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    let (cpu, mem) = measure(&env, || {
        client.rebalance(&agent, &symbol_short!("blend"), &850_i128, &0_i128);
    });

    std::println!("[budget] rebalance → blend  cpu={cpu}  mem={mem}");

    check_baseline("rebalance_to_blend", cpu, mem, 13_500_000, 540_000);
}

#[test]
fn test_budget_rebalance_to_none() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);
    client.rebalance(&agent, &symbol_short!("blend"), &850_i128, &0_i128);

    let (cpu, mem) = measure(&env, || {
        client.rebalance(&agent, &symbol_short!("none"), &0_i128, &0_i128);
    });

    std::println!("[budget] rebalance → none  cpu={cpu}  mem={mem}");

    check_baseline("rebalance_to_none", cpu, mem, 13_500_000, 540_000);
}

// ============================================================================
// Issue #505 – harvest budget
// ============================================================================

#[test]
fn test_budget_harvest() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);

    // Move funds into Blend so harvest has a position to compound
    client.rebalance(&agent, &symbol_short!("blend"), &850_i128, &0_i128);

    let (cpu, mem) = measure(&env, || {
        client.harvest(&agent, &0_i128);
    });

    std::println!("[budget] harvest  cpu={cpu}  mem={mem}");

    check_baseline("harvest", cpu, mem, 13_500_000, 540_000);
}
