//! LibFuzzer harness: tests share-accounting invariants.
//!
//! This target verifies that the vault's share-based accounting maintains
//! mathematical invariants across deposit/withdraw/asset-update sequences,
//! extended (#987) to the newer flows where several accounting bugs in this
//! batch live: `lock_shares`, `unlock_shares`, `migrate_shares`,
//! `emergency_withdraw`, `queue_withdrawal`/`process_withdrawal_queue`, and
//! `batch_deposit`.
//!
//! Invariants checked after every step:
//! 1. sum(user_shares) == total_shares
//! 2. user_balance(user) == user_shares(user) * total_assets / total_shares
//! 3. total_assets >= total_deposits (yield should be non-negative)
//! 4. user_shares <= total_shares for all users
//! 5. user_balance <= total_assets for all users
//! 6. 0 <= locked_shares(user) <= shares(user) for all users (#987)
//! 7. `UserSharesIndex` (via `get_users_with_shares`) contains exactly the
//!    users with non-zero shares, no more, no fewer (#987)
//! 8. The vault is solvent: its own token balance covers `total_assets` minus
//!    whatever shares are locked-but-not-yet-withdrawn has no bearing on,
//!    since this harness never deploys funds to an external protocol (#987)
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
//! - `Error(Contract, #70)` — SharesAlreadyLocked
//! - `Error(Contract, #71)` — LockPeriodNotEnded
//! - `Error(Contract, #72)` — InvalidLockDuration
//! - `Error(Contract, #73)` — InsufficientUnlockedShares
//! - `Error(Contract, #74)` — EmergencyWithdrawalNotAllowed
//! - `Error(Contract, #69)` — NoSharesToMigrate
//! - `Error(Contract, #67)` — MigrationPaused
//! - `Error(Contract, #35)` — Paused (vault paused for emergency_withdraw setup)
//! - Token transfer failures
//! - Cross-contract call failures to the migration-target vault (its own
//!   deposit limits are a different contract's concern, not an accounting
//!   bug in this one)
//!
//! Input length is capped at `MAX_OPS_PER_CASE` chunks. `soroban-sdk`
//! 21.7.7's test harness builds a full ledger snapshot in `Env::drop`
//! (`to_test_snapshot_file`) when the process exits; with a long enough
//! operation sequence in one `Env` this snapshot-building step can itself
//! panic ("panic in a destructor during cleanup"), which aborts the whole
//! fuzzer process rather than just failing one input — confirmed via a
//! crafted ~70-op input of this exact shape. This is the same class of
//! `Env::drop` scale limitation already documented and `#[ignore]`d in
//! `test_tvl_cap_stress.rs` and `test_users_with_shares.rs`, not an
//! accounting bug; capping the op count here is the fuzz-target analog of
//! those tests' `#[ignore]`.

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

use token::{FuzzToken, FuzzTokenClient};

const MIN_DEPOSIT: i128 = 1_000_000;
const MAX_DEPOSIT: i128 = 10_000_000_000;
const USER_CAP: i128 = 10_000_000_000;
const TVL_CAP: i128 = 100_000_000_000;
const TOKEN_FLOAT: i128 = 50_000_000_000;
/// Matches `UPGRADE_TIMELOCK_LEDGERS`/`MIGRATION_TARGET_TIMELOCK_LEDGERS` in
/// `lib.rs` (24h at ~5s/ledger). Advanced once in `setup` so `migrate_shares`
/// is fuzzable without re-running the timelock on every step.
const MIGRATION_TIMELOCK_LEDGERS: u32 = 17_280;
const LOCK_DURATIONS_DAYS: [u32; 3] = [30, 90, 180];
/// Caps how many 4-byte op chunks a single fuzz input can drive. See the
/// module doc comment for why: cumulative resource/budget usage across
/// enough operations in one `Env` can exhaust the host's budget — observed
/// at both `Env::drop`'s snapshot-building AND inside auth-tree validation
/// (`soroban-env-host`'s `auth.rs`), not just at process exit — causing an
/// internal `unwrap()` on `Error(Budget, ExceededLimit)` to panic and abort
/// the whole process, independent of anything the contract itself does
/// wrong. This was tuned down twice by fuzzer-discovered crashes (40, then
/// 20 ops were both still too high); 12 plus a tighter
/// `process_withdrawal_queue` batch size (see its call site below) is the
/// current safe ceiling. If a future crash reproduces this same
/// `Error(Budget, ExceededLimit)` pattern, lower this further rather than
/// treating it as a contract bug.
const MAX_OPS_PER_CASE: usize = 12;

struct Harness<'a> {
    client: NeuroWealthVaultClient<'a>,
    owner: Address,
    users: Vec<Address>,
    usdc: Address,
}

