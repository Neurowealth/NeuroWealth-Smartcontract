//! Property tests for share-conversion math (Issue #323, #412).
//!
//! These tests exercise the mathematical invariants of the vault's share-pricing
//! formulas in isolation, using `proptest` to generate thousands of random inputs
//! without needing the Soroban environment.
//!
//! The three helper functions below replicate the *exact* integer arithmetic from:
//!   - `convert_to_shares_internal`       lib.rs lines 4229-4247  (floor mint)
//!   - `convert_to_shares_internal_ceil`  lib.rs lines 4253-4284  (ceil burn)
//!   - `convert_to_assets_internal`       lib.rs lines 4288-4305  (floor return)
//!
//! Any change to those formulas that breaks the invariants here will fail CI.
//!
//! Invariants tested:
//!   (a) Round-trip no-value-creation: assets → shares (floor) → assets (floor) ≤ input.
//!   (b) Ceil-burn ≥ floor-mint for the same asset amount (vault never under-burns).
//!   (c) Ceil and floor differ by at most 1 (tightest possible rounding gap).
//!   (d) Monotonicity: more assets in → more-or-equal shares out.
//!   (e) Zero input → zero output for all three directions.
//!   (f) Conservation: shares minted for A assets, converted back, never exceed A.
//!   (g) (#412) Ceil round-trip: assets → shares_ceil → assets ≤ assets.
//!   (h) (#412) Deposit+withdraw cycle: user never receives more than deposited.
//!   (i) (#412) Withdraw+deposit cycle: shares burned consistent with original.

// The vault crate is `#![no_std]`; tests are run with the standard test harness
// which links std, but we must declare it explicitly in no_std crates.
extern crate std;

use proptest::prelude::*;
use share_math::{assets_from_shares, shares_ceil, shares_floor};

// ---------------------------------------------------------------------------
// Input strategy
//
// Bounded to 1..=10^12 so that the worst-case intermediate product
// (assets × total_shares = 10^24) fits comfortably inside i128 (max ~1.7×10^38).
// ---------------------------------------------------------------------------

const MAX_VAL: i128 = 1_000_000_000_000i128; // 10^12

