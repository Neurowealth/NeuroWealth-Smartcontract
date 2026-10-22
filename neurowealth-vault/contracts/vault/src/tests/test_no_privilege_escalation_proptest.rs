//! Property test (Issue #597): from the initial role assignment, the agent
//! address can never become owner, never modify pending upgrades, and never
//! change caps/pools/TTLs - regardless of call ordering.
//!
//! `test_adversarial_agent_simulation.rs` (#596) already proves this for
//! every owner-gated entrypoint called *individually*, in isolation. What it
//! does not cover is arbitrary *sequences*: this property test generates a
//! random-length, randomly-ordered sequence drawn from that same set of
//! owner-gated calls (with the agent as the sole, correctly-scoped signer -
//! see [`mock_agent_only_auth`]) and asserts, after **every single call in
//! the sequence**, that:
//!   1. the call itself was rejected, and
//!   2. no privileged singleton field changed (owner, pending_owner, agent,
//!      pending_agent_update, pending_upgrade, tvl_cap, user_deposit_cap,
//!      min/max_deposit, blend/dex_pool, approval_ttl,
//!      max_consecutive_failures, rebalance_cooldown).
//!
//! Because each individual call is already proven side-effect-free in
//! isolation (#596), the property here is really that *composition* holds:
//! there is no ordering or accumulation effect (e.g. a rejected call
//! secretly writing partial state that a later call then exploits) that
//! could let the agent escalate. Running this over many random orderings is
//! how that absence-of-interaction-effect claim gets tested rather than
//! assumed.
//!
//! Kept to a modest case count (see `ProptestConfig` below): each case boots
//! a fresh `Env` and contract and can run up to
//! [`OWNER_GATED_CALLS`]`.len()` real contract invocations, so the default
//! 256 cases would be needlessly slow for a property with no numeric
//! search space to speak of (the call table is fixed and small).

extern crate std;

use super::utils::*;
use proptest::prelude::*;
use proptest::sample::subsequence;
use soroban_sdk::{testutils::Address as _, Address, BytesN, Env, IntoVal};

/// One owner-gated entrypoint, attempted as the agent with correctly scoped
/// (agent-only) auth. Returns `true` if the call was accepted (which would
/// itself be the bug this property is designed to catch).
///
/// Every closure mirrors the call convention already established per-function
/// in `test_adversarial_agent_simulation.rs` (Group 2: implicit owner check).
/// `pause`/`unpause`/`emergency_pause`/`set_blend_pool`/`set_dex_pool` take an
/// explicit identity parameter (Group 1 there) and are covered by that
/// existing per-call suite instead of here, since scoping their auth to
/// "agent-only" is equivalent to just calling them as the agent directly.
type OwnerGatedCall = fn(&Env, &Address, &Address) -> bool;

fn call_set_tvl_cap(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let cap = 50_000_000_000_i128;
    mock_agent_only_auth(env, contract_id, agent, "set_tvl_cap", (cap,).into_val(env));
    let accepted = client.try_set_tvl_cap(&cap).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_set_user_deposit_cap(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let cap = 25_000_000_000_i128;
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "set_user_deposit_cap",
        (cap,).into_val(env),
    );
    let accepted = client.try_set_user_deposit_cap(&cap).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_set_deposit_limits(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let (min, max) = (2_000_000_i128, 50_000_000_000_i128);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "set_deposit_limits",
        (min, max).into_val(env),
    );
    let accepted = client.try_set_deposit_limits(&min, &max).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_set_rebalance_cooldown(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let interval = 720_u32;
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "set_rebalance_cooldown",
        (interval,).into_val(env),
    );
    let accepted = client.try_set_rebalance_cooldown(&interval).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_set_max_consecutive_failures(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let threshold = 5_u32;
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "set_max_consecutive_failures",
        (threshold,).into_val(env),
    );
    let accepted = client.try_set_max_consecutive_failures(&threshold).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_set_approval_ttl(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let ttl = 2_000_u32;
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "set_approval_ttl",
        (ttl,).into_val(env),
    );
    let accepted = client.try_set_approval_ttl(&ttl).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_update_agent(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let new_agent = Address::generate(env);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "update_agent",
        (new_agent.clone(),).into_val(env),
    );
    let accepted = client.try_update_agent(&new_agent).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_confirm_agent_update(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "confirm_agent_update",
        ().into_val(env),
    );
    let accepted = client.try_confirm_agent_update().is_ok();
    env.mock_all_auths();
    accepted
}

