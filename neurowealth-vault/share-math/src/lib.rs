//! Pure share-accounting arithmetic used by the `NeuroWealth` vault.
//!
//! These helpers are the *exact* integer formulas implemented by
//! `NeuroWealthVault::convert_to_shares_internal`,
//! `convert_to_shares_internal_ceil`, and `convert_to_assets_internal`.
//! Extracting them into a `no_std`, Soroban-free crate lets Kani (and the
//! existing `proptest` suite) prove the economic invariants against the same
//! code the contract executes on-chain.
//!
//! Rounding policy (ERC-4626):
//! - Deposit mints with **floor** division (vault keeps the dust).
//! - Withdrawal burns with **ceil** division (user burns ≥ the exact shares).
//! - Share→asset conversion uses **floor** division (user never receives extra).

#![cfg_attr(not(any(test, kani)), no_std)]
#![allow(clippy::similar_names)]

/// Floor conversion used on deposit: `floor(assets × total_shares / total_assets)`.
///
/// Returns `None` only when the intermediate product overflows `i128`.
/// A zero `assets` input always yields `Some(0)`. When either side of the
/// pool is empty the mapping is 1:1 (`Some(assets)`).
#[must_use]
pub fn shares_floor(assets: i128, total_shares: i128, total_assets: i128) -> Option<i128> {
    if assets == 0 {
        return Some(0);
    }
    if total_shares == 0 || total_assets == 0 {
        return Some(assets);
    }
    assets.checked_mul(total_shares)?.checked_div(total_assets)
}

/// Ceiling conversion used on withdrawal: `ceil(assets × total_shares / total_assets)`.
///
/// Implemented as `(assets × total_shares + total_assets − 1) / total_assets`.
/// Returns `None` on overflow. A zero `assets` input always yields `Some(0)`.
#[must_use]
pub fn shares_ceil(assets: i128, total_shares: i128, total_assets: i128) -> Option<i128> {
    if assets == 0 {
        return Some(0);
    }
    if total_shares == 0 || total_assets == 0 {
        return Some(assets);
    }
    let product = assets.checked_mul(total_shares)?;
    let numerator = product.checked_add(total_assets.checked_sub(1)?)?;
    numerator.checked_div(total_assets)
}

/// Floor conversion of shares back to assets: `floor(shares × total_assets / total_shares)`.
///
/// Returns `Some(0)` when `shares == 0` or either pool total is zero.
/// Returns `None` on overflow.
#[must_use]
pub fn assets_from_shares(shares: i128, total_shares: i128, total_assets: i128) -> Option<i128> {
    if shares == 0 {
        return Some(0);
    }
    if total_shares == 0 || total_assets == 0 {
        return Some(0);
    }
    shares.checked_mul(total_assets)?.checked_div(total_shares)
}

/// Compare two exchange rates `assets/shares` without floating point.
///
/// Returns `true` when `after_assets/after_shares >= before_assets/before_shares`
/// (monotonically non-decreasing). Empty-share vaults are treated as a
/// bootstrap 1:1 rate.
#[must_use]
pub fn rate_non_decreasing(
    before_assets: i128,
    before_shares: i128,
    after_assets: i128,
    after_shares: i128,
) -> Option<bool> {
    if before_shares == 0 || after_shares == 0 {
        return Some(true);
    }
    let left = after_assets.checked_mul(before_shares)?;
    let right = before_assets.checked_mul(after_shares)?;
    Some(left >= right)
}

/// Two-user vault model used by Kani proofs of conservation properties.
///
/// `total_shares` is maintained as the sum of `user_shares`. Operations that
/// would overflow or take a user negative are rejected (`None`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultModel {
    /// Sum of all user share balances.
    pub total_shares: i128,
    /// Reported vault assets (principal + yield).
    pub total_assets: i128,
    /// Per-user share balances. Length is fixed so Kani can unroll easily.
    pub user_shares: [i128; 2],
}

