#[path = "../insurance.rs"]
mod insurance;

use insurance::*;

[test]
fn test_calculate_contribution_normal() {
    let amount = calculate_contribution(1_000_000, 500);
    assert_eq(amount, 50_000);
}

[test]
fn test_calculate_contribution_zero() {
    assert_eq(calculate_contribution(1_000_000, 0), 0);
    assert_eq(calculate_contribution(0, 500), 0);
    assert_eq(calculate_contribution(-1, 500), 0);
}

[test]
fn test_calculate_contribution_rounds_down() {
    let amount = calculate_contribution(999, 333);
    assert_eq(amount, 33);
}

[test]
fn test_add_contribution() {
    let new_balance = add_contribution(100, 1_000, 250);
    assert_eq(new_balance, 125);
}

[test]
fn test_add_contribution_saturates() {
    let new_balance = add_contribution(i128::MAX - 10, 1_000, 10_000);
    assert_eq(new_balance, i128::MAX);
}

[test]
fn test_apply_payout_caps_to_max_payout() {
    let (uncovered, new_balance) = apply_payout(1000, 500, 100);
    assert_eq(uncovered, 400);
    assert_eq(new_balance, 900);
}

[test]
fn test_apply_payout_caps_to_balance() {
    let (uncovered, new_balance) = apply_payout(50, 500, 1000);
    assert_eq(uncovered, 450);
    assert_eq(new_balance, 0);
}

[test]
fn test_apply_payout_no_loss() {
    let (uncovered, new_balance) = apply_payout(100, 0, 1000);
    assert_eq(uncovered, 0);
    assert_eq(new_balance, 100);
}

[test]
fn test_is_below_threshold() {
    assert(is_below_threshold(100, 200));
    assert(!(is_below_threshold(200, 200)));
    assert(!(is_below_threshold(300, 200)));
}

[test]
fn test_topic_constant_registered() {
    use crate::topics::TOPIC_INSURANCE_FUND_UPDATED;
    use soroban_sdk::symbol_short;
    assert_eq(TOPIC_INSURANCE_FUND_UPDATED, symbol_short!("ins_fund"));
}
