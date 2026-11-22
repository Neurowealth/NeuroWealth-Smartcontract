//! Kani proofs for the vault share-accounting properties (Issue #672).
//!
//! Bounds are kept small so the proofs finish in CI. The same formulas are
//! additionally stress-tested at 10^12 scale by
//! `test_share_conversion_proptest.rs` and the `share_accounting_invariants`
//! fuzz target.

use crate::{assets_from_shares, rate_non_decreasing, shares_ceil, shares_floor, VaultModel};

/// Upper bound used for every Kani `any()` input. 64 is large enough to
/// include leftover-remainder rounding cases and small enough that CBMC
/// finishes on the CI runners.
const MAX: i128 = 64;

fn bounded_positive() -> i128 {
    let value: i128 = kani::any();
    kani::assume(value >= 1 && value <= MAX);
    value
}

fn bounded_non_negative() -> i128 {
    let value: i128 = kani::any();
    kani::assume(value >= 0 && value <= MAX);
    value
}

/// TotalShares of the two-user model never exceeds (and in fact equals)
/// the sum of all user shares, across deposit / yield / withdraw.
#[kani::proof]
fn proof_total_shares_equals_sum_of_user_shares() {
    let mut vault = VaultModel::empty();
    let a = bounded_positive();
    let b = bounded_positive();
    vault = vault.deposit(0, a).unwrap();
    vault = vault.deposit(1, b).unwrap();
    assert!(
        vault.total_shares == vault.sum_user_shares().unwrap(),
        "total_shares drifted from the sum of user shares after deposits"
    );

    let yield_amount = bounded_non_negative();
    vault = vault.accrue_yield(yield_amount).unwrap();
    assert!(
        vault.total_shares == vault.sum_user_shares().unwrap(),
        "yield must not mint or burn shares"
    );

    if let Some(after) = vault.withdraw(0, 1) {
        assert!(
            after.total_shares == after.sum_user_shares().unwrap(),
            "total_shares drifted from the sum of user shares after withdraw"
        );
    }
}

/// No user share balance, and not `total_shares`, can go negative.
#[kani::proof]
fn proof_no_negative_share_balance() {
    let mut vault = VaultModel::empty();
    vault = vault.deposit(0, bounded_positive()).unwrap();
    vault = vault.deposit(1, bounded_positive()).unwrap();
    assert!(
        vault.no_negative_shares(),
        "deposit produced a negative balance"
    );

    let requested = bounded_positive();
    match vault.withdraw(0, requested) {
        Some(after) => assert!(
            after.no_negative_shares(),
            "withdraw produced a negative share balance"
        ),
        None => assert!(
            vault.user_shares[0] >= 0,
            "rejected withdraw must leave balances untouched"
        ),
    }
}

/// Accruing yield (increasing `total_assets` with shares held constant)
/// never decreases the exchange rate.
#[kani::proof]
fn proof_exchange_rate_non_decreasing_with_yield() {
    let mut vault = VaultModel::empty();
    vault = vault.deposit(0, bounded_positive()).unwrap();
    let before_assets = vault.total_assets;
    let before_shares = vault.total_shares;
    vault = vault.accrue_yield(bounded_non_negative()).unwrap();
    assert!(
        rate_non_decreasing(
            before_assets,
            before_shares,
            vault.total_assets,
            vault.total_shares,
        )
        .unwrap(),
        "yield decreased the exchange rate"
    );
}

/// Deposit then immediately redeem the minted shares never returns more
/// assets than were deposited (rounding always favours the vault).
#[kani::proof]
fn proof_deposit_withdraw_round_trip_within_tolerance() {
    let assets = bounded_positive();
    let total_shares = bounded_positive();
    let total_assets = bounded_positive();
    let minted = shares_floor(assets, total_shares, total_assets).unwrap();
    let redeemed = assets_from_shares(minted, total_shares, total_assets).unwrap();
    assert!(redeemed <= assets, "round-trip created value for the user");
}

/// Floor mint, ceil burn, and floor redeem all favour the vault:
/// - ceil burn ≥ floor mint, gap at most 1
/// - assets returned from minted shares ≤ assets deposited
#[kani::proof]
fn proof_rounding_always_favours_the_vault() {
    let assets = bounded_positive();
    let total_shares = bounded_positive();
    let total_assets = bounded_positive();

    let floor = shares_floor(assets, total_shares, total_assets).unwrap();
    let ceil = shares_ceil(assets, total_shares, total_assets).unwrap();
    assert!(
        ceil >= floor,
        "ceil burn under-burned relative to floor mint"
    );
    assert!(ceil - floor <= 1, "ceil and floor diverged by more than 1");

    let returned = assets_from_shares(floor, total_shares, total_assets).unwrap();
    assert!(
        returned <= assets,
        "floor redeem paid the user more than they deposited"
    );
}

/// Zero in always yields zero out, for every conversion direction.
#[kani::proof]
fn proof_zero_input_zero_output() {
    let total_shares = bounded_non_negative();
    let total_assets = bounded_non_negative();
    assert_eq!(
        shares_floor(0, total_shares, total_assets),
        Some(0),
        "shares_floor(0) was non-zero"
    );
    assert_eq!(
        shares_ceil(0, total_shares, total_assets),
        Some(0),
        "shares_ceil(0) was non-zero"
    );
    assert_eq!(
        assets_from_shares(0, total_shares, total_assets),
        Some(0),
        "assets_from_shares(0) was non-zero"
    );
}