/// Deploys the vault under test plus a second vault instance to serve as the
/// `migrate_shares` destination, and runs the real
/// `schedule_migration_target` / `execute_migration_target` timelock flow
/// once so `migrate_shares` is fuzzable from the first fuzz step onward.
fn setup(env: &Env) -> Harness<'_> {
    // Keep instance storage alive across the migration timelock's ~17,280
    // ledger jump below (mirrors test_migration_timelock.rs's setup).
    env.ledger().with_mut(|li| {
        li.min_persistent_entry_ttl = 100_000;
        li.min_temp_entry_ttl = 100_000;
        li.max_entry_ttl = 200_000;
    });

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

    // Create multiple users to test multi-user invariants
    let users: std::vec::Vec<Address> = (0..3).map(|_| Address::generate(env)).collect();

    client.initialize(&deployer, &owner, &agent, &usdc, &salt);

    let token = FuzzTokenClient::new(env, &usdc);
    for user in &users {
        token.mint(user, &TOKEN_FLOAT);
    }

    // A second vault, sharing the same USDC token, to be the migration
    // target. Its own deposit limits are deliberately left at their
    // defaults; a migrate_shares call that trips them is an expected panic
    // in the destination contract, not an accounting bug in the one under
    // test here.
    let migration_salt = BytesN::from_array(env, &[8u8; 32]);
    let migration_deployer = Address::generate(env);
    let migration_target_id = env
        .deployer()
        .with_address(migration_deployer.clone(), migration_salt.clone())
        .deployed_address();
    env.register_contract(&migration_target_id, NeuroWealthVault);
    let migration_client = NeuroWealthVaultClient::new(env, &migration_target_id);
    let migration_owner = Address::generate(env);
    let migration_agent = Address::generate(env);
    migration_client.initialize(
        &migration_deployer,
        &migration_owner,
        &migration_agent,
        &usdc,
        &migration_salt,
    );

    client.schedule_migration_target(&owner, &migration_target_id);
    env.ledger()
        .with_mut(|li| li.sequence_number += MIGRATION_TIMELOCK_LEDGERS + 1);
    client.execute_migration_target(&owner);

    Harness {
        client,
        owner,
        users,
        usdc,
    }
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
        "Error(Contract, #67)", // MigrationPaused
        "Error(Contract, #69)", // NoSharesToMigrate
        "Error(Contract, #70)", // SharesAlreadyLocked
        "Error(Contract, #71)", // LockPeriodNotEnded
        "Error(Contract, #72)", // InvalidLockDuration
        "Error(Contract, #73)", // InsufficientUnlockedShares
        "Error(Contract, #74)", // EmergencyWithdrawalNotAllowed
        "Error(Contract, #35)", // Paused
        "insufficient balance",
        "amount must be positive",
    ];
    ALLOWED.iter().any(|needle| msg.contains(needle))
}

