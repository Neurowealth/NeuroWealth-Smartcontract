//! `LibFuzzer` harness: random `set_user_strategy` / `get_user_strategy` sequences.
//!
//! Allowed panics (documented vault validation):
//! - `Error(Contract, #47)` — `InvalidStrategy` (unknown strategy string)
//! - Auth / not-initialized panics from the mock environment are filtered as
//!   well because `mock_all_auths()` handles auth; `NotInitialized` cannot fire
//!   after setup.

#![no_main]

use libfuzzer_sys::fuzz_target;
use neurowealth_vault::{NeuroWealthVault, NeuroWealthVaultClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, BytesN, Env, Symbol};

mod token {
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

    #[contracttype]
    enum TokenDataKey {
        Balance(Address),
    }

    #[contract]
    pub struct FuzzToken;

    #[allow(clippy::needless_pass_by_value)]
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

use token::FuzzToken;

const VALID_STRATEGIES: &[&str] = &["conservative", "balanced", "growth"];

fn setup(env: &Env) -> (NeuroWealthVaultClient<'_>, Address) {
    let deployer = Address::generate(env);
    let salt = BytesN::from_array(env, &[9u8; 32]);
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

    (client, user)
}

fn is_allowed_panic(msg: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "Error(Contract, #47)", // InvalidStrategy
    ];
    ALLOWED.iter().any(|needle| msg.contains(needle))
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let (client, user) = setup(&env);

    for (i, chunk) in data.chunks(2).enumerate() {
        let op = chunk[0] % 2;
        let strategy_idx = chunk[1] as usize;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if op == 0 {
                // set_user_strategy: use fuzzer byte to pick from valid + one invalid index
                if strategy_idx < VALID_STRATEGIES.len() {
                    let s = Symbol::new(&env, VALID_STRATEGIES[strategy_idx]);
                    client.set_user_strategy(&user, &s);
                } else {
                    // Deliberately supply an invalid strategy to exercise the guard.
                    let s = Symbol::new(&env, "invalid");
                    client.set_user_strategy(&user, &s);
                }
            } else {
                // get_user_strategy is infallible: always returns a valid symbol.
                let strategy = client.get_user_strategy(&user);
                let valid = strategy == Symbol::new(&env, "conservative")
                    || strategy == Symbol::new(&env, "balanced")
                    || strategy == Symbol::new(&env, "growth");
                assert!(
                    valid,
                    "get_user_strategy returned unexpected value at step {i}"
                );
            }
        }));

        if let Err(payload) = result {
            let msg = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            assert!(is_allowed_panic(msg), "unexpected panic at step {i}: {msg}");
        }
    }
});
