//! End-to-end accounting-invariant tests across a full protocol-rotation
//! lifecycle (Issue #843).
//!
//! Covers the sequence demanded by the issue:
//! `deposit → deployment → yield → partial exit → rotation → final withdrawal`,
//! snapshotting the vault's public accounting surface before and after every
//! transition.
//!
//! Invariants asserted at each checkpoint:
//!
//! 1. `idle + deployed == total_assets` within [`ROUNDING_TOLERANCE`] raw units
//!    (7-decimal USDC). `TotalAssets` only moves when the agent reports it via
//!    `update_total_assets` or when shares are minted/burned, so after every
//!    committed transition the *reported* total must line up with the live
//!    position.
//! 2. Summed user shares never exceed the contract's issued-share total
//!    (`get_total_shares`); in this suite they must equal it exactly.
//! 3. A failed rotation — external protocol rejects the supply, or an exit
//!    leg under-fills below the caller's `min_out` — rolls the whole
//!    invocation back: allocation, per-venue balances, share accounting and
//!    principal are unchanged and no event leaks.
//! 4. The lifecycle emits a deterministic topic sequence.
//!
//! "Rotation" here is the cross-venue rotation the rebalance path fully
//! supports (`blend → dex → none`). Rotating a *pool address* while funds are
//! deployed is intentionally out of scope: a same-symbol rebalance is a
//! no-op that strands the old position (pinned by
//! `test_dex_integration::test_set_dex_pool_rotation_while_deployed_strands_funds`).

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env, Symbol, TryFromVal};

/// Maximum accepted drift between the live position (`idle + deployed`) and the
/// reported `TotalAssets`, in raw 7-decimal USDC units.
///
/// Share minting floors and share burning ceils, so a single raw unit is the
/// documented worst case elsewhere in this suite
/// (`test_balance_shares_invariant`). The lifecycle below is expected to
/// reconcile exactly; the tolerance only guards the boundary.
const ROUNDING_TOLERANCE: i128 = 1;

const DEPOSIT_ALICE: i128 = 100_000_000;
const DEPOSIT_BOB: i128 = 50_000_000;
const DEPOSIT_TOTAL: i128 = DEPOSIT_ALICE + DEPOSIT_BOB;
const YIELD: i128 = 6_000_000;
const PARTIAL_EXIT: i128 = 10_000_000;

/// Public accounting surface snapshotted before/after each transition.
#[derive(Clone, Debug, PartialEq)]
struct AccountingSnapshot {
    total_assets: i128,
    total_shares: i128,
    total_deposits: i128,
    idle: i128,
    deployed: i128,
    current_protocol: Symbol,
    user_shares: i128,
    user_balance: i128,
}

fn abs_diff(a: i128, b: i128) -> i128 {
    if a >= b {
        a - b
    } else {
        b - a
    }
}

fn snapshot(client: &NeuroWealthVaultClient, user: &Address) -> AccountingSnapshot {
    let (idle, deployed) = client.get_asset_breakdown();
    AccountingSnapshot {
        total_assets: client.get_total_assets(),
        total_shares: client.get_total_shares(),
        total_deposits: client.get_total_deposits(),
        idle,
        deployed,
        current_protocol: client.get_current_protocol(),
        user_shares: client.get_shares(user),
        user_balance: client.get_balance(user),
    }
}