/// Assert share-accounting invariants after each operation.
///
/// `migrated` must be `true` once any `migrate_shares` call has succeeded in
/// this run. `migrate_shares` moves principal out of the vault (like
/// `withdraw`) but, unlike `withdraw`, does not call
/// `reduce_total_deposits_on_withdraw` — this is documented, pre-existing
/// behavior (`TotalDeposits` is explicitly "principal bookkeeping and
/// reporting only", see `get_total_deposits`'s doc comment), not an
/// accounting bug introduced by #987's new coverage. Invariant 1 below only
/// holds "after `update_total_assets()` is called to reflect external
/// yield" per that same doc, i.e. absent a migration moving principal out
/// without a matching `TotalDeposits` adjustment.
fn assert_share_invariants(
    client: &NeuroWealthVaultClient,
    users: &[Address],
    usdc: &Address,
    env: &Env,
    migrated: bool,
) {
    let total_shares = client.get_total_shares();
    let total_assets = client.get_total_assets();
    let total_deposits = client.get_total_deposits();

    // Invariant 1: total_assets >= total_deposits (yield should be
    // non-negative or zero). Skipped once a migration has happened, since
    // migrate_shares intentionally leaves TotalDeposits unadjusted.
    if !migrated {
        assert!(
            total_assets >= total_deposits,
            "total_assets ({total_assets}) < total_deposits ({total_deposits})"
        );
    }

    let mut sum_of_shares: i128 = 0;

    // Invariant 2: Check individual user invariants
    for user in users {
        let user_shares = client.get_shares(user);
        let user_balance = client.get_balance(user);
        sum_of_shares = sum_of_shares
            .checked_add(user_shares)
            .expect("sum of shares overflow");

        // User shares must be non-negative
        assert!(user_shares >= 0, "user_shares is negative");

        // User balance must be non-negative
        assert!(user_balance >= 0, "user_balance is negative");

        // User shares cannot exceed total shares
        assert!(
            user_shares <= total_shares,
            "user_shares ({user_shares}) > total_shares ({total_shares})"
        );

        // User balance cannot exceed total assets
        assert!(
            user_balance <= total_assets,
            "user_balance ({user_balance}) > total_assets ({total_assets})"
        );

        // If total_shares > 0, user_balance should be proportional
        if total_shares > 0 && total_assets > 0 {
            let expected_balance = user_shares
                .checked_mul(total_assets)
                .expect("overflow in balance calc")
                / total_shares;
            // Allow for rounding (floor division means actual may be slightly less)
            assert!(
                user_balance <= expected_balance,
                "user_balance ({user_balance}) > expected ({expected_balance})"
            );
        } else if total_shares == 0 || total_assets == 0 {
            assert_eq!(user_balance, 0, "no shares but non-zero balance");
        }

        // Invariant 6 (#987): locked shares never exceed the user's total shares.
        let (locked_shares, _expiry) = client.get_locked_shares(user);
        assert!(locked_shares >= 0, "locked_shares is negative");
        assert!(
            locked_shares <= user_shares,
            "locked_shares ({locked_shares}) > user_shares ({user_shares})"
        );
    }

    // Invariant 3: Exchange rate should be >= 1.0 (10_000_000 in scaled units)
    if total_shares > 0 && total_assets > 0 {
        let exchange_rate = client.get_exchange_rate();
        assert!(
            exchange_rate >= 10_000_000,
            "exchange_rate ({exchange_rate}) < 1.0"
        );
    }

    // Invariant 7 (#987): the shares index contains exactly the users (from
    // this harness's known set) with non-zero shares, no more, no fewer.
    // The harness only ever has 3 users, well under one page, so a single
    // get_users_with_shares call covers all of them.
    let (indexed, _next) = client.get_users_with_shares(&0, &u32::MAX);
    let mut indexed_total: i128 = 0;
    for (addr, shares_in_index) in indexed.iter() {
        indexed_total = indexed_total
            .checked_add(shares_in_index)
            .expect("indexed shares overflow");
        assert!(
            users.contains(&addr),
            "index contains an address outside the harness's known users"
        );
        assert_eq!(
            shares_in_index,
            client.get_shares(&addr),
            "index entry disagrees with get_shares for the same user"
        );
    }
    for user in users {
        let in_index = indexed.iter().any(|(addr, _)| addr == *user);
        let has_shares = client.get_shares(user) > 0;
        assert_eq!(
            in_index, has_shares,
            "index membership disagrees with having non-zero shares for a user"
        );
    }
    assert_eq!(
        indexed_total, sum_of_shares,
        "sum(index shares) ({indexed_total}) != sum(user shares) ({sum_of_shares})"
    );

    // Invariant 8 (#987): vault solvency. This harness never deploys funds
    // to an external protocol, so every recorded asset must be sitting in
    // the vault's own token balance.
    let token_client = FuzzTokenClient::new(env, usdc);
    let vault_address = client.address.clone();
    let vault_token_balance = token_client.balance(&vault_address);
    assert!(
        vault_token_balance >= total_assets,
        "vault token balance ({vault_token_balance}) < total_assets ({total_assets}): insolvent"
    );
}

