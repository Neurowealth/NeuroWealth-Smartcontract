//! LibFuzzer harness: interleaved admin timelock state machine (upgrade + agent).
//!
//! Randomly sequences schedule/execute/cancel for BOTH the upgrade timelock
//! and the agent-update timelock, plus random ledger advances. This catches
//! cross-timelock state corruption — e.g., one timelock's pending state leaking
//! into the other, or shared storage keys being overwritten.
//!
//! Allowed panics (documented vault validation):
//! - `Error(Contract, #35)` — Paused
//! - `Error(Contract, #34)` — OnlyOwner
//! - `Error(Contract, #48)` — TimelockAlreadyPending
//! - `Error(Contract, #49)` — NoTimelockPending
//! - `Error(Contract, #47)` — TimelockNotElapsed (upgrade)
//! - `Error(Contract, #50)` — TimelockNotExpired (agent)

#![no_main]

use libfuzzer_sys::fuzz_target;
use neurowealth_vault::{NeuroWealthVault, NeuroWealthVaultClient};
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, BytesN, Env};

const DEFAULT_TIMELOCK_DELAY: u32 = 100;
const AGENT_TIMELOCK_LEDGERS: u32 = 17_280;

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

fn fake_hash(env: &Env, seed: u8) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    BytesN::from_array(env, &bytes)
}

fn is_allowed_panic(msg: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "Error(Contract, #35)", // Paused
        "Error(Contract, #34)", // OnlyOwner
        "Error(Contract, #48)", // TimelockAlreadyPending
        "Error(Contract, #49)", // NoTimelockPending
        "Error(Contract, #47)", // TimelockNotElapsed (upgrade)
        "Error(Contract, #50)", // TimelockNotExpired (agent)
    ];
    ALLOWED.iter().any(|needle| msg.contains(needle))
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let (client, owner, initial_agent) = setup(&env);

    // Upgrade timelock state
    let mut upgrade_pending_hash: Option<BytesN<32>> = None;
    let mut upgrade_pending_expiry: Option<u32> = None;

    // Agent timelock state
    let mut expected_active_agent = initial_agent;
    let mut expected_pending_agent: Option<(Address, u32)> = None;

    for (step_idx, chunk) in data.chunks(4).enumerate() {
        if chunk.is_empty() {
            continue;
        }

        let op = chunk[0] % 8;
        let param = chunk.get(1).copied().unwrap_or(0);
        let agent_selector = chunk.get(3).copied().unwrap_or(0) as usize % 4;

        let proposal_agent = match agent_selector {
            0 => Address::generate(&env),
            1 => Address::generate(&env),
            2 => Address::generate(&env),
            _ => Address::generate(&env),
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match op {
                // ── Upgrade timelock operations ──
                0 => {
                    // schedule_upgrade
                    client.schedule_upgrade(&owner, &fake_hash(&env, param));
                }
                1 => {
                    // execute_upgrade
                    let _ = client.try_execute_upgrade(&owner);
                }
                2 => {
                    // cancel_upgrade
                    client.cancel_upgrade(&owner);
                }

                // ── Agent timelock operations ──
                3 => {
                    // propose agent update
                    client.update_agent(&proposal_agent);
                }
                4 => {
                    // confirm agent update
                    let _ = client.try_confirm_agent_update();
                }
                5 => {
                    // cancel agent update
                    client.cancel_agent_update();
                }

                // ── Ledger control ──
                6 => {
                    let advance_by = ((param as u32) % 200) + 1;
                    let next_sequence = env.ledger().sequence().saturating_add(advance_by);
                    env.ledger().set_sequence_number(next_sequence);
                }
                7 => {
                    // Large jump — fast-forward past both timelocks
                    let advance_by = DEFAULT_TIMELOCK_DELAY.max(AGENT_TIMELOCK_LEDGERS) + 1;
                    let next_sequence = env.ledger().sequence().saturating_add(advance_by);
                    env.ledger().set_sequence_number(next_sequence);
                }
                _ => unreachable!(),
            }
        }));

        match result {
            Ok(()) => {
                // Update tracked state based on what succeeded
                match op {
                    // Upgrade timelock
                    0 => {
                        let expiry = env.ledger().sequence() + DEFAULT_TIMELOCK_DELAY;
                        upgrade_pending_hash = Some(fake_hash(&env, param));
                        upgrade_pending_expiry = Some(expiry);
                    }
                    1 => {
                        // execute_upgrade succeeded — clear pending
                        upgrade_pending_hash = None;
                        upgrade_pending_expiry = None;
                    }
                    2 => {
                        // cancel_upgrade succeeded — clear pending
                        upgrade_pending_hash = None;
                        upgrade_pending_expiry = None;
                    }

                    // Agent timelock
                    3 => {
                        let expected_expiry = env
                            .ledger()
                            .sequence()
                            .saturating_add(AGENT_TIMELOCK_LEDGERS);
                        expected_pending_agent = Some((proposal_agent.clone(), expected_expiry));
                    }
                    4 => {
                        if let Some((pending_agent, expiry)) = expected_pending_agent.as_ref() {
                            if env.ledger().sequence() >= *expiry {
                                expected_active_agent = pending_agent.clone();
                                expected_pending_agent = None;
                            }
                        }
                    }
                    5 => {
                        expected_pending_agent = None;
                    }
                    _ => {}
                }

                // ── Verify invariants ──
                let actual_upgrade = client.get_pending_upgrade();
                match (upgrade_pending_hash.clone(), upgrade_pending_expiry) {
                    (Some(_), Some(expiry)) => {
                        assert!(
                            actual_upgrade.is_some(),
                            "upgrade: expected pending at step {step_idx}"
                        );
                        let (_, e) = actual_upgrade.unwrap();
                        assert_eq!(e, expiry, "upgrade: expiry mismatch at step {step_idx}");
                    }
                    (None, None) => {
                        assert!(
                            actual_upgrade.is_none(),
                            "upgrade: expected no pending at step {step_idx}"
                        );
                    }
                    _ => unreachable!(),
                }

                let actual_agent = client.get_agent();
                assert_eq!(
                    actual_agent, expected_active_agent,
                    "agent: active agent mismatch at step {step_idx}"
                );
                let actual_pending_agent = client.get_pending_agent_update();
                match (&actual_pending_agent, &expected_pending_agent) {
                    (Some((addr, expiry)), Some((expected_addr, expected_expiry))) => {
                        assert_eq!(
                            addr, expected_addr,
                            "agent: pending address mismatch at step {step_idx}"
                        );
                        assert_eq!(
                            *expiry, *expected_expiry,
                            "agent: pending expiry mismatch at step {step_idx}"
                        );
                    }
                    (None, None) => {}
                    (Some(_), None) => {
                        panic!("agent: unexpected pending state at step {step_idx}");
                    }
                    (None, Some(_)) => {
                        panic!("agent: lost pending proposal at step {step_idx}");
                    }
                }
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