impl VaultModel {
    /// Empty vault (bootstrap state).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            total_shares: 0,
            total_assets: 0,
            user_shares: [0, 0],
        }
    }

    /// Sum of the two user balances. Used as the conservation oracle.
    #[must_use]
    pub fn sum_user_shares(&self) -> Option<i128> {
        self.user_shares[0].checked_add(self.user_shares[1])
    }

    /// Whether every share balance is non-negative.
    #[must_use]
    pub fn no_negative_shares(&self) -> bool {
        self.user_shares[0] >= 0 && self.user_shares[1] >= 0 && self.total_shares >= 0
    }

    /// Accrue yield by increasing `total_assets` without minting shares.
    #[must_use]
    pub fn accrue_yield(self, yield_amount: i128) -> Option<Self> {
        if yield_amount < 0 {
            return None;
        }
        Some(Self {
            total_assets: self.total_assets.checked_add(yield_amount)?,
            ..self
        })
    }

    /// Deposit `assets` for `user` (0 or 1) using floor mint.
    #[must_use]
    pub fn deposit(self, user: usize, assets: i128) -> Option<Self> {
        if user > 1 || assets <= 0 {
            return None;
        }
        let minted = shares_floor(assets, self.total_shares, self.total_assets)?;
        if minted < 0 {
            return None;
        }
        let mut next = self;
        next.user_shares[user] = next.user_shares[user].checked_add(minted)?;
        next.total_shares = next.total_shares.checked_add(minted)?;
        next.total_assets = next.total_assets.checked_add(assets)?;
        Some(next)
    }

    /// Withdraw `assets` for `user` using ceil-burn + floor-return.
    ///
    /// The user must hold enough shares to cover the ceil-burned amount.
    /// Assets returned to the user are `assets_from_shares(burned)` and are
    /// deducted from `total_assets`.
    #[must_use]
    pub fn withdraw(self, user: usize, assets: i128) -> Option<Self> {
        if user > 1 || assets <= 0 {
            return None;
        }
        let burned = shares_ceil(assets, self.total_shares, self.total_assets)?;
        if burned < 0 || self.user_shares[user] < burned {
            return None;
        }
        let returned = assets_from_shares(burned, self.total_shares, self.total_assets)?;
        let mut next = self;
        next.user_shares[user] = next.user_shares[user].checked_sub(burned)?;
        next.total_shares = next.total_shares.checked_sub(burned)?;
        next.total_assets = next.total_assets.checked_sub(returned)?;
        Some(next)
    }
}

pub mod queue;

#[cfg(kani)]
mod proofs;

#[cfg(test)]
mod tests {
    use super::{assets_from_shares, shares_ceil, shares_floor, VaultModel};

    #[test]
    fn round_trip_never_creates_assets() {
        let assets = 1_000_000_i128;
        let total_shares = 5_000_000_i128;
        let total_assets = 5_000_000_i128;
        let shares = shares_floor(assets, total_shares, total_assets).unwrap();
        let back = assets_from_shares(shares, total_shares, total_assets).unwrap();
        assert!(back <= assets);
    }

    #[test]
    fn ceil_burn_never_under_floor_mint() {
        let assets = 7_i128;
        let total_shares = 3_i128;
        let total_assets = 5_i128;
        let floor = shares_floor(assets, total_shares, total_assets).unwrap();
        let ceil = shares_ceil(assets, total_shares, total_assets).unwrap();
        assert!(ceil >= floor);
        assert!(ceil - floor <= 1);
    }

    #[test]
    fn model_preserves_sum_and_non_negative() {
        let mut vault = VaultModel::empty();
        vault = vault.deposit(0, 1_000_000).unwrap();
        vault = vault.deposit(1, 2_000_000).unwrap();
        vault = vault.accrue_yield(100_000).unwrap();
        vault = vault.withdraw(0, 500_000).unwrap();
        assert_eq!(vault.total_shares, vault.sum_user_shares().unwrap());
        assert!(vault.no_negative_shares());
    }

    /// Deposit `assets`, then immediately withdraw the exact same amount
    /// through the real contract flow (floor mint, ceil burn, floor
    /// redeem). Rounding must only ever cost the withdrawing user — it can
    /// never conjure assets, and it can never take more than a few units of
    /// dust relative to the pool's precision.
    ///
    /// Each case below sits on a rounding boundary: exact divisions (no
    /// remainder), a remainder of exactly `total_assets - 1` (worst case for
    /// ceil), prime-number pools (no common factors to cancel a remainder),
    /// a single-unit pool, and values near `i128::MAX` (#778).
    struct RoundTripCase {
        name: &'static str,
        assets: i128,
        total_shares: i128,
        total_assets: i128,
    }

