#![cfg(test)]

use crate::tests::utils::{create_test_env, setup_vault};
use crate::VaultError;

#[test]
fn test_min_withdrawal_setting_and_enforcement() {
    let (_env, client, owner, _, user, _, _) = setup_vault();

    // Default is 0
    assert_eq!(client.get_min_withdrawal(), 0);

    // Set min withdrawal to 100_0000000 (100 USDC)
    client.set_min_withdrawal(&owner, &100_0000000).unwrap();
    assert_eq!(client.get_min_withdrawal(), 100_0000000);

    // Deposit 500 USDC
    client.deposit(&user, &500_0000000);

    // Attempt withdrawal below minimum (50 USDC) -> should panic with BelowMinimumWithdrawal
    let res = client.try_withdraw(&user, &50_0000000);
    assert_eq!(res, Err(Ok(VaultError::BelowMinimumWithdrawal)));

    // Withdraw at or above minimum (150 USDC) -> success
    client.withdraw(&user, &150_0000000);

    // Full exit withdraw_all bypasses minimum check
    client.withdraw_all(&user);
}

#[test]
fn test_min_withdrawal_must_be_positive() {
    let (_, client, owner, _, _, _, _) = setup_vault();
    let res = client.set_min_withdrawal(&owner, &0);
    assert_eq!(res, Err(Ok(VaultError::MinWithdrawalMustBePositive)));
}
