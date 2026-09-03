//! Insurance fund mechanics for the NeuroWealth Vault.
//
// These pure functions compute contribution, payout, and threshold checks.
// They are deliberately kept free of Soroban storage so they can be unit
// tested and reused by future integrations.

#![warn(missing_docs)]

/// Basis points denominator.
pub const BPS: i128 = 10_000;

/// Configuration for the insurance fund.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsuranceConfig {
    /// Contribution rate in basis points (e.g., 500 = 5%).
    pub contribution_rate_bps: i128,
    /// Maximum payout per incident in raw units.
    pub max_payout_per_incident: i128,
    /// Minimum fund balance threshold in raw units.
    pub min_threshold: i128,
}

impl InsuranceConfig {
    /// Create a new config.
    pub fn new(contribution_rate_bps: i128, max_payout_per_incident: i128, min_threshold: i128) -> Self {
        Self {
            contribution_rate_bps,
            max_payout_per_incident,
            min_threshold,
        }
    }
}

/// Calculates the insurance contribution from a yield amount.
pub fn calculate_contribution(yield_amount: i128, rate_bps: i128) -> i128 {
    if yield_amount <= 0 || rate_bps <= 0 {
        return 0;
    }
    yield_amount.saturating_mul(rate_bps) / BPS
}

/// Adds a contribution to the fund balance.
pub fn add_contribution(balance: i128, yield_amount: i128, rate_bps: i128) -> i128 {
    balance.saturating_add(calculate_contribution(yield_amount, rate_bps))
}

/// Applies an insurance payout.
/// Returns `(uncovered_loss, new_balance)`.
pub fn apply_payout(
    balance: i128,
    loss_amount: i128,
    max_payout_per_incident: i128,
) -> (i128, i128) {
    if loss_amount <= 0 || balance <= 0 || max_payout_per_incident <= 0 {
        return (loss_amount, balance);
    }
    let payout = loss_amount.min(max_payout_per_incident).min(balance);
    (loss_amount - payout, balance - payout)
}

/// Returns `true` of the balance is below the minimum threshold.
pub fn is_below_threshold(balance: i128, min_threshold: i128) -> bool {
    balance < min_threshold
}