/// Asserts every public accounting invariant the issue calls out.
fn assert_accounting_invariants(
    client: &NeuroWealthVaultClient,
    users: &[Address],
    checkpoint: &str,
) {
    let total_assets = client.get_total_assets();
    let total_shares = client.get_total_shares();
    let (idle, deployed) = client.get_asset_breakdown();

    assert!(idle >= 0, "{checkpoint}: idle balance must never be negative");
    assert!(
        deployed >= 0,
        "{checkpoint}: deployed balance must never be negative"
    );

    // (1) reconciliation, within the documented rounding tolerance.
    let drift = abs_diff(idle + deployed, total_assets);
    assert!(
        drift <= ROUNDING_TOLERANCE,
        "{checkpoint}: idle({idle}) + deployed({deployed}) must reconcile with \
         total_assets({total_assets}); drift {drift} exceeds tolerance {ROUNDING_TOLERANCE}"
    );

    // (2) per-user and summed share bounds.
    let mut sum_user_shares = 0_i128;
    for user in users.iter() {
        let shares = client.get_shares(user);
        let balance = client.get_balance(user);
        assert!(shares >= 0, "{checkpoint}: user shares must never be negative");
        assert!(
            balance >= 0,
            "{checkpoint}: user balance must never be negative"
        );
        assert!(
            shares <= total_shares,
            "{checkpoint}: user shares ({shares}) must not exceed issued shares ({total_shares})"
        );
        assert!(
            balance <= total_assets,
            "{checkpoint}: user balance ({balance}) must not exceed total assets ({total_assets})"
        );
        sum_user_shares += shares;
    }
    assert!(
        sum_user_shares <= total_shares,
        "{checkpoint}: summed user shares ({sum_user_shares}) must not exceed issued shares ({total_shares})"
    );
    assert_eq!(
        sum_user_shares, total_shares,
        "{checkpoint}: summed user shares must equal the issued-share total"
    );

    // Principal is a reporting-only, non-yield counter. In this gain-only
    // lifecycle it must never climb above the assets actually backing shares.
    assert!(
        client.get_total_deposits() <= total_assets,
        "{checkpoint}: principal must not exceed total assets in a no-loss lifecycle"
    );
}

/// First topic symbol of every emitted event, in emission order.
///
/// Addresses are deliberately excluded: they differ between test `Env`s, while
/// the topic symbols are the deterministic, indexable part of the event stream.
fn event_topic_fingerprint(env: &Env) -> soroban_sdk::Vec<Symbol> {
    let events = env.events().all();
    let mut fingerprint = soroban_sdk::Vec::new(env);
    for i in 0..events.len() {
        if let Some((_contract, topics, _data)) = events.get(i) {
            if let Some(first) = topics.get(0) {
                if let Ok(topic) = Symbol::try_from_val(env, &first) {
                    fingerprint.push_back(topic);
                }
            }
        }
    }
    fingerprint
}

fn assert_same_event_fingerprint(
    a: &soroban_sdk::Vec<Symbol>,
    b: &soroban_sdk::Vec<Symbol>,
) {
    assert_eq!(a.len(), b.len(), "event stream length must be deterministic");
    for i in 0..a.len() {
        assert_eq!(
            a.get(i).unwrap(),
            b.get(i).unwrap(),
            "event topic mismatch at index {i}"
        );
    }
}

fn fingerprint_contains(topics: &soroban_sdk::Vec<Symbol>, needle: Symbol) -> bool {
    for i in 0..topics.len() {
        if topics.get(i).unwrap() == needle {
            return true;
        }
    }
    false
}

/// Registers a vault with both a Blend and a DEX pool configured.
///
/// Returns `(vault_id, owner, agent, usdc_token, blend_pool, dex_pool)`.
fn setup_multi_venue(env: &Env) -> (Address, Address, Address, Address, Address, Address) {
    let (vault_id, agent, owner, usdc_token) = setup_vault_with_token(env);
    let blend_pool = env.register_contract(None, MockBlendPool);
    let dex_pool = env.register_contract(None, MockDexPool);

    let client = NeuroWealthVaultClient::new(env, &vault_id);
    client.set_blend_pool(&owner, &blend_pool);
    client.set_dex_pool(&owner, &dex_pool);

    (vault_id, owner, agent, usdc_token, blend_pool, dex_pool)
}

