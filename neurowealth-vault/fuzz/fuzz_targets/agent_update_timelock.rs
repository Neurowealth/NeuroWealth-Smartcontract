//! LibFuzzer harness: random propose/confirm/cancel sequences for agent updates.
//!
//! Exercises the two-step timelock for `update_agent()`:
//! - only one pending proposal may exist at a time
//! - confirmation succeeds only once the timelock has elapsed
//! - cancellation is always available while a proposal is pending
//! - scheduled `effective_ledger` is always in the future
//!
//! Allowed panics (documented vault validation):
//! - `Error(Contract, #48)` — TimelockAlreadyPending
//! - `Error(Contract, #49)` — NoTimelockPending
//! - `Error(Contract, #50)` — TimelockNotExpired

#![no_main]

use libfuzzer_sys::fuzz_target;
use neurowealth_vault::{NeuroWealthVault, NeuroWealthVaultClient};
use soroban_sdk::testutils::{Address as _, Ledger as _};
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

use token::FuzzToken;

const AGENT_TIMELOCK_LEDGERS: u32 = 17_280;

fn setup(env: &Env) -> (NeuroWealthVaultClient<'_>, Address, Address) {
    let deployer = Address::generate(env);
    let salt = BytesN::from_array(env, &[7u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(env, &contract_id);
    let owner = Address::generate(env);
    let agent = Address::generate(env);
    let usdc = env.register_contract(None, FuzzToken);

    client.initialize(&deployer, &owner, &agent, &usdc, &salt);

    (client, owner, agent)
}

fn is_allowed_panic(msg: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "Error(Contract, #48)",
        "Error(Contract, #49)",
        "Error(Contract, #50)",
    ];
    ALLOWED.iter().any(|needle| msg.contains(needle))
}

fn assert_timelock_invariants(
    client: &NeuroWealthVaultClient<'_>,
    expected_active_agent: &Address,
    expected_pending: Option<(Address, u32)>,
    current_ledger: u32,
) {
    let active_agent = client.get_agent();
    assert_eq!(active_agent, *expected_active_agent);

    let pending = client.get_pending_agent_update();
    match (pending, expected_pending) {
        (Some((addr, expiry)), Some((expected_addr, expected_expiry))) => {
            assert_eq!(addr, expected_addr, "pending agent address mismatch");
            assert_eq!(expiry, expected_expiry, "pending expiry mismatch");
            assert!(
                expiry > current_ledger,
                "effective_ledger must be in the future"
            );
        }
        (None, None) => {}
        (Some((_, _)), None) => {
            panic!("contract reported pending state that should have been cleared")
        }
        (None, Some((_, _))) => panic!("contract lost the pending proposal"),
    }
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let (client, _owner, initial_agent) = setup(&env);

    let mut expected_active_agent = initial_agent;
    let mut expected_pending: Option<(Address, u32)> = None;

    for (step_idx, chunk) in data.chunks(4).enumerate() {
        if chunk.is_empty() {
            continue;
        }

        let op = chunk[0] % 4;
        let raw = u16::from(chunk.get(1).copied().unwrap_or(0))
            | (u16::from(chunk.get(2).copied().unwrap_or(0)) << 8);
        let agent_selector = chunk.get(3).copied().unwrap_or(0) as usize % 4;

        let proposal_agent = match agent_selector {
            0 => Address::generate(&env),
            1 => Address::generate(&env),
            2 => Address::generate(&env),
            _ => Address::generate(&env),
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match op {
            0 => {
                client.update_agent(&proposal_agent);
            }
            1 => {
                client.confirm_agent_update();
            }
            2 => {
                client.cancel_agent_update();
            }
            3 => {
                let advance_by = match raw % 4 {
                    0 => 1u32,
                    1 => AGENT_TIMELOCK_LEDGERS / 2,
                    2 => AGENT_TIMELOCK_LEDGERS,
                    _ => AGENT_TIMELOCK_LEDGERS + 1,
                };
                let next_sequence = env.ledger().sequence().saturating_add(advance_by);
                env.ledger().set_sequence_number(next_sequence);
            }
            _ => unreachable!(),
        }));

        match result {
            Ok(()) => {
                match op {
                    0 => {
                        let expected_expiry = env
                            .ledger()
                            .sequence()
                            .saturating_add(AGENT_TIMELOCK_LEDGERS);
                        expected_pending = Some((proposal_agent.clone(), expected_expiry));
                    }
                    1 => {
                        if let Some((pending_agent, expiry)) = expected_pending.as_ref() {
                            if env.ledger().sequence() >= *expiry {
                                expected_active_agent = pending_agent.clone();
                                expected_pending = None;
                            }
                        }
                    }
                    2 => {
                        expected_pending = None;
                    }
                    3 => {}
                    _ => unreachable!(),
                }

                assert_timelock_invariants(
                    &client,
                    &expected_active_agent,
                    expected_pending.clone(),
                    env.ledger().sequence(),
                );
            }
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                assert!(
                    is_allowed_panic(msg),
                    "unexpected panic at step {step_idx}: {msg}"
                );
            }
        }
    }
});