fn call_cancel_agent_update(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "cancel_agent_update",
        ().into_val(env),
    );
    let accepted = client.try_cancel_agent_update().is_ok();
    env.mock_all_auths();
    accepted
}

fn call_transfer_ownership(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let new_owner = Address::generate(env);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "transfer_ownership",
        (new_owner.clone(),).into_val(env),
    );
    let accepted = client.try_transfer_ownership(&new_owner).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_cancel_ownership_transfer(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "cancel_ownership_transfer",
        ().into_val(env),
    );
    let accepted = client.try_cancel_ownership_transfer().is_ok();
    env.mock_all_auths();
    accepted
}

fn call_schedule_upgrade(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    let fake_hash = BytesN::from_array(env, &[7u8; 32]);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "schedule_upgrade",
        (agent.clone(), fake_hash.clone()).into_val(env),
    );
    let accepted = client
        .try_schedule_upgrade(&agent.clone(), &fake_hash)
        .is_ok();
    env.mock_all_auths();
    accepted
}

fn call_execute_upgrade(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "execute_upgrade",
        (agent.clone(),).into_val(env),
    );
    let accepted = client.try_execute_upgrade(&agent.clone()).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_cancel_upgrade(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "cancel_upgrade",
        (agent.clone(),).into_val(env),
    );
    let accepted = client.try_cancel_upgrade(&agent.clone()).is_ok();
    env.mock_all_auths();
    accepted
}

fn call_emergency_harvest(env: &Env, contract_id: &Address, agent: &Address) -> bool {
    let client = NeuroWealthVaultClient::new(env, contract_id);
    mock_agent_only_auth(
        env,
        contract_id,
        agent,
        "emergency_harvest",
        (0_i128,).into_val(env),
    );
    let accepted = client.try_emergency_harvest(&0_i128).is_ok();
    env.mock_all_auths();
    accepted
}

/// The full table of owner-gated calls exercised by this property. Kept as a
/// `const` array of function pointers (not closures) so `proptest` can
/// select indices into it cheaply.
const OWNER_GATED_CALLS: &[OwnerGatedCall] = &[
    call_set_tvl_cap,
    call_set_user_deposit_cap,
    call_set_deposit_limits,
    call_set_rebalance_cooldown,
    call_set_max_consecutive_failures,
    call_set_approval_ttl,
    call_update_agent,
    call_confirm_agent_update,
    call_cancel_agent_update,
    call_transfer_ownership,
    call_cancel_ownership_transfer,
    call_schedule_upgrade,
    call_execute_upgrade,
    call_cancel_upgrade,
    call_emergency_harvest,
];

/// Comparable snapshot of the privileged singleton fields this property
/// cares about. A strict subset of `VaultStateSnapshot`'s fields
/// (deliberately excludes `total_assets`/`total_shares`/`current_protocol`/
/// `last_rebalance_ledger`/`consecutive_failures`/`paused`, which are
/// legitimately mutable by the agent's allowed set per #596 and are out of
/// scope for *this* property).
///
/// A plain struct rather than a tuple: `core` only implements `PartialEq`/
/// `Debug` for tuples up to 12 elements, and this snapshot needs 14 fields.
#[derive(Clone, Debug, PartialEq)]
struct PrivilegedFields {
    owner: Address,
    agent: Address,
    pending_owner: Option<Address>,
    pending_agent_update: Option<(Address, u32)>,
    pending_upgrade: Option<(BytesN<32>, u32)>,
    tvl_cap: i128,
    user_deposit_cap: i128,
    min_deposit: i128,
    max_deposit: i128,
    blend_pool: Option<Address>,
    dex_pool: Option<Address>,
    approval_ttl: u32,
    max_consecutive_failures: u32,
    rebalance_cooldown: u32,
}