    /// Runs the mint / ceil-burn / full-model-cycle assertions for one
    /// rounding-boundary case. Split out of the test itself purely to stay
    /// under `clippy::pedantic`'s `too_many_lines`.
    fn assert_round_trip_favours_vault(case: &RoundTripCase) {
        let minted = shares_floor(case.assets, case.total_shares, case.total_assets)
            .unwrap_or_else(|| panic!("{}: mint overflowed", case.name));
        assert!(minted >= 0, "{}: mint produced negative shares", case.name);

        // Redeeming the freshly minted shares (deposit's own inverse) must
        // never return more than was deposited.
        let redeemed = assets_from_shares(minted, case.total_shares, case.total_assets)
            .unwrap_or_else(|| panic!("{}: redeem overflowed", case.name));
        assert!(
            redeemed <= case.assets,
            "{}: round-trip minted value out of thin air ({redeemed} > {})",
            case.name,
            case.assets
        );

        // Ceil-burn for the same nominal amount must never under-charge
        // relative to the floor-mint, and the two must never diverge by
        // more than a single share.
        let burned = shares_ceil(case.assets, case.total_shares, case.total_assets)
            .unwrap_or_else(|| panic!("{}: ceil burn overflowed", case.name));
        assert!(
            burned >= minted,
            "{}: ceil burn ({burned}) under-charged floor mint ({minted})",
            case.name
        );
        assert!(
            burned - minted <= 1,
            "{}: ceil/floor diverged by more than one share ({burned} vs {minted})",
            case.name
        );

        // Full round trip through the vault model: deposit into an existing
        // pool of this size, then withdraw exactly what those minted shares
        // are worth (the maximum the user could withdraw without another
        // party's deposit or yield moving the rate). The user must never
        // walk away having extracted more assets than they put in.
        let vault = VaultModel {
            total_shares: case.total_shares,
            total_assets: case.total_assets,
            user_shares: [case.total_shares, 0],
        };
        let after_deposit = vault
            .deposit(1, case.assets)
            .unwrap_or_else(|| panic!("{}: model deposit failed", case.name));
        let entitlement = assets_from_shares(
            minted,
            after_deposit.total_shares,
            after_deposit.total_assets,
        )
        .unwrap_or_else(|| panic!("{}: entitlement overflowed", case.name));
        assert!(
            entitlement <= case.assets,
            "{}: post-deposit entitlement exceeds the original deposit ({entitlement} > {})",
            case.name,
            case.assets
        );
        if entitlement == 0 {
            // Dust-sized deposits can round down to a zero-value
            // withdrawal; there is nothing left to redeem, which is itself
            // the vault-favouring outcome under test.
            return;
        }
        let after_withdraw = after_deposit
            .withdraw(1, entitlement)
            .unwrap_or_else(|| panic!("{}: model withdraw failed", case.name));

        assert!(
            after_withdraw.user_shares[1] >= 0,
            "{}: withdrawing user went negative",
            case.name
        );
        assert_eq!(
            after_withdraw.total_shares,
            after_withdraw.sum_user_shares().unwrap(),
            "{}: total_shares diverged from the sum of user shares",
            case.name
        );
        assert!(
            after_withdraw.total_assets >= vault.total_assets,
            "{}: pool lost assets net of a full deposit+withdraw cycle ({} < {})",
            case.name,
            after_withdraw.total_assets,
            vault.total_assets
        );
    }

    /// Deposit `assets`, then immediately withdraw the same amount through
    /// the real contract flow (floor mint, ceil burn, floor redeem).
    /// Rounding must only ever cost the withdrawing user — it can never
    /// conjure assets, and mint/burn can never diverge by more than a
    /// single share.
    ///
    /// Each case sits on a rounding boundary: exact divisions (no
    /// remainder), a remainder of exactly `total_assets - 1` (worst case for
    /// ceil), prime-number pools (no common factors to cancel a remainder),
    /// a single-unit pool, and values near `i128::MAX` (#778).
    #[test]
    fn deposit_withdraw_round_trip_precision_across_rounding_boundaries() {
        let cases = [
            RoundTripCase {
                name: "exact division, no remainder",
                assets: 1_000_000,
                total_shares: 5_000_000,
                total_assets: 5_000_000,
            },
            RoundTripCase {
                name: "maximal remainder (total_assets - 1)",
                assets: 7,
                total_shares: 3,
                total_assets: 5,
            },
            RoundTripCase {
                name: "coprime pool, no common factor to cancel remainder",
                assets: 97,
                total_shares: 101,
                total_assets: 103,
            },
            RoundTripCase {
                name: "single-unit pool (1:1 bootstrap-adjacent)",
                assets: 1,
                total_shares: 1,
                total_assets: 1,
            },
            RoundTripCase {
                name: "assets smaller than total_shares (fractional share price)",
                assets: 3,
                total_shares: 1_000,
                total_assets: 7,
            },
            RoundTripCase {
                name: "large values near i128 precision limits",
                assets: 1_000_000_000_000_000_000,
                total_shares: 3_000_000_000_000_000_001,
                total_assets: 9_000_000_000_000_000_001,
            },
        ];

        for case in &cases {
            assert_round_trip_favours_vault(case);
        }
    }
}