/// The actual per-input logic, factored out of the `fuzz_target!` macro
/// purely for readability. Note: a `libfuzzer-sys` `fuzz_target!` binary
/// always installs its own libFuzzer `main`, which intercepts every
/// argument `cargo test` or the binary itself is invoked with (including
/// `--list` and test-name filters) — there is no way to also expose plain
/// `#[test]` functions through this binary; any such module would be
/// silently unreachable. This crate's actual validation is running the
/// fuzzer itself: `cargo fuzz run share_accounting_invariants` (needs a
/// nightly toolchain), or `cargo test --bin share_accounting_invariants --
/// -max_total_time=<seconds>` (works on stable, since cargo-fuzz's own
/// `-Z` sanitizer flags aren't involved — only libFuzzer's runtime flags,
/// which it accepts directly).
fn run_case(data: &[u8]) {
    if data.is_empty() {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let harness = setup(&env);
    let Harness {
        client,
        owner,
        users,
        usdc,
    } = harness;
    let mut migrated = false;

    for (i, chunk) in data.chunks(4).take(MAX_OPS_PER_CASE).enumerate() {
        if chunk.is_empty() {
            continue;
        }

        let op = chunk[0] % 11;
        let user_idx = chunk.get(1).copied().unwrap_or(0) as usize % users.len();
        let raw = u16::from(chunk.get(2).copied().unwrap_or(0))
            | (u16::from(chunk.get(3).copied().unwrap_or(0)) << 8);
        let amount = i128::from(raw % 20_000) * MIN_DEPOSIT + MIN_DEPOSIT;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let user = &users[user_idx];

            match op {
                0 => {
                    // Deposit
                    if !(MIN_DEPOSIT..=MAX_DEPOSIT).contains(&amount)
                        || amount > USER_CAP
                        || amount > TVL_CAP
                    {
                        return;
                    }
                    let token = FuzzTokenClient::new(&env, &usdc);
                    if token.balance(user) < amount {
                        return;
                    }
                    client.deposit(user, &amount);
                }
                1 => {
                    // Withdraw
                    let balance = client.get_balance(user);
                    if balance <= 0 {
                        return;
                    }
                    let withdraw_amount = amount.min(balance);
                    if withdraw_amount <= 0 {
                        return;
                    }
                    client.withdraw(user, &withdraw_amount);
                }
                2 => {
                    // Deposit then immediately withdraw (round-trip)
                    if !(MIN_DEPOSIT..=MAX_DEPOSIT).contains(&amount)
                        || amount > USER_CAP
                        || amount > TVL_CAP
                    {
                        return;
                    }
                    let token = FuzzTokenClient::new(&env, &usdc);
                    if token.balance(user) < amount {
                        return;
                    }
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        client.deposit(user, &amount);
                    }));

                    let balance = client.get_balance(user);
                    if balance > MIN_DEPOSIT {
                        let withdraw_amount = balance / 2;
                        if withdraw_amount >= MIN_DEPOSIT {
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                client.withdraw(user, &withdraw_amount);
                            }));
                        }
                    }
                }
                3 => {
                    // Deposit from a different user (cross-user invariant test)
                    let other_idx = (user_idx + 1) % users.len();
                    let other_user = &users[other_idx];

                    if !(MIN_DEPOSIT..=MAX_DEPOSIT).contains(&amount)
                        || amount > USER_CAP
                        || amount > TVL_CAP
                    {
                        return;
                    }
                    let token = FuzzTokenClient::new(&env, &usdc);
                    if token.balance(other_user) < amount {
                        return;
                    }
                    client.deposit(other_user, &amount);
                }
                4 => {
                    // lock_shares (#987): lock a portion of the user's unlocked shares.
                    let user_shares = client.get_shares(user);
                    if user_shares <= 0 {
                        return;
                    }
                    let (already_locked, _) = client.get_locked_shares(user);
                    let unlocked = user_shares - already_locked;
                    if unlocked <= 0 {
                        return;
                    }
                    let to_lock = (amount % unlocked.max(1)).max(1).min(unlocked);
                    let duration = LOCK_DURATIONS_DAYS[user_idx % LOCK_DURATIONS_DAYS.len()];
                    client.lock_shares(user, &to_lock, &duration);
                }
                5 => {
                    // unlock_shares (#987): only succeeds once the lock has expired,
                    // which this harness never advances the ledger for mid-run, so
                    // this exercises the LockPeriodNotEnded rejection path far more
                    // often than a successful unlock — both are valid outcomes.
                    client.unlock_shares(user);
                }
                6 => {
                    // migrate_shares (#987): moves the user's full position to the
                    // pre-linked migration-target vault.
                    client.migrate_shares(user);
                }
                7 => {
                    // emergency_withdraw (#987): only allowed while paused.
                    // pause() now rejects a repeat call (#988:
                    // VaultError::AlreadyPaused), unlike before, so only
                    // pause if not already paused.
                    if !client.is_paused() {
                        client.pause(&owner);
                    }
                    let balance = client.get_balance(user);
                    if balance <= 0 {
                        return;
                    }
                    let withdraw_amount = amount.min(balance).max(1);
                    client.emergency_withdraw(user, &withdraw_amount);
                }
                8 => {
                    // queue_withdrawal (#987)
                    let balance = client.get_balance(user);
                    if balance <= 0 {
                        return;
                    }
                    let queue_amount = amount.min(balance).max(1);
                    client.queue_withdrawal(user, &queue_amount);
                }
                9 => {
                    // process_withdrawal_queue (#987): any authenticated caller may
                    // process a bounded batch (confirmed by
                    // test_any_authenticated_keeper_can_process_bounded_queue).
                    client.process_withdrawal_queue(user, &3u32);
                }
                10 => {
                    // batch_deposit (#987), single-entry batch.
                    if !(MIN_DEPOSIT..=MAX_DEPOSIT).contains(&amount)
                        || amount > USER_CAP
                        || amount > TVL_CAP
                    {
                        return;
                    }
                    let token = FuzzTokenClient::new(&env, &usdc);
                    if token.balance(user) < amount {
                        return;
                    }
                    let entries = soroban_sdk::vec![&env, (usdc.clone(), amount)];
                    client.batch_deposit(user, &entries);
                }
                _ => unreachable!(),
            }
        }));

        match result {
            Ok(()) => {
                if op == 6 {
                    migrated = true;
                }
                assert_share_invariants(&client, &users, &usdc, &env, migrated);
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
}

fuzz_target!(|data: &[u8]| {
    run_case(data);
});
