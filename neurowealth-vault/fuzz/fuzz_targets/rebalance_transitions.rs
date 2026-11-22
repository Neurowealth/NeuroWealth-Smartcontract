//! LibFuzzer harness: random rebalance transitions against the vault.
//!
//! Exercises protocol switching (none → blend → none → dex → none)
//! to catch state-inconsistency bugs during rebalance transitions.
//!
//! Allowed panics (documented vault validation):
//! - `Error(Contract, #35)` — Paused
//! - `Error(Contract, #19)` — OnlyOwnerCanPause (not relevant here)
//! - `Error(Contract, #37)` — AmountMustBePositive
//! - `Error(Contract, #38)` — BelowMinimumDeposit
//! - `Error(Contract, #39)` — MaximumDepositExceeded
//! - `Error(Contract, #40)` — ExceedsUserDepositCap
//! - `Error(Contract, #41)` — ExceedsTvlCap
//! - `Error(Contract, #6)`  — SharesToMintMustBePositive
//! - `Error(Contract, #7)`  — InsufficientLiquidity
//! - `Error(Contract, #17)` — UnsupportedProtocol
//! - Token transfer failures

#![no_main]

use libfuzzer_sys::fuzz_target;
use neurowealth_vault::{NeuroWealthVault, NeuroWealthVaultClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{symbol_short, Address, BytesN, Env};

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

fn setup(env: &Env) -> (NeuroWealthVaultClient<'_>, Address, Address, Address) {
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

    (client, user, agent, usdc)
}

fn is_allowed_panic(msg: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "Error(Contract, #35)", // Paused
        "Error(Contract, #37)", // AmountMustBePositive
        "Error(Contract, #38)", // BelowMinimumDeposit
        "Error(Contract, #39)", // MaximumDepositExceeded
        "Error(Contract, #40)", // ExceedsUserDepositCap
        "Error(Contract, #41)", // ExceedsTvlCap
        "Error(Contract, #6)",  // SharesToMintMustBePositive
        "Error(Contract, #7)",  // InsufficientLiquidity
        "Error(Contract, #17)", // UnsupportedProtocol
        "insufficient balance",
        "amount must be positive",
        "vault: expected_apy out of range",
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

    let (client, user, agent, usdc) = setup(&env);
    let _token = FuzzTokenClient::new(&env, &usdc);

    // Supported protocols for rebalance transitions
    let protocols = [
        symbol_short!("none"),
        symbol_short!("blend"),
        symbol_short!("dex"),
    ];

    for (i, chunk) in data.chunks(4).enumerate() {
        if chunk.is_empty() {
            continue;
        }

        let op = chunk[0] % 3;
        let raw = u16::from(chunk.get(1).copied().unwrap_or(0))
            | (u16::from(chunk.get(2).copied().unwrap_or(0)) << 8);
        let amount = i128::from(raw % 20_000) * MIN_DEPOSIT + MIN_DEPOSIT;
        let protocol_idx = chunk.get(3).copied().unwrap_or(0) as usize % 3;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match op {
                0 => {
                    // Deposit to build up vault state
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
                    // Rebalance to a target protocol
                    let protocol = protocols[protocol_idx].clone();
                    let expected_apy = (raw as i128) % 10_000;
                    client.rebalance(&agent, &protocol, &expected_apy, &0);
                }
                2 => {
                    // Withdraw to test protocol exit during rebalance
                    let balance = client.get_balance(&user);
                    if balance <= 0 {
                        return;
                    }
                    let withdraw_amount = amount.min(balance);
                    if withdraw_amount <= 0 {
                        return;
                    }
                    client.withdraw(&user, &withdraw_amount);
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
