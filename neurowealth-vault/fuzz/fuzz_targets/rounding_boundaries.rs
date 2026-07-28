//! LibFuzzer harness: tests rounding at boundary conditions.
//!
//! This target exercises edge cases in the vault's share-to-asset conversions:
//! - Deposits/withdrawals at minimum and maximum limits
//! - Multiple deposits/withdrawals to compound rounding effects
//! - Boundary values that could cause rounding errors in integer division
//!
//! Allowed panics (documented vault validation):
//! - `Error(Contract, #37)` — AmountMustBePositive
//! - `Error(Contract, #38)` — BelowMinimumDeposit
//! - `Error(Contract, #39)` — MaximumDepositExceeded
//! - `Error(Contract, #40)` — ExceedsUserDepositCap
//! - `Error(Contract, #41)` — ExceedsTvlCap
//! - `Error(Contract, #7)`  — InsufficientLiquidity
//! - `Error(Contract, #6)`  — SharesToMintMustBePositive
//! - `Error(Contract, #8)`  — InsufficientShares
//! - `Error(Contract, #10)` — SharesToBurnMustBePositive
//! - `Error(Contract, #11)` — InsufficientSharesForRequestedAmount
//! - Token transfer failures

#![no_main]

use libfuzzer_sys::fuzz_target;
use neurowealth_vault::{NeuroWealthVault, NeuroWealthVaultClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env};

mod token {
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

    #[contracttype]
    enum TokenDataKey {
        Balance(Address),
    }

    #[contract]
    pub struct FuzzToken;

    #[contractimpl]
    impl FuzzToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let balance: i128 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::Balance(to.clone()))
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&TokenDataKey::Balance(to), &(balance + amount));
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            assert!(amount > 0, "amount must be positive");

            let from_balance: i128 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::Balance(from.clone()))
                .unwrap_or(0);
            assert!(from_balance >= amount, "insufficient balance");

            let to_balance: i128 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::Balance(to.clone()))
                .unwrap_or(0);

            env.storage()
                .persistent()
                .set(&TokenDataKey::Balance(from), &(from_balance - amount));
            env.storage()
                .persistent()
                .set(&TokenDataKey::Balance(to), &(to_balance + amount));
        }

        pub fn balance(env: Env, owner: Address) -> i128 {
            env.storage()
                .persistent()
                .get(&TokenDataKey::Balance(owner))
                .unwrap_or(0)
        }
    }
}

use token::{FuzzToken, FuzzTokenClient};

const MIN_DEPOSIT: i128 = 1_000_000;
const MAX_DEPOSIT: i128 = 10_000_000_000;
const USER_CAP: i128 = 10_000_000_000;
const TVL_CAP: i128 = 100_000_000_000;
const TOKEN_FLOAT: i128 = 50_000_000_000;