fn privileged_fields(snap: &VaultStateSnapshot) -> PrivilegedFields {
    PrivilegedFields {
        owner: snap.owner.clone(),
        agent: snap.agent.clone(),
        pending_owner: snap.pending_owner.clone(),
        pending_agent_update: snap.pending_agent_update.clone(),
        pending_upgrade: snap.pending_upgrade.clone(),
        tvl_cap: snap.tvl_cap,
        user_deposit_cap: snap.user_deposit_cap,
        min_deposit: snap.min_deposit,
        max_deposit: snap.max_deposit,
        blend_pool: snap.blend_pool.clone(),
        dex_pool: snap.dex_pool.clone(),
        approval_ttl: snap.approval_ttl,
        max_consecutive_failures: snap.max_consecutive_failures,
        rebalance_cooldown: snap.rebalance_cooldown,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// For any random-length, randomly-ordered sequence of owner-gated calls
    /// attempted by a compromised agent (correctly-scoped auth: only the
    /// agent's signature, never the owner's), every single call is rejected
    /// and privileged state never moves from its initial (owner-assigned)
    /// values - regardless of which calls are attempted or in what order.
    #[test]
    fn prop_agent_cannot_escalate_regardless_of_call_order(
        indices in subsequence(
            (0..OWNER_GATED_CALLS.len()).collect::<std::vec::Vec<_>>(),
            0..=OWNER_GATED_CALLS.len(),
        ),
        // A second, independent draw of indices (with repeats and its own
        // order) layered on top, so the property also covers repeated calls
        // and orderings `subsequence` alone (which preserves relative order
        // of a fixed set) cannot produce.
        repeats in proptest::collection::vec(0..OWNER_GATED_CALLS.len(), 0..=OWNER_GATED_CALLS.len()),
    ) {
        let env = Env::default();
        env.mock_all_auths();
        // Each case can chain up to 2 * OWNER_GATED_CALLS.len() real contract
        // invocations against one `Env`. On a live network each of those
        // would be its own transaction with its own fresh budget; here they
        // share a single `Env`'s budget, which is sized for one call. Reset
        // it to unlimited so the property is checked purely on auth/state
        // correctness rather than tripping the simulator's per-`Env` compute
        // ceiling (same pattern `test_budget.rs` uses for its own multi-call
        // measurements).
        env.budget().reset_unlimited();
        let (contract_id, agent, _owner, _usdc_token) = setup_vault_with_token(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);

        let initial = snapshot_vault_state(&client, &[agent.clone()]);
        let initial_key = privileged_fields(&initial);

        // Interleave the two index sources into one call sequence so both
        // "which subset, in table order" and "arbitrary repeats/order" are
        // exercised by a single sequence per case.
        let mut sequence: std::vec::Vec<usize> = std::vec::Vec::new();
        sequence.extend(indices);
        sequence.extend(repeats);

        for idx in sequence {
            let call = OWNER_GATED_CALLS[idx];
            let accepted = call(&env, &contract_id, &agent);

            prop_assert!(
                !accepted,
                "owner-gated call at table index {} was accepted from a compromised agent",
                idx
            );

            let after = snapshot_vault_state(&client, &[agent.clone()]);
            prop_assert_eq!(
                privileged_fields(&after),
                initial_key.clone(),
                "privileged state mutated after owner-gated call at table index {} \
                 (agent must never affect owner/pending-owner/pending-upgrade/caps/pools/TTLs)",
                idx
            );
        }

        // Composition check: after the *entire* sequence, the agent is still
        // not the owner and no ownership transfer/agent-rotation/upgrade is
        // pending - the concrete escalation outcomes #597 calls out.
        let final_state = snapshot_vault_state(&client, &[agent.clone()]);
        prop_assert_ne!(final_state.owner, agent, "agent must never become owner");
        prop_assert_eq!(final_state.pending_owner, None::<Address>);
        prop_assert_eq!(final_state.pending_agent_update, None::<(Address, u32)>);
        prop_assert_eq!(final_state.pending_upgrade, None::<(BytesN<32>, u32)>);
    }
}
