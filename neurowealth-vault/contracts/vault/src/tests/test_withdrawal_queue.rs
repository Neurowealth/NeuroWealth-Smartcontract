#![cfg(test)]

use super::utils::*;
use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env};

#[allow(clippy::too_many_arguments)]
fn assert_withdrawal_state(
    token: &TestTokenClient,
    client: &NeuroWealthVaultClient,
    contract_id: &Address,
    user: &Address,
    user_usdc: i128,
    user_shares: i128,
    total_shares: i128,
    total_assets: i128,
    vault_usdc: i128,
) {
    assert_eq!(token.balance(user), user_usdc, "user USDC balance");
    assert_eq!(client.get_shares(user), user_shares, "user shares");
    assert_eq!(client.get_total_shares(), total_shares, "TotalShares");
    assert_eq!(client.get_total_assets(), total_assets, "TotalAssets");
    assert_eq!(token.balance(contract_id), vault_usdc, "vault USDC balance");
}

#[test]
fn test_withdrawal_queue_ordering_cancellation_and_processing() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let deposit = 300_0000000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);

    // Configure queue: max_size = 5, ttl = 3600s
    client.set_queue_config(&5u32, &3600u64);
    let (max_size, ttl) = client.get_queue_config();
    assert_eq!(max_size, 5);
    assert_eq!(ttl, 3600);

    // Queue withdrawal request
    let req_id1 = client.queue_withdrawal(&user, &200_0000000);
    assert_eq!(req_id1, 1);

    let req1 = client.get_withdrawal_request(&req_id1);
    assert_eq!(req1.user, user);
    assert_eq!(req1.amount, 200_0000000);
    assert!(!req1.fulfilled);
    assert!(!req1.cancelled);

    // Queue second withdrawal request
    let req_id2 = client.queue_withdrawal(&user, &100_0000000);
    assert_eq!(req_id2, 2);

    // Cancel req2
    client.cancel_withdrawal_request(&user, &req_id2);
    assert!(client.try_get_withdrawal_request(&req_id2).is_err());

    assert_withdrawal_state(
        &token,
        &client,
        &contract_id,
        &user,
        0,
        deposit,
        deposit,
        deposit,
        deposit,
    );

    // Agent processes queue in FIFO order
    let user_balance_before = soroban_sdk::token::Client::new(&env, &usdc_token).balance(&user);
    let processed = client.process_withdrawal_queue(&agent, &10u32);
    assert_eq!(processed, 1); // 1 fulfilled, 1 cancelled skipped
    let user_balance_after = soroban_sdk::token::Client::new(&env, &usdc_token).balance(&user);

    // Verify user received USDC
    assert!(user_balance_after > user_balance_before);
    assert!(client.try_get_withdrawal_request(&req_id1).is_err());
    assert_withdrawal_state(
        &token,
        &client,
        &contract_id,
        &user,
        200_0000000,
        100_0000000,
        100_0000000,
        100_0000000,
        100_0000000,
    );
}

#[test]
fn test_any_authenticated_keeper_can_process_bounded_queue() {
    let env = Env::default();
    env.mock_all_auths();
    // 50 queue fulfillments in a single call exceed the default debug-build
    // CPU budget; the test asserts batching semantics, not metered cost.
    env.budget().reset_unlimited();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let keeper = soroban_sdk::Address::generate(&env);
    // 1,000 USDC (7-decimal units), not the raw 1,000 stroops this was
    // originally coded as (`1_000_i128`): that fell below
    // `DEFAULT_MIN_DEPOSIT` (1,000,000 stroops = 0.1 USDC) and panicked with
    // BelowMinimumDeposit on every run. Each of the 55 queued withdrawals
    // below is rescaled to match (1 USDC instead of 1 stroop) so the
    // relative before/after balances the test asserts stay the same shape.
    let deposit = 1_000_0000000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);
    client.set_queue_config(&100u32, &3600u64);

    for _ in 0..55 {
        client.queue_withdrawal(&user, &1_0000000);
    }

    assert_withdrawal_state(
        &token,
        &client,
        &contract_id,
        &user,
        0,
        deposit,
        deposit,
        deposit,
        deposit,
    );
    assert_eq!(
        client.process_withdrawal_queue(&keeper, &u32::MAX),
        crate::MAX_WITHDRAWAL_PROCESS_BATCH,
    );
    assert_withdrawal_state(
        &token,
        &client,
        &contract_id,
        &user,
        50_0000000,
        950_0000000,
        950_0000000,
        950_0000000,
        950_0000000,
    );
    assert_eq!(client.process_withdrawal_queue(&keeper, &u32::MAX), 5);
    assert_withdrawal_state(
        &token,
        &client,
        &contract_id,
        &user,
        55_0000000,
        945_0000000,
        945_0000000,
        945_0000000,
        945_0000000,
    );
}