/// Deposits `amount` from a fresh user and deploys the whole position to Blend.
///
/// Returns `(vault_id, owner, agent, usdc_token, blend_pool, dex_pool, user)`.
fn setup_deployed_blend(
    env: &Env,
    amount: i128,
) -> (Address, Address, Address, Address, Address, Address, Address) {
    let (vault_id, owner, agent, usdc_token, blend_pool, dex_pool) = setup_multi_venue(env);
    let client = NeuroWealthVaultClient::new(env, &vault_id);

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, amount);
    client.rebalance(&symbol_short!("blend"), &850_i128, &0_i128);

    (
        vault_id, owner, agent, usdc_token, blend_pool, dex_pool, user,
    )
}

// ===========================================================================
// Full lifecycle: deposit → deployment → yield → partial exit → rotation →
// final withdrawal, with an invariant checkpoint after every transition.
// ===========================================================================

/// Runs the complete lifecycle and returns the deterministic event-topic
/// fingerprint. All per-transition assertions live inside, so this is also
/// the single source of truth for both the invariant test and the
/// determinism test.
fn run_full_lifecycle(env: &Env) -> soroban_sdk::Vec<Symbol> {
    env.mock_all_auths();

    let (vault_id, _owner, agent, usdc, blend_pool, dex_pool) = setup_multi_venue(env);
    let client = NeuroWealthVaultClient::new(env, &vault_id);
    let token = TestTokenClient::new(env, &usdc);
    let blend = MockBlendPoolClient::new(env, &blend_pool);
    let dex = MockDexPoolClient::new(env, &dex_pool);

    let alice = Address::generate(env);
    let bob = Address::generate(env);
    let users = [alice.clone(), bob.clone()];

    // ── 1. Deposit ────────────────────────────────────────────────────────
    mint_and_deposit(env, &client, &usdc, &alice, DEPOSIT_ALICE);
    mint_and_deposit(env, &client, &usdc, &bob, DEPOSIT_BOB);

    let after_deposit = snapshot(&client, &alice);
    assert_eq!(after_deposit.total_assets, DEPOSIT_TOTAL);
    assert_eq!(after_deposit.total_shares, DEPOSIT_TOTAL);
    assert_eq!(after_deposit.total_deposits, DEPOSIT_TOTAL);
    assert_eq!(after_deposit.idle, DEPOSIT_TOTAL);
    assert_eq!(after_deposit.deployed, 0);
    assert_eq!(after_deposit.user_shares, DEPOSIT_ALICE);
    assert_eq!(after_deposit.current_protocol, symbol_short!("none"));
    assert_accounting_invariants(&client, &users, "after deposit");

    // ── 2. Deployment (rotate funds into Blend) ───────────────────────────
    client.rebalance(&symbol_short!("blend"), &850_i128, &0_i128);

    let after_deploy = snapshot(&client, &alice);
    assert_eq!(after_deploy.current_protocol, symbol_short!("blend"));
    assert_eq!(after_deploy.idle, 0);
    assert_eq!(after_deploy.deployed, DEPOSIT_TOTAL);
    assert_eq!(blend.supplied(&usdc), DEPOSIT_TOTAL);
    // Deployment moves funds; it must not change accounting.
    assert_eq!(after_deploy.total_assets, after_deposit.total_assets);
    assert_eq!(after_deploy.total_shares, after_deposit.total_shares);
    assert_eq!(after_deploy.total_deposits, after_deposit.total_deposits);
    assert_accounting_invariants(&client, &users, "after deployment");

    // ── 3. Yield ──────────────────────────────────────────────────────────
    token.mint(&blend_pool, &YIELD);
    let reported_with_yield = DEPOSIT_TOTAL + YIELD;
    client.update_total_assets(&agent, &reported_with_yield, &false, &0_u32);

    let after_yield = snapshot(&client, &alice);
    assert_eq!(after_yield.total_assets, reported_with_yield);
    assert_eq!(after_yield.deployed, reported_with_yield);
    assert_eq!(after_yield.idle, 0);
    // Yield must not mint shares and must not move principal.
    assert_eq!(after_yield.total_shares, after_deploy.total_shares);
    assert_eq!(after_yield.total_deposits, after_deploy.total_deposits);
    assert_accounting_invariants(&client, &users, "after yield");

    // ── 4. Partial exit ───────────────────────────────────────────────────
    client.withdraw(&alice, &PARTIAL_EXIT);

    let after_partial = snapshot(&client, &alice);
    assert_eq!(after_partial.total_assets, reported_with_yield - PARTIAL_EXIT);
    assert_eq!(after_partial.deployed, reported_with_yield - PARTIAL_EXIT);
    assert_eq!(after_partial.idle, 0);
    assert!(
        after_partial.total_shares < after_yield.total_shares,
        "partial exit must burn shares"
    );
    assert!(
        after_partial.total_deposits < after_yield.total_deposits,
        "partial exit must reduce principal"
    );
    assert!(
        after_partial.user_balance < after_yield.user_balance,
        "the exiting user's claim must shrink"
    );
    assert_eq!(blend.supplied(&usdc), reported_with_yield - PARTIAL_EXIT);
    assert_accounting_invariants(&client, &users, "after partial exit");

    // ── 5. Rotation: Blend → DEX ──────────────────────────────────────────
    client.rebalance(&symbol_short!("dex"), &900_i128, &0_i128);

    let after_rotation = snapshot(&client, &alice);
    assert_eq!(after_rotation.current_protocol, symbol_short!("dex"));
    assert_eq!(blend.supplied(&usdc), 0, "rotation must fully exit the old venue");
    assert_eq!(dex.balance(&usdc, &vault_id), after_partial.deployed);
    assert_eq!(after_rotation.idle, 0);
    assert_eq!(after_rotation.deployed, after_partial.deployed);
    assert_eq!(after_rotation.total_assets, after_partial.total_assets);
    assert_eq!(after_rotation.total_shares, after_partial.total_shares);
    assert_eq!(after_rotation.total_deposits, after_partial.total_deposits);
    assert_eq!(after_rotation.user_shares, after_partial.user_shares);
    assert_eq!(after_rotation.user_balance, after_partial.user_balance);
    assert_accounting_invariants(&client, &users, "after blend->dex rotation");

    // ── 6. Rotation: DEX → idle ───────────────────────────────────────────
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    let after_exit = snapshot(&client, &alice);
    assert_eq!(after_exit.current_protocol, symbol_short!("none"));
    assert_eq!(after_exit.deployed, 0);
    assert_eq!(after_exit.idle, after_rotation.deployed);
    assert_eq!(after_exit.total_assets, after_rotation.total_assets);
    assert_eq!(after_exit.total_shares, after_rotation.total_shares);
    assert_eq!(after_exit.total_deposits, after_rotation.total_deposits);
    assert_accounting_invariants(&client, &users, "after dex->idle rotation");

    // ── 7. Final withdrawal ───────────────────────────────────────────────
    client.withdraw_all(&alice);
    client.withdraw_all(&bob);

    let after_final = snapshot(&client, &alice);
    assert_eq!(after_final.total_assets, 0);
    assert_eq!(after_final.total_shares, 0);
    assert_eq!(after_final.total_deposits, 0);
    assert_eq!(after_final.idle, 0);
    assert_eq!(after_final.deployed, 0);
    assert_eq!(client.get_shares(&bob), 0);
    assert_eq!(client.get_balance(&bob), 0);
    assert_eq!(dex.balance(&usdc, &vault_id), 0);
    assert_accounting_invariants(&client, &users, "after final withdrawal");

    event_topic_fingerprint(env)
}

