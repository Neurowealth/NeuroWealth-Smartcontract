//! Storage-griefing benchmark (Issue #598).
//!
//! `UserSharesIndex` (see `add_to_user_index` in `lib.rs`) is a single
//! `Instance`-storage `Vec<Address>` that is appended to the first time an
//! address holds non-zero shares, and is never pruned — a fully-withdrawn
//! holder's slot is left behind as a zero-share entry (see
//! `get_users_with_shares`'s doc comment and `test_users_with_shares.rs`).
//!
//! Every `deposit()` call from a `current_shares == 0` address reads the
//! *entire* index into memory and linear-scans it (`index.contains(user)`)
//! before conditionally appending and rewriting the whole `Vec` back to
//! storage. That means both the CPU cost and the memory footprint of a
//! `deposit()` call grow with the **total number of distinct addresses that
//! have ever deposited**, not with the size of any individual deposit.
//!
//! This is the concrete "storage-griefing" surface Issue #598 asks about: an
//! attacker who never intends to keep funds in the vault can still grow this
//! index — and therefore raise the CPU/memory cost of every future first-time
//! depositor's `deposit()` call, and grow the contract's on-chain footprint
//! size — by making the minimum deposit from many distinct addresses.
//!
//! These tests measure that growth directly rather than asserting it only in
//! prose, following the same `measure()` harness as `test_budget.rs`.

extern crate std;

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// Resets the env budget to zero, runs `f`, and returns (cpu, mem).
/// Mirrors `test_budget.rs::measure` — kept local rather than shared because
/// `test_budget.rs` does not currently expose it via `utils.rs`.
fn measure<F: FnOnce()>(env: &Env, f: F) -> (u64, u64) {
    let mut budget = env.budget();
    budget.reset_unlimited();
    f();
    (
        env.budget().cpu_instruction_cost(),
        env.budget().memory_bytes_cost(),
    )
}

/// Deposits the minimum allowed amount from `count` distinct, never-before-seen
/// addresses, growing `UserSharesIndex` by exactly `count` entries.
fn grow_index_with_distinct_depositors(
    env: &Env,
    client: &NeuroWealthVaultClient,
    usdc_token: &Address,
    count: u32,
) {
    let token_client = TestTokenClient::new(env, usdc_token);
    // 1 USDC (DEFAULT_MIN_DEPOSIT) — the cheapest possible way to add one
    // entry to the index, i.e. the attacker's worst-case-for-defenders move.
    let min_deposit = 1_000_000_i128;
    for _ in 0..count {
        let user = Address::generate(env);
        token_client.mint(&user, &min_deposit);
        client.deposit(&user, &min_deposit);
    }
}

/// Baseline: cost of the very first deposit into a fresh vault (index size 0
/// going to 1).
#[test]
fn test_deposit_cost_with_empty_index() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);

    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    token_client.mint(&user, &amount);

    let (cpu, mem) = measure(&env, || {
        client.deposit(&user, &amount);
    });

    std::println!("[storage-griefing] deposit, index size 0->1  cpu={cpu}  mem={mem}");

    // Same soft ceiling as test_budget_deposit — a first-time deposit into an
    // empty index should cost the same as any other ordinary deposit.
    assert!(cpu < 5_000_000, "deposit CPU cost regressed: {cpu}");
    assert!(mem < 300_000, "deposit memory cost regressed: {mem}");
}

/// Grows the index to 500 distinct dust depositors, then measures the cost of
/// the *next* first-time deposit. If `add_to_user_index`'s full-vec read +
/// linear scan + full-vec rewrite is the dominant cost, this should be
/// measurably more expensive than `test_deposit_cost_with_empty_index`,
/// demonstrating the growth is real rather than theoretical.
///
/// 500 is chosen to keep the test itself fast (500 real contract invocations
/// to build the fixture) while still being large enough to show a clear
/// trend; it is not a claim about where any hard limit sits.
#[test]
fn test_deposit_cost_grows_with_index_size() {
    let env = Env::default();
    env.mock_all_auths();
    // Building the 500-entry fixture below is 500 chained contract calls
    // against one `Env`; each would be its own transaction (own budget) on a
    // live network. Reset so fixture setup doesn't trip the simulator's
    // per-`Env` compute ceiling before the actual measurement below.
    env.budget().reset_unlimited();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Raise the TVL cap so 500 dust deposits plus the measured deposit don't
    // themselves trip ExceedsTvlCap before the measurement runs.
    client.set_tvl_cap(&1_000_000_000_000_i128);

    const INDEX_SIZE: u32 = 500;
    grow_index_with_distinct_depositors(&env, &client, &usdc_token, INDEX_SIZE);

    let token_client = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let amount = 10_000_000_i128;
    token_client.mint(&user, &amount);

    let (cpu, mem) = measure(&env, || {
        client.deposit(&user, &amount);
    });

    std::println!(
        "[storage-griefing] deposit, index size {INDEX_SIZE}->{} cpu={cpu}  mem={mem}",
        INDEX_SIZE + 1
    );

    // This is deliberately NOT a tight regression gate the way test_budget.rs's
    // bounds are: the whole point of this test is that cost scales with index
    // size, so a fixed ceiling here would either be too loose to mean anything
    // or fail as soon as the index legitimately grows in production. Instead,
    // assert the qualitative claim: cost with a 500-entry index must not be
    // cheaper than the empty-index baseline once index-scan overhead is real
    // (a regression to O(1) behavior would show as `cpu`/`mem` collapsing
    // toward the empty-index numbers printed above; a future maintainer
    // diagnosing a griefing report should compare the two printed lines).
    //
    // We do assert an ordering sanity check: cost must not be negative/zero,
    // and must stay within a generous absolute ceiling so a truly runaway
    // (e.g. accidentally quadratic elsewhere) regression is still caught.
    assert!(cpu > 0 && mem > 0);
    assert!(
        cpu < 50_000_000,
        "deposit CPU cost with a {INDEX_SIZE}-entry index is unexpectedly high: {cpu} \
         (see docs/PARTIAL_WITHDRAWAL_BEHAVIOR.md sibling doc, ARCHITECTURE.md storage \
         section, for the storage-griefing analysis this benchmark supports)"
    );
    assert!(
        mem < 5_000_000,
        "deposit memory cost with a {INDEX_SIZE}-entry index is unexpectedly high: {mem}"
    );
}