fn setup(env: &Env) -> (NeuroWealthVaultClient<'_>, Address, Address) {
    let deployer = Address::generate(env);
    let salt = BytesN::from_array(env, &[7u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(env, &contract_id);
    let agent = Address::generate(env);
    let owner = Address::generate(env);
    let usdc = env.register_contract(None, FuzzToken);
    let user = Address::generate(env);

    client.initialize(&deployer, &owner, &agent, &usdc, &salt);

    let token = FuzzTokenClient::new(env, &usdc);
    token.mint(&user, &TOKEN_FLOAT);

    (client, user, usdc)
}

fn is_allowed_panic(msg: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "Error(Contract, #37)", // AmountMustBePositive
        "Error(Contract, #38)", // BelowMinimumDeposit
        "Error(Contract, #39)", // MaximumDepositExceeded
        "Error(Contract, #40)", // ExceedsUserDepositCap
        "Error(Contract, #41)", // ExceedsTvlCap
        "Error(Contract, #7)",  // InsufficientLiquidity
        "Error(Contract, #6)",  // SharesToMintMustBePositive
        "Error(Contract, #8)",  // InsufficientShares
        "Error(Contract, #10)", // SharesToBurnMustBePositive
        "Error(Contract, #11)", // InsufficientSharesForRequestedAmount
        "insufficient balance",
        "amount must be positive",
    ];
    ALLOWED.iter().any(|needle| msg.contains(needle))
}

fn assert_vault_invariants(client: &NeuroWealthVaultClient, user: &Address) {
    let total_shares = client.get_total_shares();
    let total_assets = client.get_total_assets();
    let user_shares = client.get_shares(user);
    let user_balance = client.get_balance(user);

    assert!(user_shares >= 0);
    assert!(user_balance >= 0);
    assert!(user_shares <= total_shares);
    if total_shares > 0 {
        assert!(user_balance <= total_assets);
    } else {
        assert_eq!(user_balance, 0);
    }
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let (client, user, usdc) = setup(&env);
    let _token = FuzzTokenClient::new(&env, &usdc);

    for (i, chunk) in data.chunks(3).enumerate() {
        if chunk.is_empty() {
            continue;
        }

        let op = chunk[0] % 4;
        let raw = u16::from(chunk.get(1).copied().unwrap_or(0))
            | (u16::from(chunk.get(2).copied().unwrap_or(0)) << 8);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match op {
                0 => {
                    // Deposit at boundary values
                    // Focus on values near MIN_DEPOSIT and MAX_DEPOSIT
                    let amount = match raw % 5 {
                        0 => MIN_DEPOSIT,
                        1 => MIN_DEPOSIT + 1,
                        2 => MAX_DEPOSIT,
                        3 => MAX_DEPOSIT - 1,
                        _ => i128::from(raw % 1000) * MIN_DEPOSIT + MIN_DEPOSIT,
                    };

                    if !(MIN_DEPOSIT..=MAX_DEPOSIT).contains(&amount)
                        || amount > USER_CAP
                        || amount > TVL_CAP
                    {
                        return;
                    }
                    let token = FuzzTokenClient::new(&env, &usdc);
                    if token.balance(&user) < amount {
                        return;
                    }
                    client.deposit(&user, &amount);
                }
                1 => {
                    // Withdraw at boundary values
                    let balance = client.get_balance(&user);
                    if balance <= 0 {
                        return;
                    }

                    let amount = match raw % 4 {
                        0 => 1, // Withdraw just 1 stroop
                        1 => balance, // Withdraw all
                        2 => balance - 1, // Withdraw all but 1
                        _ => (i128::from(raw) % balance) + 1,
                    };

                    if amount <= 0 || amount > balance {
                        return;
                    }
                    client.withdraw(&user, &amount, &None);
                }
                2 => {
                    // Deposit followed by immediate partial withdrawal (round-trip test)
                    let deposit_amount = i128::from(raw % 1000) * MIN_DEPOSIT + MIN_DEPOSIT;
                    if !(MIN_DEPOSIT..=MAX_DEPOSIT).contains(&deposit_amount)
                        || deposit_amount > USER_CAP
                        || deposit_amount > TVL_CAP
                    {
                        return;
                    }
                    let token = FuzzTokenClient::new(&env, &usdc);
                    if token.balance(&user) < deposit_amount {
                        return;
                    }
                    client.deposit(&user, &deposit_amount);

                    // Now withdraw a portion
                    let balance = client.get_balance(&user);
                    if balance > MIN_DEPOSIT {
                        let withdraw_amount = balance / 2;
                        if withdraw_amount >= MIN_DEPOSIT {
                            client.withdraw(&user, &withdraw_amount);
                        }
                    }
                }
                3 => {
                    // Multiple small deposits followed by single withdrawal
                    let small_amount = MIN_DEPOSIT;
                    let token = FuzzTokenClient::new(&env, &usdc);
                    if token.balance(&user) < small_amount * 3 {
                        return;
                    }

                    // Three small deposits
                    for _ in 0..3 {
                        if token.balance(&user) >= small_amount {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                client.deposit(&user, &small_amount);
                            }));
                        }
                    }

                    // Withdraw half of total
                    let balance = client.get_balance(&user);
                    if balance > MIN_DEPOSIT {
                        let withdraw_amount = balance / 2;
                        if withdraw_amount >= MIN_DEPOSIT {
                            client.withdraw(&user, &withdraw_amount);
                        }
                    }
                }
                _ => unreachable!(),
            }
        }));

        match result {
            Ok(()) => assert_vault_invariants(&client, &user),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                assert!(is_allowed_panic(msg), "unexpected panic at step {i}: {msg}");
            }
        }
    }
});
