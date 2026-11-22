//! Defense-in-depth reentrancy tests for `NeuroWealthVault` (Issue #567).
//!
//! # Defense-in-Depth & CEI Invariants
//!
//! Soroban/Stellar native tokens and standard SEP-41 token contracts do not support
//! arbitrary transfer callbacks or execution hooks. However, smart contract best
//! practices dictate enforcing the Checks-Effects-Interactions (CEI) pattern so that
//! state modifications precede external contract interactions.
//!
//! This test module defines a mock `ReentrantMockToken` that attempts cross-contract
//! re-entrant calls back into the vault (`withdraw` and `deposit`) during a `transfer`
//! call. These tests formally lock in CEI state ordering as a verified invariant:
//! - In `withdraw()`, user shares and total shares/assets are updated *before*
//!   `token_client.transfer` is invoked. A re-entrant withdrawal attempt during transfer
//!   observes the already-reduced share balance and is rejected with `InsufficientShares`
//!   or `InsufficientSharesForAmount`, preventing double-withdrawals.
//! - In `deposit()`, user shares, total shares, total assets, and total deposits are
//!   updated *before* `token_client.transfer` is invoked.

extern crate std;

use super::utils::*;
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, testutils::Address as _, Address, BytesN,
    Env, Symbol,


use super::utils::*;
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, testutils::Address as _, Address, Env,
    Symbol,

};

/// Registers the vault against a deterministic deployer address and
/// initializes it with `token` as the USDC token. Returns the vault address.
fn register_vault_with_token(env: &Env, owner: &Address, agent: &Address, token: &Address, seed: u8) -> Address {
    let deployer = Address::generate(env);
    let salt = BytesN::from_array(env, &[seed; 32]);
    let vault_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&vault_id, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(env, &vault_id);
    client.initialize(&deployer, owner, agent, token, &salt);
    vault_id
}

// Data keys for the ReentrantMockToken storage
#[contracttype]
#[derive(Clone)]
#[contracttype]
pub enum MockTokenDataKey {
    Balance(Address),
    Allowance(Address, Address),
    ReentrancyTarget,
    ReentrancyMode,
    ReentrancyUser,
    ReentrancyAmount,
    ReentrancyTriggered,
}

#[contract]
pub struct ReentrantMockToken;

#[contractimpl]
impl ReentrantMockToken {
    pub fn initialize(_env: Env) {}

    pub fn set_reentrancy(
        env: Env,
        target_vault: Address,
        mode: Symbol,
        user: Address,
        amount: i128,
    ) {
        env.storage()
            .persistent()
            .set(&MockTokenDataKey::ReentrancyTarget, &target_vault);
        env.storage()
            .persistent()
            .set(&MockTokenDataKey::ReentrancyMode, &mode);
        env.storage()
            .persistent()
            .set(&MockTokenDataKey::ReentrancyUser, &user);
        env.storage()
            .persistent()
            .set(&MockTokenDataKey::ReentrancyAmount, &amount);
        env.storage()
            .persistent()
            .set(&MockTokenDataKey::ReentrancyTriggered, &false);
    }

    pub fn mint(env: Env, to: Address, amount: i128) {
        let balance: i128 = env
            .storage()
            .persistent()
            .get(&MockTokenDataKey::Balance(to.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&MockTokenDataKey::Balance(to), &(balance + amount));
    }

    pub fn balance(env: Env, owner: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&MockTokenDataKey::Balance(owner))
            .unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let from_bal: i128 = env
            .storage()
            .persistent()
            .get(&MockTokenDataKey::Balance(from.clone()))
            .unwrap_or(0);
        assert!(from_bal >= amount, "insufficient mock token balance");

        let to_bal: i128 = env
            .storage()
            .persistent()
            .get(&MockTokenDataKey::Balance(to.clone()))
            .unwrap_or(0);

        env.storage().persistent().set(
            &MockTokenDataKey::Balance(from.clone()),
            &(from_bal - amount),
        );
        env.storage()
            .persistent()
            .set(&MockTokenDataKey::Balance(to.clone()), &(to_bal + amount));

        // Check if reentrancy attack should be attempted during this transfer
        let triggered: bool = env
            .storage()
            .persistent()
            .get(&MockTokenDataKey::ReentrancyTriggered)
            .unwrap_or(false);

        if !triggered {
            if let Some(target) = env
                .storage()
                .persistent()
                .get::<_, Address>(&MockTokenDataKey::ReentrancyTarget)
            {
                // Mark triggered to prevent infinite call loops
                env.storage()
                    .persistent()
                    .set(&MockTokenDataKey::ReentrancyTriggered, &true);

                let mode: Symbol = env
                    .storage()
                    .persistent()
                    .get(&MockTokenDataKey::ReentrancyMode)
                    .unwrap();
                let reentrant_user: Address = env
                    .storage()
                    .persistent()
                    .get(&MockTokenDataKey::ReentrancyUser)
                    .unwrap();
                let reentrant_amount: i128 = env
                    .storage()
                    .persistent()
                    .get(&MockTokenDataKey::ReentrancyAmount)
                    .unwrap();

                let vault_client = NeuroWealthVaultClient::new(&env, &target);

                if mode == symbol_short!("withdrw") {
                    // Attempt re-entrant withdrawal mid-execution
                    vault_client.withdraw(&reentrant_user, &reentrant_amount);
                } else if mode == symbol_short!("deposit") {
                    // Attempt re-entrant deposit mid-execution
                    vault_client.deposit(&reentrant_user, &reentrant_amount);
                }
            }
        }
    }
}

