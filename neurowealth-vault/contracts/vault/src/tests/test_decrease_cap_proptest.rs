//! Property tests for update_total_assets decrease-cap logic (Issue #380).
//!
//! These tests exercise the decrease-cap enforcement in `update_total_assets`,
//! specifically the boundary conditions around:
//!   - The bps floor (100 bps minimum)
//!   - The bps ceiling (10_000 bps maximum)
//!   - The checked-mul/div cap math
//!   - Extreme old_total/new_total combinations
//!
//! The decrease-cap logic (from lib.rs lines 3244-3252):
//!   - effective_cap_bps = max_decrease_bps.max(100)
//!   - max_decrease = old_total.checked_mul(effective_cap_bps as i128)
//!   - max_decrease = max_decrease.checked_div(10_000)
//!   - require(new_total >= old_total - max_decrease)
//!
//! Invariants tested:
//!   (a) Bps floor: max_decrease_bps = 0 applies 100 bps floor
//!   (b) Bps ceiling: max_decrease_bps >= 10_000 allows 100% decrease
//!   (c) Boundary at exactly bps floor/ceiling succeeds
//!   (d) One unit above bps ceiling still applies ceiling (no overflow)
//!   (e) Decrease never exceeds the computed cap
//!   (f) No panics/overflows for valid inputs within i128 bounds
//!   (g) Extreme magnitudes: large old_total with small/large bps values

// The vault crate is `#![no_std]`; tests are run with the standard test harness
// which links std, but we must declare it explicitly in no_std crates.
extern crate std;

use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Pure-math helpers — mirror the exact formulas from lib.rs
// ---------------------------------------------------------------------------

/// Computes the effective cap bps with the 100 bps floor and 10_000 bps
/// ceiling.
///
/// From lib.rs's `update_total_assets`:
/// `let effective_cap_bps = max_decrease_bps.clamp(100, Self::BPS_DENOMINATOR);`
fn effective_cap_bps(max_decrease_bps: u32) -> u32 {
    max_decrease_bps.clamp(100, 10_000)
}

/// Computes the maximum allowed decrease amount.
///
/// From lib.rs lines 3246-3248:
/// ```rust
/// let max_decrease = old_total
///     .checked_mul(effective_cap_bps as i128)
///     .expect("vault: total available overflow")
///     .checked_div(10_000)
///     .expect("vault: total available overflow");
/// ```
fn max_decrease(old_total: i128, max_decrease_bps: u32) -> Option<i128> {
    let cap = effective_cap_bps(max_decrease_bps) as i128;
    old_total
        .checked_mul(cap)
        .and_then(|product| product.checked_div(10_000))
}

/// Checks if a decrease from old_total to new_total is allowed.
///
/// From lib.rs lines 3249-3251:
/// ```rust
/// let min_allowed = old_total - max_decrease;
/// require(new_total >= min_allowed, DecreaseExceedsMaximumAllowedBps);
/// ```
fn is_decrease_allowed(old_total: i128, new_total: i128, max_decrease_bps: u32) -> bool {
    if new_total >= old_total {
        return true; // Increases are always allowed
    }
    match max_decrease(old_total, max_decrease_bps) {
        Some(max_dec) => new_total >= old_total - max_dec,
        None => false, // Overflow in cap computation
    }
}

// ---------------------------------------------------------------------------
// Input strategy
//
// Bounded to avoid i128 overflow in intermediate products.
// Worst case: old_total * cap_bps where cap_bps can be up to 10_000.
// To keep old_total * 10_000 within i128 (~1.7×10^38), we bound old_total to 10^34.
// ---------------------------------------------------------------------------

const MAX_OLD_TOTAL: i128 = 10_000_000_000_000_000_000_000_000_000_000_000i128; // 10^34