#[test]
fn test_withdrawal_queue_keeps_request_pending_with_partial_liquidity() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let deposit = 10_0000000_i128;

    client.set_blend_pool(&owner, &blend_pool);
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);
    client.rebalance(
        &agent,
        &soroban_sdk::symbol_short!("blend"),
        &500_i128,
        &0_i128,
    );

    token.mint(&contract_id, &3_0000000_i128);
    client.update_total_assets(&agent, &13_0000000_i128, &false, &0_u32);
    let request_id = client.queue_withdrawal(&user, &5_0000000_i128);

    assert_withdrawal_state(
        &token,
        &client,
        &contract_id,
        &user,
        0,
        deposit,
        deposit,
        13_0000000,
        3_0000000,
    );
    assert_eq!(client.process_withdrawal_queue(&agent, &10), 0);
    assert!(client.try_get_withdrawal_request(&request_id).is_ok());
    assert_withdrawal_state(
        &token,
        &client,
        &contract_id,
        &user,
        0,
        deposit,
        deposit,
        13_0000000,
        3_0000000,
    );
}

#[test]
fn test_cannot_over_queue_withdrawals() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    client.set_queue_config(&100u32, &3600u64);

    // Deposit 1000 USDC
    let deposit_amount = 1_000_0000000i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Queue 900 USDC
    let req_id1 = client.queue_withdrawal(&user, &900_0000000);
    assert_eq!(req_id1, 1);

    // Try to queue another 900 USDC (should fail - only 100 available)
    assert!(client.try_queue_withdrawal(&user, &900_0000000).is_err());

    // Should be able to queue 100 USDC (the remaining amount)
    let req_id2 = client.queue_withdrawal(&user, &100_0000000);
    assert_eq!(req_id2, 2);

    // Now all shares are reserved, any additional queue should fail
    assert!(client.try_queue_withdrawal(&user, &1).is_err());
}

#[test]
fn test_cancel_unreserves_shares() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    client.set_queue_config(&100u32, &3600u64);

    // Deposit 1000 USDC
    let deposit_amount = 1_000_0000000i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Queue 900 USDC
    let req_id1 = client.queue_withdrawal(&user, &900_0000000);

    // Should not be able to withdraw/queue more
    assert!(client.try_queue_withdrawal(&user, &101_0000000).is_err());

    // Cancel the request
    client.cancel_withdrawal_request(&user, &req_id1);

    // Now should be able to queue again
    let req_id2 = client.queue_withdrawal(&user, &900_0000000);
    assert_eq!(req_id2, 2);
}

#[test]
fn test_cannot_queue_locked_shares() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    client.set_queue_config(&100u32, &3600u64);

    // Deposit 1000 USDC
    let deposit_amount = 1_000_0000000i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit_amount);

    // Lock half the shares. `lock_shares` only accepts 30/90/180-day
    // durations (lib.rs); the originally coded `1000u32` matched none of
    // them and panicked with InvalidLockDuration (#72) on every run — this
    // test's actual purpose (locked shares can't be queued) doesn't depend
    // on which of the three valid durations is used.
    client.lock_shares(&user, &(1_000_0000000i128 / 2), &30u32);

    // Try to queue all shares (should fail - half are locked)
    assert!(client.try_queue_withdrawal(&user, &1_000_0000000).is_err());

    // Should be able to queue only unlocked amount
    let req_id = client.queue_withdrawal(&user, &500_0000000);
    assert!(req_id > 0);
}

/// Regression test (#987): `queue_withdrawal`'s shares-needed calculation
/// used to add an unconditional `+ 1` instead of a true ceiling division, so
/// a user requesting their exact full balance (the single most common
/// withdrawal request) was computed as needing one more share than they
/// actually held, failing with `InsufficientSharesForAmount` even though the
/// request was for no more than their own balance.
#[test]
fn test_queue_withdrawal_accepts_exact_full_balance() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 2_768_000_000_i128);

    let full_balance = client.get_balance(&user);
    assert_eq!(full_balance, 2_768_000_000_i128);

    // Must not panic: requesting the exact full balance is always valid.
    let req_id = client.queue_withdrawal(&user, &full_balance);
    let req = client.get_withdrawal_request(&req_id);
    assert_eq!(req.amount, full_balance);
}