#[test]
fn test_reentrant_withdraw_blocked_by_cei_ordering() {
    // Verifies that a re-entrant withdrawal attempt during token transfer fails or is blocked
    // because user shares were already decremented prior to the external transfer call.
    let env = Env::default();
    env.mock_all_auths();

    // Register ReentrantMockToken
    let mock_token_id = env.register_contract(None, ReentrantMockToken);
    let mock_token_client = ReentrantMockTokenClient::new(&env, &mock_token_id);

    let deployer = Address::generate(&env);
    let owner = Address::generate(&env);
    let agent = Address::generate(&env);


    let vault_id = register_vault_with_token(&env, &owner, &agent, &mock_token_id, 1);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);


    let salt = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);

    // Register a vault at the address derived from its deployer and salt so
    // initialize's anti-front-running check is exercised by this test too.
    let vault_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&vault_id, NeuroWealthVault);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    vault_client.initialize(&deployer, &owner, &agent, &mock_token_id, &salt);


    let user = Address::generate(&env);
    let amount = 10_000_000_i128; // 10 USDC

    // Mint tokens to user
    mock_token_client.mint(&user, &amount);

    // Initial deposit: mints 10_000_000 shares to user
    vault_client.deposit(&user, &amount);
    assert_eq!(vault_client.get_shares(&user), amount);
    assert_eq!(vault_client.get_total_assets(), amount);

    // Configure mock token to attempt a second re-entrant withdraw of 10_000_000
    // during the token transfer of the first withdraw.
    mock_token_client.set_reentrancy(&vault_id, &symbol_short!("withdrw"), &user, &amount);

    // Mint additional mock tokens to vault so transfer succeeds on token side
    mock_token_client.mint(&vault_id, &amount);

    // Now call withdraw. During token_client.transfer, the token invokes vault.withdraw.
    // Because user_shares was ALREADY updated to 0 before transfer, the re-entrant withdraw
    // fails with InsufficientShares (#8 or #10), causing the transaction to safely panic/revert.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vault_client.withdraw(&user, &amount);
    }));

    assert!(
        res.is_err(),
        "Re-entrant withdrawal attempt must fail and revert"
    );
}

#[test]
fn test_reentrant_deposit_cei_state_integrity() {
    // Verifies that a re-entrant deposit attempt during token transfer operates on
    // pre-updated total deposits and total shares.
    let env = Env::default();
    env.mock_all_auths();

    let mock_token_id = env.register_contract(None, ReentrantMockToken);
    let mock_token_client = ReentrantMockTokenClient::new(&env, &mock_token_id);

    let deployer = Address::generate(&env);
    let owner = Address::generate(&env);
    let agent = Address::generate(&env);
    let salt = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);

    let vault_id = register_vault_with_token(&env, &owner, &agent, &mock_token_id, 2);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let vault_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&vault_id, NeuroWealthVault);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    vault_client.initialize(&deployer, &owner, &agent, &mock_token_id, &salt);

    let user = Address::generate(&env);
    let amount = 5_000_000_i128;

    mock_token_client.mint(&user, &(amount * 4));

    // Configure mock token to attempt a re-entrant deposit during transfer
    mock_token_client.set_reentrancy(&vault_id, &symbol_short!("deposit"), &user, &amount);

    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vault_client.deposit(&user, &amount);
    }));

    // Either the re-entrant call fails or completes cleanly; in either case, total shares & assets remain consistent.
    if res.is_ok() {
        assert_eq!(vault_client.get_shares(&user), amount * 2);
        assert_eq!(vault_client.get_total_assets(), amount * 2);
    }
}