#[test]
fn test_full_rotation_lifecycle_preserves_accounting_invariants() {
    let env = Env::default();
    let fingerprint = run_full_lifecycle(&env);

    assert!(!fingerprint.is_empty(), "lifecycle must emit events");
    for topic in [
        symbol_short!("deposit"),
        symbol_short!("blend_sup"),
        symbol_short!("assets"),
        symbol_short!("withdraw"),
        symbol_short!("dex_sup"),
        symbol_short!("dex_wd"),
        symbol_short!("rebalance"),
    ] {
        assert!(
            fingerprint_contains(&fingerprint, topic),
            "lifecycle must emit the {topic:?} topic"
        );
    }
}

#[test]
fn test_rotation_lifecycle_events_are_deterministic() {
    let env_first = Env::default();
    let first = run_full_lifecycle(&env_first);

    let env_second = Env::default();
    let second = run_full_lifecycle(&env_second);

    assert_same_event_fingerprint(&first, &second);
}

// ===========================================================================
// Failure injection: expected external-protocol failures must roll the whole
// rotation back, leaving allocation and user accounting untouched.
// ===========================================================================

/// Entry leg fails: the DEX refuses the entire supply, so the realized amount
/// is 0 and `min_out = 1` trips `MinOutNotMet`. The Blend exit that already ran
/// in the same invocation must be rolled back with it.
#[test]
fn test_failed_rotation_entry_leg_rolls_back_all_accounting() {
    let env = Env::default();
    env.mock_all_auths();

    let amount = 100_000_000_i128;
    let (vault_id, _owner, _agent, usdc, blend_pool, dex_pool, user) =
        setup_deployed_blend(&env, amount);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let blend = MockBlendPoolClient::new(&env, &blend_pool);
    let dex = MockDexPoolClient::new(&env, &dex_pool);

    let before = snapshot(&client, &user);
    let events_before = event_topic_fingerprint(&env);
    assert_eq!(before.current_protocol, symbol_short!("blend"));
    assert_eq!(blend.supplied(&usdc), amount);

    // Negative supply limit makes the mock reject the whole supply.
    dex.set_max_supply_limit(&-1_i128);
    let result = client.try_rebalance(&symbol_short!("dex"), &850_i128, &1_i128);
    assert!(result.is_err(), "a blocked DEX entry must fail the rotation");

    assert_eq!(
        snapshot(&client, &user),
        before,
        "failed entry-leg rotation must leave all accounting untouched"
    );
    assert_eq!(
        blend.supplied(&usdc),
        amount,
        "the reverted Blend exit must leave the original position intact"
    );
    assert_eq!(dex.balance(&usdc, &vault_id), 0, "DEX must hold nothing");
    assert_same_event_fingerprint(&event_topic_fingerprint(&env), &events_before);
}

