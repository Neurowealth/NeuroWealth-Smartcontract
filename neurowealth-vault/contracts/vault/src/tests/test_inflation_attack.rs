//! First-depositor / donation inflation-attack scenario tests.
//!
//! ## The attack (on a vulnerable, balance-based vault)
//! 1. Attacker is the first depositor for a negligible amount and receives a
//!    tiny number of shares.
//! 2. Attacker "donates" a large amount by transferring tokens directly to the
//!    vault. On a vault that prices shares off its live token balance, this
//!    inflates the price per share.
//! 3. A victim deposits; with the price inflated, their assets round DOWN to
//!    zero shares. The attacker then redeems the victim's funds.
//!
//! ## Why this vault is safe (audit conclusion)
//! Share pricing reads the **stored** `TotalAssets`, which is only mutated by
//! `deposit`, `withdraw`, and the agent's balance-verified `update_total_assets`.
//! A direct token transfer to the vault does not change `TotalAssets`, so the
//! donation step (2) is a no-op for pricing. Combined with the minimum-deposit
//! floor and the non-zero-share-mint guard, the attack is neutralized. These
//! tests assert that behavior.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

/// A direct token transfer to the vault (a "donation") must NOT change
/// `TotalAssets` or the share price. This is the core mitigation.
#[test]
fn test_direct_donation_does_not_inflate_share_price() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let attacker = Address::generate(&env);
    let seed = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &attacker, seed);

    let assets_before = client.get_total_assets();
    let shares_before = client.get_total_shares();

    // Attacker donates a large amount straight to the vault, bypassing deposit.
    let donation = 1_000_000_000_i128;
    token.mint(&attacker, &donation);
    token.transfer(&attacker, &contract_id, &donation);

    // Share price = TotalAssets / TotalShares. Both are storage-tracked and must
    // be unchanged by a direct donation, so the price is unchanged.
    assert_eq!(
        client.get_total_assets(),
        assets_before,
        "donation must not change stored TotalAssets"
    );
    assert_eq!(
        client.get_total_shares(),
        shares_before,
        "donation must not change TotalShares"
    );
}

/// Full scenario: attacker seeds, donates, then a victim deposits. The victim
/// must receive fair (non-zero, proportional) shares, and the attacker must not
/// profit from the donation.
#[test]
fn test_donation_then_victim_deposit_is_fair() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    // 1. Attacker becomes first depositor (must clear the minimum deposit).
    let attacker = Address::generate(&env);
    let attacker_deposit = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &attacker, attacker_deposit);
    let attacker_value_before = client.get_balance(&attacker);

    // 2. Attacker donates a large amount directly to the vault.
    let donation = 1_000_000_000_i128;
    token.mint(&attacker, &donation);
    token.transfer(&attacker, &contract_id, &donation);

    // 3. Victim deposits.
    let victim = Address::generate(&env);
    let victim_deposit = 10_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &victim, victim_deposit);

    // Victim received non-zero, fair shares (1:1 since the donation is ignored).
    let victim_shares = client.get_shares(&victim);
    assert!(victim_shares > 0, "victim must receive shares, not zero");
    assert_eq!(
        victim_shares, victim_deposit,
        "victim shares must be proportional (1:1) — donation ignored"
    );

    // Victim's redeemable value equals their deposit; nothing was stolen.
    assert_eq!(
        client.get_balance(&victim),
        victim_deposit,
        "victim must be able to reclaim ~their full deposit"
    );

    // Attacker did not gain from the donation: their claim is still their seed.
    assert_eq!(
        client.get_balance(&attacker),
        attacker_value_before,
        "attacker's claim must not grow from the donation"
    );
}

/// End-to-end: the victim can actually withdraw their full deposit after the
/// attacker's donation, proving funds were never at risk.
#[test]
fn test_victim_can_withdraw_full_deposit_after_donation() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token = TestTokenClient::new(&env, &usdc_token);

    let attacker = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &attacker, 10_000_000_i128);

    let donation = 5_000_000_000_i128;
    token.mint(&attacker, &donation);
    token.transfer(&attacker, &contract_id, &donation);

    let victim = Address::generate(&env);
    let victim_deposit = 25_000_000_i128;
    mint_and_deposit(&env, &client, &usdc_token, &victim, victim_deposit);

    let returned = client.withdraw_all(&victim, &None);
    assert_eq!(
        returned, victim_deposit,
        "victim must withdraw their full deposit despite the donation"
    );
    assert_eq!(token.balance(&victim), victim_deposit);
}

/// Property-style sweep: across a range of seed / donation / victim sizes, the
/// victim always receives non-zero shares and a fair claim.
#[test]
fn test_inflation_resistance_across_sizes() {
    let cases = [
        (1_000_000_i128, 1_000_000_000_i128, 1_000_000_i128),
        (10_000_000_i128, 50_000_000_000_i128, 2_000_000_i128),
        (5_000_000_i128, 1_000_000_i128, 100_000_000_i128),
        (1_000_000_i128, 999_999_999_999_i128, 1_000_000_i128),
    ];

    for (seed, donation, victim_deposit) in cases {
        let env = Env::default();
        env.mock_all_auths();

        let (contract_id, _agent, _owner, usdc_token) = setup_vault_with_token(&env);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);
        let token = TestTokenClient::new(&env, &usdc_token);

        let attacker = Address::generate(&env);
        mint_and_deposit(&env, &client, &usdc_token, &attacker, seed);

        token.mint(&attacker, &donation);
        token.transfer(&attacker, &contract_id, &donation);

        let victim = Address::generate(&env);
        mint_and_deposit(&env, &client, &usdc_token, &victim, victim_deposit);

        // The inflation attack's payoff is the victim minting ZERO shares; a
        // non-zero share balance is the decisive proof the attack fails.
        let victim_shares = client.get_shares(&victim);
        assert!(
            victim_shares > 0,
            "victim must receive non-zero shares (seed={}, donation={}, victim={})",
            seed,
            donation,
            victim_deposit
        );
        // The donation is ignored, so the victim's claim tracks their deposit up
        // to ERC-4626 floor-division dust (never more than deposited, and at most
        // a negligible amount less). This rules out value theft.
        let claim = client.get_balance(&victim);
        let tolerance = victim_deposit / 100_000 + 10; // 0.001% + dust
        assert!(
            claim <= victim_deposit && claim >= victim_deposit - tolerance,
            "victim claim must be within dust of deposit (claim={}, deposit={}, seed={}, donation={})",
            claim,
            victim_deposit,
            seed,
            donation
        );
    }
}
