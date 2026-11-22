//! Flash-loan protection (`MinHoldingPeriod`) coverage across every deposit
//! and withdraw entrypoint (#990).
//!
//! Before this fix, `batch_deposit` and `deposit_asset` never wrote
//! `DataKey::LastDepositLedger`, and `withdraw_asset` never checked it —
//! so a user could enter through either deposit path and withdraw in the
//! same ledger, bypassing the holding period entirely. `deposit` and
//! `withdraw`/`withdraw_all` already round-trip this key correctly and
//! are covered elsewhere (e.g. `test_withdraw.rs`); this file only
//! targets the three gaps #990 identified.

#![cfg(test)]

use super::utils::*;
use crate::VaultError;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol, Vec,
};

const USDC: &str = "USDC";

fn asset(env: &Env, name: &str) -> Symbol {
    Symbol::new(env, name)
}

#[test]
fn test_batch_deposit_sets_last_deposit_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_min_holding_period(&10_u32);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &amount);

    let mut entries = Vec::new(&env);
    entries.push_back((usdc_token.clone(), amount));
    client.batch_deposit(&user, &entries);

    // Withdrawing in the same ledger must now be rejected, proving
    // batch_deposit recorded LastDepositLedger just like deposit() does.
    let result = client.try_withdraw_all(&user);
    assert!(
        result.is_err(),
        "batch_deposit must start the holding-period clock"
    );
    assert_eq!(
        result.err().unwrap().unwrap(),
        VaultError::HoldingPeriodNotElapsed.into()
    );
}

#[test]
fn test_batch_deposit_allows_withdrawal_after_holding_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let min_holding = 10_u32;
    client.set_min_holding_period(&min_holding);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &amount);

    let mut entries = Vec::new(&env);
    entries.push_back((usdc_token.clone(), amount));
    client.batch_deposit(&user, &entries);

    let current_sequence = env.ledger().sequence();
    env.ledger().with_mut(|li| {
        li.sequence_number = current_sequence + min_holding;
    });

    let withdrawn = client.withdraw_all(&user);
    assert!(
        withdrawn > 0,
        "withdraw_all should succeed once the holding period has elapsed"
    );
}

#[test]
fn test_deposit_asset_sets_last_deposit_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.add_supported_asset(&asset(&env, USDC), &usdc_token, &0, &0, &0);
    client.set_min_holding_period(&10_u32);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &amount);

    client.deposit_asset(&user, &asset(&env, USDC), &amount);

    // withdraw_asset itself is the function under test for the check side
    // of #990; same-ledger withdrawal must now be rejected.
    let result = client.try_withdraw_asset(&user, &asset(&env, USDC), &amount);
    assert!(
        result.is_err(),
        "deposit_asset must start the holding-period clock"
    );
    assert_eq!(
        result.err().unwrap().unwrap(),
        VaultError::HoldingPeriodNotElapsed.into()
    );
}

#[test]
fn test_withdraw_asset_allowed_after_holding_period() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.add_supported_asset(&asset(&env, USDC), &usdc_token, &0, &0, &0);
    let min_holding = 10_u32;
    client.set_min_holding_period(&min_holding);

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &amount);

    client.deposit_asset(&user, &asset(&env, USDC), &amount);

    let current_sequence = env.ledger().sequence();
    env.ledger().with_mut(|li| {
        li.sequence_number = current_sequence + min_holding;
    });

    client.withdraw_asset(&user, &asset(&env, USDC), &amount);
    assert_eq!(
        client.get_balance_asset(&user, &asset(&env, USDC)),
        0,
        "the full balance should have been withdrawn"
    );
}

#[test]
fn test_withdraw_asset_with_no_holding_period_configured_succeeds_immediately() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.add_supported_asset(&asset(&env, USDC), &usdc_token, &0, &0, &0);
    // No min holding period configured: 0 is the default, meaning the
    // flash-loan protection check must be a no-op.

    let user = Address::generate(&env);
    let amount = 1_000_000_i128;
    let token_client = TestTokenClient::new(&env, &usdc_token);
    token_client.mint(&user, &amount);

    client.deposit_asset(&user, &asset(&env, USDC), &amount);

    // Same-ledger withdrawal must succeed when no holding period is set.
    client.withdraw_asset(&user, &asset(&env, USDC), &amount);
    assert_eq!(client.get_balance_asset(&user, &asset(&env, USDC)), 0);
}