/// Exit leg fails: the source pool under-fills the exit below the caller's
/// `min_out`, so a withdrawal that a later protocol call depends on cannot
/// complete and the rotation reverts.
#[test]
fn test_failed_rotation_exit_leg_rolls_back_all_accounting() {
    let env = Env::default();
    env.mock_all_auths();

    let amount = 100_000_000_i128;
    let (vault_id, _owner, _agent, usdc, blend_pool, dex_pool, user) =
        setup_deployed_blend(&env, amount);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let blend = MockBlendPoolClient::new(&env, &blend_pool);
    let dex = MockDexPoolClient::new(&env, &dex_pool);

    let before = snapshot(&client, &user);
    let events_before = event_topic_fingerprint(&env);
    assert_eq!(before.current_protocol, symbol_short!("blend"));
    assert_eq!(blend.supplied(&usdc), amount);

    // The Blend bug/illiquidity scenario: only 40 USDC of a 100 USDC exit can
    // be realized, below the caller's 50 USDC floor.
    blend.set_max_withdraw_limit(&40_000_000_i128);
    let result = client.try_rebalance(&symbol_short!("dex"), &850_i128, &50_000_000_i128);
    assert!(
        result.is_err(),
        "an under-filled exit below min_out must fail the rotation"
    );

    assert_eq!(
        snapshot(&client, &user),
        before,
        "failed exit-leg rotation must leave all accounting untouched"
    );
    assert_eq!(
        blend.supplied(&usdc),
        amount,
        "a failed exit must not partially drain the source venue"
    );
    assert_eq!(dex.balance(&usdc, &vault_id), 0, "DEX must hold nothing");
    assert_same_event_fingerprint(&event_topic_fingerprint(&env), &events_before);
}
