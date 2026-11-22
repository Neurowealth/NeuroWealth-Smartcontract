//! LibFuzzer harness: random upgrade timelock state machine transitions.
//!
//! Exercises schedule_upgrade/execute_upgrade/cancel_upgrade interleaved with
//! random ledger advances to catch state-inconsistency bugs in the upgrade
//! timelock logic. This is the highest-stakes admin action as it controls
//! contract WASM replacement.
//!
//! Allowed panics (documented vault validation):
//! - `Error(Contract, #35)` — Paused
//! - `Error(Contract, #34)` — OnlyOwner
//! - `Error(Contract, #48)` — TimelockAlreadyPending
//! - `Error(Contract, #49)` — NoTimelockPending
//! - `Error(Contract, #47)` — TimelockNotElapsed

#![no_main]

use libfuzzer_sys::fuzz_target;
use neurowealth_vault::{NeuroWealthVault, NeuroWealthVaultClient};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, BytesN, Env};

const DEFAULT_TIMELOCK_DELAY: u32 = 100;

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
    let usdc = Address::generate(env);

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
        "Error(Contract, #47)", // TimelockNotElapsed
    ];
    ALLOWED.iter().any(|needle| msg.contains(needle))
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let (client, owner, _agent) = setup(&env);

    // Track the expected state for invariant checking
    let mut pending_hash: Option<BytesN<32>> = None;
    let mut pending_expiry: Option<u32> = None;

    for (i, chunk) in data.chunks(2).enumerate() {
        if chunk.is_empty() {
            continue;
        }

        let op = chunk[0] % 4;
        let param = chunk.get(1).copied().unwrap_or(0);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match op {
                0 => {
                    // schedule_upgrade
                    if pending_hash.is_some() {
                        // Should fail with TimelockAlreadyPending
                        let _ = client.try_schedule_upgrade(&owner, &fake_hash(&env, param));
                        return;
                    }
                    client.schedule_upgrade(&owner, &fake_hash(&env, param));
                    let sequence = env.ledger().sequence();
                    pending_hash = Some(fake_hash(&env, param));
                    pending_expiry = Some(sequence + DEFAULT_TIMELOCK_DELAY);
                }
                1 => {
                    // execute_upgrade
                    if pending_hash.is_none() {
                        // Should fail with NoTimelockPending
                        let _ = client.try_execute_upgrade(&owner);
                        return;
                    }
                    let expiry = pending_expiry.unwrap();
                    let current = env.ledger().sequence();
                    if current < expiry {
                        // Should fail with TimelockNotElapsed
                        let _ = client.try_execute_upgrade(&owner);
                    } else {
                        // Should succeed - but will trap on fake hash
                        // We catch this as an expected outcome
                        let _ = client.try_execute_upgrade(&owner);
                        pending_hash = None;
                        pending_expiry = None;
                    }
                }
                2 => {
                    // cancel_upgrade
                    if pending_hash.is_none() {
                        // Should fail with NoTimelockPending
                        let _ = client.try_cancel_upgrade(&owner);
                        return;
                    }
                    client.cancel_upgrade(&owner);
                    pending_hash = None;
                    pending_expiry = None;
                }
                3 => {
                    // advance ledger
                    let advance = (param as u32) % 200;
                    let current = env.ledger().sequence();
                    env.ledger().set_sequence_number(current + advance);
                }
                _ => unreachable!(),
            }
        }));

        match result {
            Ok(()) => {
                // Verify invariants after successful operation
                let actual_pending = client.get_pending_upgrade();
                match (pending_hash.clone(), pending_expiry) {
                    (Some(hash), Some(expiry)) => {
                        assert!(
                            actual_pending.is_some(),
                            "pending state mismatch at step {}: expected Some, got None",
                            i
                        );
                        let (actual_hash, actual_expiry) = actual_pending.unwrap();
                        assert_eq!(actual_hash, hash, "pending hash mismatch at step {}", i);
                        assert_eq!(
                            actual_expiry, expiry,
                            "pending expiry mismatch at step {}",
                            i
                        );
                    }
                    (None, None) => {
                        assert!(
                            actual_pending.is_none(),
                            "pending state mismatch at step {}: expected None, got Some",
                            i
                        );
                    }
                    _ => unreachable!(),
                }
            }
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