/// Regression test (#987): same scenario as above, but with a second user
/// present so the share/asset ratio isn't trivially 1:1, closer to how the
/// fuzz harness found the bug.
#[test]
fn test_queue_withdrawal_accepts_exact_full_balance_with_multiple_holders() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user_a, 1_000_0000000);
    mint_and_deposit(&env, &client, &usdc_token, &user_b, 2_768_000_000_i128);

    let full_balance_b = client.get_balance(&user_b);
    let req_id = client.queue_withdrawal(&user_b, &full_balance_b);
    let req = client.get_withdrawal_request(&req_id);
    assert_eq!(req.amount, full_balance_b);
}

// ── #1089: queued withdrawals reserve shares against locking and spending ────
//
// `queue_withdrawal` only reserves *assets*; the shares backing a request stay
// in the user's balance. `lock_shares` (and the direct withdrawal paths) used
// to ignore that reservation, so a user could queue a withdrawal and then lock
// or spend the same shares. When the queue reached the head request,
// `burn_and_pay` rejected the burn and the panic aborted the whole batch.

/// Regression test (#1089): shares reserved by a queued request cannot be
/// locked, and a lock on the unreserved remainder does not block the payout.
#[test]
fn test_lock_shares_cannot_lock_shares_reserved_by_queue() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let deposit = 1_000_0000000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);
    client.set_queue_config(&10u32, &3600u64);
    // Clear the default 120-ledger MinHoldingPeriod so payouts can run (#975).
    env.ledger().set_sequence_number(200u32);

    let request_id = client.queue_withdrawal(&user, &900_0000000_i128);

    // The request reserves 900 USDC worth of shares; locking either the
    // reserved amount or the full balance would strand it.
    assert!(client
        .try_lock_shares(&user, &900_0000000_i128, &30u32)
        .is_err());
    assert!(client.try_lock_shares(&user, &deposit, &30u32).is_err());
    assert_eq!(client.get_locked_shares(&user), (0, 0));

    // Only the unreserved remainder can be locked.
    client.lock_shares(&user, &100_0000000_i128, &30u32);
    assert_eq!(client.get_locked_shares(&user).0, 100_0000000_i128);

    // The queued request still pays out; the partial lock does not block it.
    assert_eq!(client.process_withdrawal_queue(&agent, &10u32), 1);
    assert!(client.try_get_withdrawal_request(&request_id).is_err());
    assert_eq!(token.balance(&user), 900_0000000_i128);
    assert_eq!(client.get_shares(&user), 100_0000000_i128);
}

/// Regression test (#1089): a direct withdrawal cannot spend shares that back
/// a pending queue request, and the request still fulfils afterwards.
#[test]
fn test_withdraw_cannot_spend_shares_reserved_by_queue() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let deposit = 1_000_0000000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);
    client.set_queue_config(&10u32, &3600u64);
    // Clear the default 120-ledger MinHoldingPeriod so payouts can run (#975).
    env.ledger().set_sequence_number(200u32);

    let request_id = client.queue_withdrawal(&user, &900_0000000_i128);

    // 100 USDC of the balance is still free; anything beyond that would eat
    // into the shares the queued request depends on.
    assert!(client.try_withdraw(&user, &150_0000000_i128).is_err());
    client.withdraw(&user, &50_0000000_i128);
    assert_eq!(client.get_shares(&user), 950_0000000_i128);

    assert_eq!(client.process_withdrawal_queue(&agent, &10u32), 1);
    assert!(client.try_get_withdrawal_request(&request_id).is_err());
    assert_eq!(client.get_shares(&user), 50_0000000_i128);
    assert_eq!(token.balance(&user), 950_0000000_i128);
}

/// Regression test (#1089): `withdraw_all` redeems only the unreserved shares
/// and leaves the queued request's shares in place for the queue to pay.
#[test]
fn test_withdraw_all_keeps_shares_reserved_by_queue() {
    let env = Env::default();
    env.mock_all_auths();
    let (contract_id, agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);
    let user = Address::generate(&env);
    let deposit = 1_000_0000000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &user, deposit);
    client.set_queue_config(&10u32, &3600u64);
    // Clear the default 120-ledger MinHoldingPeriod so payouts can run (#975).
    env.ledger().set_sequence_number(200u32);

    let request_id = client.queue_withdrawal(&user, &200_0000000_i128);

    assert_eq!(client.withdraw_all(&user), 800_0000000_i128);
    assert_eq!(client.get_shares(&user), 200_0000000_i128);

    assert_eq!(client.process_withdrawal_queue(&agent, &10u32), 1);
    assert!(client.try_get_withdrawal_request(&request_id).is_err());
    assert_eq!(client.get_shares(&user), 0);
    assert_eq!(token.balance(&user), deposit);
}