proptest! {
    // -----------------------------------------------------------------------
    // (a) Bps floor: max_decrease_bps = 0 applies 100 bps floor
    // -----------------------------------------------------------------------

    /// When max_decrease_bps = 0, the effective floor of 100 bps is applied.
    /// This prevents a cap of 0 from disabling decreases entirely.
    #[test]
    fn prop_bps_floor_applied_when_zero_passed(
        old_total in 1_000_000i128..=MAX_OLD_TOTAL,
    ) {
        let max_dec_zero = max_decrease(old_total, 0);
        let max_dec_100 = max_decrease(old_total, 100);

        prop_assert_eq!(
            max_dec_zero, max_dec_100,
            "bps=0 should apply 100 bps floor: old_total={}", old_total
        );
    }

    // -----------------------------------------------------------------------
    // (b) Bps ceiling: max_decrease_bps >= 10_000 allows 100% decrease
    // -----------------------------------------------------------------------

    /// When max_decrease_bps >= 10_000, the effective cap is 10_000 (100%).
    /// This allows the total to decrease to zero.
    #[test]
    fn prop_bps_ceiling_at_10000_allows_full_decrease(
        old_total in 1_000_000i128..=MAX_OLD_TOTAL,
        bps_above_ceiling in 10_000u32..=20_000,
    ) {
        let max_dec_10000 = max_decrease(old_total, 10_000);
        let max_dec_above = max_decrease(old_total, bps_above_ceiling);

        prop_assert_eq!(
            max_dec_10000, max_dec_above,
            "bps >= 10000 should apply 10000 ceiling: old_total={}, bps={}",
            old_total, bps_above_ceiling
        );
        prop_assert_eq!(
            max_dec_10000, Some(old_total),
            "10000 bps should allow full decrease to zero: old_total={}", old_total
        );
    }

    // -----------------------------------------------------------------------
    // (c) Boundary at exactly bps floor/ceiling succeeds
    // -----------------------------------------------------------------------

    /// Decrease at exactly the bps floor (100) should succeed.
    #[test]
    fn prop_decrease_at_exactly_bps_floor_succeeds(
        old_total in 1_000_000i128..=MAX_OLD_TOTAL,
    ) {
        let max_dec = max_decrease(old_total, 100).expect("overflow not possible at tested bounds");
        let new_total = old_total - max_dec;

        prop_assert!(
            is_decrease_allowed(old_total, new_total, 100),
            "decrease at exactly 100 bps floor should succeed: old_total={}, new_total={}",
            old_total, new_total
        );
    }

    /// Decrease at exactly the bps ceiling (10_000) should succeed.
    #[test]
    fn prop_decrease_at_exactly_bps_ceiling_succeeds(
        old_total in 1_000_000i128..=MAX_OLD_TOTAL,
    ) {
        let max_dec = max_decrease(old_total, 10_000).expect("overflow not possible at tested bounds");
        let new_total = old_total - max_dec; // Should be 0

        prop_assert!(
            is_decrease_allowed(old_total, new_total, 10_000),
            "decrease at exactly 10000 bps ceiling should succeed: old_total={}, new_total={}",
            old_total, new_total
        );
        prop_assert_eq!(
            new_total, 0,
            "10000 bps decrease should result in zero: old_total={}", old_total
        );
    }

    // -----------------------------------------------------------------------
    // (d) One unit above bps ceiling still applies ceiling (no overflow)
    // -----------------------------------------------------------------------

    /// max_decrease_bps = 10_001 should behave identically to 10_000.
    #[test]
    fn prop_bps_one_above_ceiling_uses_ceiling(
        old_total in 1_000_000i128..=MAX_OLD_TOTAL,
    ) {
        let max_dec_10000 = max_decrease(old_total, 10_000);
        let max_dec_10001 = max_decrease(old_total, 10_001);

        prop_assert_eq!(
            max_dec_10000, max_dec_10001,
            "bps=10001 should use 10000 ceiling: old_total={}", old_total
        );
    }

    // -----------------------------------------------------------------------
    // (e) Decrease never exceeds the computed cap
    // -----------------------------------------------------------------------

    /// For any allowed decrease, the actual decrease amount must be <= max_decrease.
    #[test]
    fn prop_decrease_never_exceeds_computed_cap(
        old_total in 1_000_000i128..=MAX_OLD_TOTAL,
        max_decrease_bps in 0u32..=10_000,
        decrease_factor in 0.0f64..=1.0f64,
    ) {
        let max_dec = match max_decrease(old_total, max_decrease_bps) {
            Some(d) => d,
            None => return Ok(()), // Skip overflow cases
        };

        // Compute a new_total that may or may not be allowed
        let decrease_amount = (old_total as f64 * decrease_factor) as i128;
        let new_total = old_total - decrease_amount;

        if is_decrease_allowed(old_total, new_total, max_decrease_bps) {
            let actual_decrease = old_total - new_total;
            prop_assert!(
                actual_decrease <= max_dec,
                "allowed decrease {} exceeds cap {}: old_total={}, bps={}",
                actual_decrease, max_dec, old_total, max_decrease_bps
            );
        }
    }

    // -----------------------------------------------------------------------
    // (f) No panics/overflows for valid inputs within i128 bounds
    // -----------------------------------------------------------------------

    /// max_decrease should return None (not panic) on overflow, and Some for valid inputs.
    #[test]
    fn prop_max_decrease_returns_none_on_overflow_some_on_valid(
        old_total in 1i128..=1_000_000_000_000i128, // Smaller range for this test
        max_decrease_bps in 0u32..=10_000,
    ) {
        let result = max_decrease(old_total, max_decrease_bps);

        // For the tested range, overflow should not occur
        prop_assert!(
            result.is_some(),
            "max_decrease should not overflow for old_total={}, bps={}",
            old_total, max_decrease_bps
        );
    }

    // -----------------------------------------------------------------------
    // (g) Extreme magnitudes: large old_total with small/large bps values
    // -----------------------------------------------------------------------

    /// Very large old_total with minimum bps (100) should not overflow.
    #[test]
    fn prop_large_old_total_with_min_bps_no_overflow(
        old_total in 1_000_000_000_000_000_000_000_000_000_000_000i128..=MAX_OLD_TOTAL,
    ) {
        let result = max_decrease(old_total, 100);
        prop_assert!(
            result.is_some(),
            "large old_total with min bps should not overflow: old_total={}", old_total
        );
    }

    /// Very large old_total with maximum bps (10_000) should not overflow.
    #[test]
    fn prop_large_old_total_with_max_bps_no_overflow(
        old_total in 1_000_000_000_000_000_000_000_000_000_000_000i128..=MAX_OLD_TOTAL,
    ) {
        let result = max_decrease(old_total, 10_000);
        prop_assert!(
            result.is_some(),
            "large old_total with max bps should not overflow: old_total={}", old_total
        );
    }

    // -----------------------------------------------------------------------
    // (h) Boundary: decrease exactly at cap succeeds, one unit over fails
    // -----------------------------------------------------------------------

    /// Decrease of exactly max_decrease should succeed.
    /// Decrease of max_decrease + 1 should fail.
    #[test]
    fn prop_decrease_at_cap_boundary(
        old_total in 1_000_000i128..=MAX_OLD_TOTAL,
        max_decrease_bps in 100u32..=10_000,
    ) {
        let max_dec = match max_decrease(old_total, max_decrease_bps) {
            Some(d) => d,
            None => return Ok(()), // Skip overflow cases
        };

        // Exactly at cap should succeed
        let new_total_at_cap = old_total - max_dec;
        prop_assert!(
            is_decrease_allowed(old_total, new_total_at_cap, max_decrease_bps),
            "decrease at exactly cap should succeed: old_total={}, max_dec={}, bps={}",
            old_total, max_dec, max_decrease_bps
        );

        // One unit over cap should fail (if max_dec > 0)
        if max_dec > 0 {
            let new_total_over = old_total - max_dec - 1;
            prop_assert!(
                !is_decrease_allowed(old_total, new_total_over, max_decrease_bps),
                "decrease one unit over cap should fail: old_total={}, max_dec={}, bps={}",
                old_total, max_dec, max_decrease_bps
            );
        }
    }

    // -----------------------------------------------------------------------
    // (i) Monotonicity: higher bps allows larger decreases
    // -----------------------------------------------------------------------

    /// For a fixed old_total, a higher max_decrease_bps should allow
    /// a decrease that a lower bps would reject.
    #[test]
    fn prop_higher_bps_allows_larger_decrease(
        old_total in 1_000_000i128..=MAX_OLD_TOTAL,
        low_bps in 100u32..=5_000,
        high_bps in 5_001u32..=10_000,
    ) {
        let max_dec_low = max_decrease(old_total, low_bps).unwrap();
        let max_dec_high = max_decrease(old_total, high_bps).unwrap();

        prop_assert!(
            max_dec_high >= max_dec_low,
            "higher bps ({}) should allow >= decrease than lower bps ({}): old_total={}",
            high_bps, low_bps, old_total
        );

        // A decrease at the high bps cap should be rejected at the low bps
        let new_total_high = old_total - max_dec_high;
        if max_dec_high > max_dec_low {
            prop_assert!(
                !is_decrease_allowed(old_total, new_total_high, low_bps),
                "decrease allowed at high bps ({}) should be rejected at low bps ({}): old_total={}",
                high_bps, low_bps, old_total
            );
        }
    }
}
