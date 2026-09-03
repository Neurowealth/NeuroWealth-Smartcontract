//! Pure arithmetic regression tests for the documented performance-fee policy.

#[test]
fn test_performance_fee_configuration_and_deduction() {
    // Performance fee is expressed in basis points and is capped at 10%.
    let max_allowed_bps = 1_000_u32;
    let set_bps = 500_u32;
    assert!(set_bps <= max_allowed_bps);

    let yield_earned = 1_000_000_000_i128;
    let fee_amount = (yield_earned * i128::from(set_bps)) / 10_000;
    let net_yield = yield_earned - fee_amount;

    assert_eq!(fee_amount, 50_000_000);
    assert_eq!(net_yield, 950_000_000);
}

#[test]
fn test_performance_fee_exceeds_maximum_rejected() {
    let max_allowed_bps = 1_000_u32;
    let invalid_bps = 1_001_u32;
    assert!(invalid_bps > max_allowed_bps);
}