proptest! {
    // -----------------------------------------------------------------------
    // (a) + (f)  Round-trip never creates assets
    // -----------------------------------------------------------------------

    /// Depositing `assets`, receiving `shares_floor(assets)` shares, then
    /// converting those shares back must never yield *more* than `assets`.
    ///
    /// This is the core ERC-4626 rounding invariant: rounding always favours
    /// the vault, never the user.
    #[test]
    fn prop_round_trip_never_creates_assets(
        assets       in 1i128..=MAX_VAL,
        total_shares in 1i128..=MAX_VAL,
        total_assets in 1i128..=MAX_VAL,
    ) {
        let shares = shares_floor(assets, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");
        let assets_back = assets_from_shares(shares, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        prop_assert!(
            assets_back <= assets,
            "round-trip created value: {} assets → {} shares → {} assets \
             (total_shares={}, total_assets={})",
            assets, shares, assets_back, total_shares, total_assets
        );
    }

    // -----------------------------------------------------------------------
    // (b) + (c)  Ceil-burn ≥ floor-mint, gap ≤ 1
    // -----------------------------------------------------------------------

    /// The shares burned on withdrawal (ceil) must be ≥ the shares minted on
    /// deposit (floor) for the same asset amount.  The gap must be at most 1:
    /// ceil and floor integer division can differ by exactly 1 when the result
    /// is not an integer.
    #[test]
    fn prop_ceil_burn_gte_floor_mint_gap_at_most_one(
        assets       in 1i128..=MAX_VAL,
        total_shares in 1i128..=MAX_VAL,
        total_assets in 1i128..=MAX_VAL,
    ) {
        let floor_shares = shares_floor(assets, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");
        let ceil_shares = shares_ceil(assets, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        prop_assert!(
            ceil_shares >= floor_shares,
            "ceil ({}) < floor ({}) for assets={} total_shares={} total_assets={}",
            ceil_shares, floor_shares, assets, total_shares, total_assets
        );
        prop_assert!(
            ceil_shares - floor_shares <= 1,
            "ceil-floor gap > 1: ceil={} floor={} assets={} total_shares={} total_assets={}",
            ceil_shares, floor_shares, assets, total_shares, total_assets
        );
    }

    // -----------------------------------------------------------------------
    // (d)  Monotonicity
    // -----------------------------------------------------------------------

    /// More assets always produce more-or-equal shares (floor conversion).
    /// This ensures the pricing function is non-decreasing.
    #[test]
    fn prop_monotone_more_assets_more_shares(
        base  in 0i128..=MAX_VAL / 2,
        delta in 0i128..=MAX_VAL / 2,
        total_shares in 1i128..=MAX_VAL,
        total_assets in 1i128..=MAX_VAL,
    ) {
        let assets1 = base;
        let assets2 = base + delta; // always >= assets1, no overflow since both halved

        let s1 = shares_floor(assets1, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");
        let s2 = shares_floor(assets2, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        prop_assert!(
            s2 >= s1,
            "monotonicity broken: assets {} → {} shares, assets {} → {} shares \
             (total_shares={}, total_assets={})",
            assets1, s1, assets2, s2, total_shares, total_assets
        );
    }

    // -----------------------------------------------------------------------
    // (e)  Zero input always gives zero output
    // -----------------------------------------------------------------------

    /// Regardless of vault state, converting zero assets/shares must return zero.
    #[test]
    fn prop_zero_input_zero_output(
        total_shares in 0i128..=MAX_VAL,
        total_assets in 0i128..=MAX_VAL,
    ) {
        prop_assert_eq!(
            shares_floor(0, total_shares, total_assets),
            Some(0),
            "shares_floor(0) != 0 for total_shares={} total_assets={}",
            total_shares, total_assets
        );
        prop_assert_eq!(
            shares_ceil(0, total_shares, total_assets),
            Some(0),
            "shares_ceil(0) != 0 for total_shares={} total_assets={}",
            total_shares, total_assets
        );
        prop_assert_eq!(
            assets_from_shares(0, total_shares, total_assets),
            Some(0),
            "assets_from_shares(0) != 0 for total_shares={} total_assets={}",
            total_shares, total_assets
        );
    }

    // -----------------------------------------------------------------------
    // (f)  Conservation: withdrawal of exactly minted shares never exceeds deposit
    // -----------------------------------------------------------------------

    /// A user who deposits `assets` and later redeems exactly the shares they
    /// were minted must receive back ≤ `assets`.  This is a restatement of (a)
    /// from the issue's acceptance criteria phrased as the withdrawal path.
    #[test]
    fn prop_redeem_minted_shares_no_excess(
        assets       in 1i128..=MAX_VAL,
        total_shares in 1i128..=MAX_VAL,
        total_assets in 1i128..=MAX_VAL,
    ) {
        let shares_minted = shares_floor(assets, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");
        let assets_redeemed = assets_from_shares(shares_minted, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        prop_assert!(
            assets_redeemed <= assets,
            "redemption exceeded deposit: deposited {} assets, minted {} shares, \
             redeemed {} assets (total_shares={}, total_assets={})",
            assets, shares_minted, assets_redeemed, total_shares, total_assets
        );
    }

    // -----------------------------------------------------------------------
    // (g)  Ceil round-trip: assets → shares_ceil → assets ≤ assets  (#412)
    // -----------------------------------------------------------------------

    /// The withdrawal path uses ceil conversion for shares (vault protection)
    /// and floor for assets. This invariant verifies that even with ceil rounding
    /// on the share side, the user never gets back more than they deposited.
    #[test]
    fn prop_ceil_round_trip_no_excess(
        assets       in 1i128..=MAX_VAL,
        total_shares in 1i128..=MAX_VAL,
        total_assets in 1i128..=MAX_VAL,
    ) {
        let shares_ceil_val = shares_ceil(assets, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");
        let assets_back = assets_from_shares(shares_ceil_val, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        prop_assert!(
            assets_back <= assets,
            "ceil round-trip created value: {} assets → {} shares (ceil) → {} assets \
             (total_shares={}, total_assets={})",
            assets, shares_ceil_val, assets_back, total_shares, total_assets
        );
    }

    // -----------------------------------------------------------------------
    // (h)  Deposit+withdraw cycle: user never receives more than deposited (#412)
    // -----------------------------------------------------------------------

    /// Simulates a full deposit-then-withdraw cycle:
    ///   1. Deposit `assets` → receive `shares_floor` shares.
    ///   2. Immediately withdraw those exact shares → get back `assets_from_shares`.
    ///   3. The amount received must be ≤ the amount deposited.
    ///
    /// This is the real user-facing invariant: no matter what the vault state is,
    /// a deposit followed by a full withdrawal should never create value for the user.
    #[test]
    fn prop_deposit_withdraw_cycle_no_excess(
        assets       in 1i128..=MAX_VAL,
        total_shares in 1i128..=MAX_VAL,
        total_assets in 1i128..=MAX_VAL,
    ) {
        let shares_minted = shares_floor(assets, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        // Nothing to withdraw if no shares were minted.
        if shares_minted == 0 {
            return Ok(());
        }

        // Withdrawal burns shares_ceil(shares_minted, ...) shares.
        // But in the real vault, the user specifies assets to withdraw,
        // and the vault computes shares to burn via ceil.
        // So we model: user wants to withdraw all their assets,
        // which means burning their shares via ceil conversion.
        let shares_to_burn = shares_ceil(shares_minted, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        // The vault returns assets_from_shares(shares_burned)
        let assets_returned = assets_from_shares(shares_to_burn, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        prop_assert!(
            assets_returned <= assets,
            "deposit+withdraw cycle created value: deposited {}, got back {} \
             (shares_minted={}, shares_burned={}, total_shares={}, total_assets={})",
            assets, assets_returned, shares_minted, shares_to_burn,
            total_shares, total_assets
        );

        // The user should never lose more than 1 unit to rounding per direction
        // (floor + ceil = at most 2 units total rounding loss).
        let max_loss = if assets >= 2 { 2i128 } else { assets };
        let loss = assets - assets_returned;
        prop_assert!(
            loss <= max_loss,
            "excessive rounding loss: deposited {}, got back {}, loss {} > max {} \
             (total_shares={}, total_assets={})",
            assets, assets_returned, loss, max_loss,
            total_shares, total_assets
        );
    }

    // -----------------------------------------------------------------------
    // (i)  Withdraw+deposit cycle: shares burned consistent with original (#412)
    // -----------------------------------------------------------------------

    /// A user holds `held_shares`. The vault has total_shares and total_assets.
    /// If the user withdraws an amount `assets` (computed from floor conversion of
    /// their shares) and then immediately deposits those same assets back,
    /// the new shares minted should be ≤ the original shares burned.
    ///
    /// This ensures the vault cannot inflate the share count through a
    /// withdraw+deposit cycle.
    #[test]
    fn prop_withdraw_deposit_cycle_no_inflation(
        held_shares  in 1i128..=MAX_VAL,
        total_shares in 1i128..=MAX_VAL,
        total_assets in 1i128..=MAX_VAL,
    ) {
        // The user's entitled assets (what they'd get on withdrawal)
        let entitled_assets = assets_from_shares(held_shares, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        if entitled_assets == 0 {
            return Ok(());
        }

        // Withdrawal path: shares_to_burn = ceil(entitled_assets)
        let shares_burned = shares_ceil(entitled_assets, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        // The vault returns assets_from_shares(shares_burned) — floor
        let assets_returned = assets_from_shares(shares_burned, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        // Re-deposit: new_shares = floor(assets_returned)
        let new_shares = shares_floor(assets_returned, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        // New shares should never exceed the original shares that were burned.
        prop_assert!(
            new_shares <= held_shares,
            "withdraw+deposit cycle inflated shares: held {} shares, burned {} for {} assets, \
             re-deposited to {} shares (total_shares={}, total_assets={})",
            held_shares, shares_burned, assets_returned, new_shares,
            total_shares, total_assets
        );
    }

    // -----------------------------------------------------------------------
    // (j)  Harvest conversion round-trip (#520)
    // -----------------------------------------------------------------------

    /// Simulates a deposit followed by a harvest that alters total_assets,
    /// then a withdrawal. Verifies that the share-to-asset conversion remains
    /// bounded and consistent.
    #[test]
    fn prop_harvest_conversion_round_trip(
        assets       in 1i128..=MAX_VAL,
        total_shares in 1i128..=MAX_VAL,
        total_assets in 1i128..=MAX_VAL,
        harvest_yield in 0i128..=MAX_VAL / 10,
    ) {
        let shares_minted = shares_floor(assets, total_shares, total_assets)
            .expect("overflow not possible at tested bounds");

        if shares_minted == 0 {
            return Ok(());
        }

        let new_total_shares = total_shares + shares_minted;
        let new_total_assets = total_assets + assets;

        // Harvest adds yield to total_assets without changing total_shares
        let post_harvest_assets = new_total_assets + harvest_yield;

        // User wants to withdraw their full position. First compute the
        // assets they are entitled to, then compute shares to burn via ceil.
        let entitled_assets = assets_from_shares(shares_minted, new_total_shares, post_harvest_assets)
            .expect("overflow not possible at tested bounds");

        if entitled_assets == 0 {
            return Ok(());
        }

        let shares_to_burn = shares_ceil(entitled_assets, new_total_shares, post_harvest_assets)
            .expect("overflow not possible at tested bounds");

        let assets_returned = assets_from_shares(shares_to_burn, new_total_shares, post_harvest_assets)
            .expect("overflow not possible at tested bounds");

        // The user's expected exact share of the vault post-harvest
        let expected_exact_assets = (shares_minted as i128) * (post_harvest_assets as i128) / (new_total_shares as i128);

        // Given ceil and floor rounding, the user should receive at most expected_exact_assets + 2
        prop_assert!(
            assets_returned <= expected_exact_assets + 2,
            "Deposit+harvest+withdraw yielded too many assets: deposited {}, yield {}, returned {} > expected ~{} \
             (shares_minted={}, new_total_shares={}, post_harvest_assets={})",
            assets, harvest_yield, assets_returned, expected_exact_assets,
            shares_minted, new_total_shares, post_harvest_assets
        );
    }
}
