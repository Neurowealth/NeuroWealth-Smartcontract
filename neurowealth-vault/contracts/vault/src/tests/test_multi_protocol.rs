#![cfg(test)]
//! Multi-protocol (simultaneous Blend + DEX) allocation tests.
//!
//! Covers the Phase 2 allocation model that replaces the legacy
//! mutual-exclusion `CurrentProtocol` behaviour:
//!
//! - migration on/off multi-protocol mode,
//! - allocation validation,
//! - `rebalance_multi` converging to a target split (including re-splits in
//!   both directions),
//! - per-venue deployment tracking,
//! - proportional withdrawal across both venues,
//! - per-protocol APY and composite yield,
//! - `ProtocolAllocationChangedEvent` emission.

extern crate std;

use super::utils::*;
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

const BPS: u32 = 10_000;

/// Registers a vault with **both** a Blend pool and a DEX pool configured.
///
/// Returns `(vault_id, agent, owner, usdc_token, blend_pool, dex_pool)`.
fn setup_multi(env: &Env) -> (Address, Address, Address, Address, Address, Address) {
    let (vault_id, agent, owner, usdc_token) = setup_vault_with_token(env);
    let client = NeuroWealthVaultClient::new(env, &vault_id);

    let blend_pool = env.register_contract(None, MockBlendPool);
    let dex_pool = env.register_contract(None, MockDexPool);
    client.set_blend_pool(&owner, &blend_pool);
    client.set_dex_pool(&owner, &dex_pool);

    (vault_id, agent, owner, usdc_token, blend_pool, dex_pool)
}

/// Sets up a vault in multi-protocol mode with `amount` deposited by one user.
fn setup_multi_funded(
    env: &Env,
    amount: i128,
) -> (
    Address,
    NeuroWealthVaultClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let (vault_id, _agent, owner, usdc_token, blend_pool, dex_pool) = setup_multi(env);
    let client = NeuroWealthVaultClient::new(env, &vault_id);

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, amount);

    client.enable_multi_protocol(&true);
    assert!(client.is_multi_protocol_enabled());

    (vault_id, client, usdc_token, blend_pool, dex_pool, user)
}

// ===========================================================================
// MIGRATION PATH
// ===========================================================================

#[test]
fn test_multi_protocol_disabled_by_default() {
    let env = Env::default();
    env.mock_all_auths();
    let (vault_id, _a, _o, _t, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    assert!(!client.is_multi_protocol_enabled());
    assert_eq!(client.get_allocation(), (0, 0));
}

#[test]
fn test_enable_multi_protocol_from_none_seeds_zero_allocation() {
    let env = Env::default();
    env.mock_all_auths();
    let (vault_id, _a, _o, _t, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    client.enable_multi_protocol(&true);

    assert!(client.is_multi_protocol_enabled());
    assert_eq!(client.get_allocation(), (0, 0));
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
}

#[test]
fn test_enable_multi_protocol_from_blend_seeds_full_blend_allocation() {
    let env = Env::default();
    env.mock_all_auths();
    let (vault_id, _a, _o, usdc, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    let user = Address::generate(&env);
    let amount = 100_000_000_i128;
    mint_and_deposit(&env, &client, &usdc, &user, amount);

    // Legacy single-protocol deployment.
    client.rebalance(&symbol_short!("blend"), &800, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    // Migrating must not move any funds.
    client.enable_multi_protocol(&true);

    assert_eq!(client.get_allocation(), (BPS, 0));
    assert_eq!(client.get_deployed_to_blend(), amount);
    assert_eq!(client.get_deployed_to_dex(), 0);
    // Summary symbol is preserved for legacy consumers.
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
}

#[test]
fn test_enable_multi_protocol_from_dex_seeds_full_dex_allocation() {
    let env = Env::default();
    env.mock_all_auths();
    let (vault_id, _a, _o, usdc, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    let user = Address::generate(&env);
    let amount = 100_000_000_i128;
    mint_and_deposit(&env, &client, &usdc, &user, amount);

    client.rebalance(&symbol_short!("dex"), &800, &0_i128);
    client.enable_multi_protocol(&true);

    assert_eq!(client.get_allocation(), (0, BPS));
    assert_eq!(client.get_deployed_to_dex(), amount);
    assert_eq!(client.get_deployed_to_blend(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #77)")]
fn test_disable_multi_protocol_with_both_venues_funded_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, amount);

    client.rebalance_multi(&6_000, &4_000, &0_i128);
    assert!(client.get_deployed_to_blend() > 0);
    assert!(client.get_deployed_to_dex() > 0);

    // Collapsing back would lose information about one of the two positions.
    client.enable_multi_protocol(&false);
}

#[test]
fn test_disable_multi_protocol_after_consolidating_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, amount);

    client.rebalance_multi(&6_000, &4_000, &0_i128);
    // Consolidate everything back into Blend.
    client.rebalance_multi(&BPS, &0, &0_i128);
    assert_eq!(client.get_deployed_to_dex(), 0);

    client.enable_multi_protocol(&false);
    assert!(!client.is_multi_protocol_enabled());
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
}

// ===========================================================================
// ALLOCATION VALIDATION
// ===========================================================================

#[test]
#[should_panic(expected = "Error(Contract, #78)")]
fn test_rebalance_multi_requires_multi_protocol_mode() {
    let env = Env::default();
    env.mock_all_auths();
    let (vault_id, _a, _o, _t, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    client.rebalance_multi(&6_000, &4_000, &0_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #77)")]
fn test_allocation_summing_above_100_percent_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, 100_000_000);

    client.rebalance_multi(&6_000, &5_000, &0_i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #77)")]
fn test_allocation_leg_above_10000_bps_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, 100_000_000);

    client.rebalance_multi(&10_001, &0, &0_i128);
}

#[test]
fn test_allocation_summing_below_100_percent_keeps_remainder_idle() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, amount);

    // 50% Blend, 25% DEX → 25% deliberately idle.
    client.rebalance_multi(&5_000, &2_500, &0_i128);

    assert_eq!(client.get_allocation(), (5_000, 2_500));
    assert_eq!(client.get_deployed_to_blend(), amount / 2);
    assert_eq!(client.get_deployed_to_dex(), amount / 4);
    assert_eq!(client.get_idle_balance(), amount / 4);
}

// ===========================================================================
// SIMULTANEOUS DEPLOYMENT
// ===========================================================================

#[test]
fn test_rebalance_multi_60_40_split_deploys_to_both_protocols() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, amount);

    client.rebalance_multi(&6_000, &4_000, &0_i128);

    assert_eq!(client.get_allocation(), (6_000, 4_000));
    assert_eq!(client.get_deployed_to_blend(), 60_000_000);
    assert_eq!(client.get_deployed_to_dex(), 40_000_000);
    assert_eq!(client.get_protocol_breakdown(), (60_000_000, 40_000_000));
    assert_eq!(client.get_idle_balance(), 0);

    // Both venues funded → the legacy summary symbol reports "multi".
    assert_eq!(client.get_current_protocol(), symbol_short!("multi"));
    // Aggregate deployed assets still reconcile.
    assert_eq!(client.get_deployed_assets(), amount);
    assert_eq!(client.get_asset_breakdown(), (0, amount));
}

#[test]
fn test_rebalance_multi_reallocates_between_protocols() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, amount);

    client.rebalance_multi(&6_000, &4_000, &0_i128);
    assert_eq!(client.get_protocol_breakdown(), (60_000_000, 40_000_000));

    // Shift weight toward the DEX: Blend must be exited down, DEX topped up.
    client.rebalance_multi(&2_500, &7_500, &0_i128);
    assert_eq!(client.get_allocation(), (2_500, 7_500));
    assert_eq!(client.get_protocol_breakdown(), (25_000_000, 75_000_000));

    // And back the other way.
    client.rebalance_multi(&9_000, &1_000, &0_i128);
    assert_eq!(client.get_protocol_breakdown(), (90_000_000, 10_000_000));

    // Total is conserved throughout.
    assert_eq!(client.get_idle_balance() + client.get_deployed_assets(), amount);
}

#[test]
fn test_rebalance_multi_to_zero_zero_exits_both_protocols() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, amount);

    client.rebalance_multi(&6_000, &4_000, &0_i128);
    client.rebalance_multi(&0, &0, &0_i128);

    assert_eq!(client.get_protocol_breakdown(), (0, 0));
    assert_eq!(client.get_idle_balance(), amount);
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
}

#[test]
fn test_rebalance_multi_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, amount);

    client.rebalance_multi(&6_000, &4_000, &0_i128);
    let before = client.get_protocol_breakdown();
    client.rebalance_multi(&6_000, &4_000, &0_i128);

    assert_eq!(client.get_protocol_breakdown(), before);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_rebalance_multi_requires_blend_pool_for_nonzero_blend_allocation() {
    let env = Env::default();
    env.mock_all_auths();

    // Vault with only a DEX pool configured.
    let (vault_id, _agent, owner, usdc) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);
    let dex_pool = env.register_contract(None, MockDexPool);
    client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc, &user, 100_000_000);
    client.enable_multi_protocol(&true);

    client.rebalance_multi(&5_000, &5_000, &0_i128);
}

// ===========================================================================
// PROPORTIONAL WITHDRAWAL
// ===========================================================================

#[test]
fn test_withdrawal_pulls_from_both_protocols_proportionally() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, usdc, _b, _d, user) = setup_multi_funded(&env, amount);
    let token = TestTokenClient::new(&env, &usdc);

    client.rebalance_multi(&6_000, &4_000, &0_i128);
    assert_eq!(client.get_protocol_breakdown(), (60_000_000, 40_000_000));

    // Withdraw half the vault. Nothing is idle, so it must come from the pools.
    let withdraw_amount = 50_000_000_i128;
    client.withdraw(&user, &withdraw_amount);

    assert_eq!(token.balance(&user), withdraw_amount);

    // 50% of the position was pulled, split 60/40 in line with the venues.
    let (blend, dex) = client.get_protocol_breakdown();
    assert_eq!(blend, 30_000_000, "Blend leg must be halved proportionally");
    assert_eq!(dex, 20_000_000, "DEX leg must be halved proportionally");

    // Neither venue was drained to satisfy the other.
    assert!(blend > 0 && dex > 0);
    // The 60/40 ratio survives the redemption.
    assert_eq!(blend * 4, dex * 6);
}

#[test]
fn test_withdraw_all_exits_both_protocols() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, usdc, _b, _d, user) = setup_multi_funded(&env, amount);
    let token = TestTokenClient::new(&env, &usdc);

    client.rebalance_multi(&7_000, &3_000, &0_i128);

    let returned = client.withdraw_all(&user);

    assert_eq!(returned, amount);
    assert_eq!(token.balance(&user), amount);
    assert_eq!(client.get_protocol_breakdown(), (0, 0));
    assert_eq!(client.get_shares(&user), 0);
}

#[test]
fn test_partial_withdrawal_smaller_than_idle_does_not_touch_protocols() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, user) = setup_multi_funded(&env, amount);

    // Leave 20% idle.
    client.rebalance_multi(&5_000, &3_000, &0_i128);
    let before = client.get_protocol_breakdown();
    assert_eq!(client.get_idle_balance(), 20_000_000);

    client.withdraw(&user, &10_000_000_i128);

    assert_eq!(
        client.get_protocol_breakdown(),
        before,
        "an idle-satisfiable withdrawal must not disturb the deployed legs"
    );
}

#[test]
fn test_multi_user_withdrawals_preserve_allocation_ratio() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _a, _o, usdc, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc, &alice, 60_000_000);
    mint_and_deposit(&env, &client, &usdc, &bob, 40_000_000);

    client.enable_multi_protocol(&true);
    client.rebalance_multi(&5_000, &5_000, &0_i128);
    assert_eq!(client.get_protocol_breakdown(), (50_000_000, 50_000_000));

    client.withdraw(&alice, &20_000_000_i128);
    let (b1, d1) = client.get_protocol_breakdown();
    assert_eq!(b1, d1, "50/50 ratio must hold after alice's withdrawal");

    client.withdraw(&bob, &20_000_000_i128);
    let (b2, d2) = client.get_protocol_breakdown();
    assert_eq!(b2, d2, "50/50 ratio must hold after bob's withdrawal");

    assert_eq!(b2 + d2 + client.get_idle_balance(), 60_000_000);
}

// ===========================================================================
// PER-PROTOCOL APY / COMPOSITE YIELD
// ===========================================================================

#[test]
fn test_per_protocol_apy_is_tracked_independently() {
    let env = Env::default();
    env.mock_all_auths();
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, 100_000_000);

    client.set_protocol_apy(&symbol_short!("blend"), &800);
    client.set_protocol_apy(&symbol_short!("dex"), &1_200);

    assert_eq!(client.get_protocol_apy(&symbol_short!("blend")), 800);
    assert_eq!(client.get_protocol_apy(&symbol_short!("dex")), 1_200);
}

#[test]
fn test_composite_apy_is_allocation_weighted() {
    let env = Env::default();
    env.mock_all_auths();
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, 100_000_000);

    client.set_protocol_apy(&symbol_short!("blend"), &800);
    client.set_protocol_apy(&symbol_short!("dex"), &1_200);
    client.rebalance_multi(&6_000, &4_000, &0_i128);

    // 0.6 * 800 + 0.4 * 1200 = 480 + 480 = 960
    assert_eq!(client.get_composite_apy(), 960);
}

#[test]
fn test_composite_apy_dilutes_with_idle_capital() {
    let env = Env::default();
    env.mock_all_auths();
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, 100_000_000);

    client.set_protocol_apy(&symbol_short!("blend"), &800);
    // Only half the vault is deployed → half the yield.
    client.rebalance_multi(&5_000, &0, &0_i128);

    assert_eq!(client.get_composite_apy(), 400);
}

#[test]
fn test_composite_apy_in_single_protocol_mode_uses_active_venue() {
    let env = Env::default();
    env.mock_all_auths();
    let (vault_id, _a, _o, usdc, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc, &user, 100_000_000);

    client.set_protocol_apy(&symbol_short!("blend"), &750);
    client.rebalance(&symbol_short!("blend"), &750, &0_i128);

    assert!(!client.is_multi_protocol_enabled());
    assert_eq!(client.get_composite_apy(), 750);
}

#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn test_set_protocol_apy_rejects_unknown_protocol() {
    let env = Env::default();
    env.mock_all_auths();
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, 100_000_000);

    client.set_protocol_apy(&symbol_short!("aave"), &500);
}

// ===========================================================================
// EVENTS
// ===========================================================================

#[test]
fn test_rebalance_multi_emits_protocol_allocation_changed_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, 100_000_000);

    client.rebalance_multi(&6_000, &4_000, &0_i128);

    let events = find_events_by_topic(env.events().all(), &env, symbol_short!("alloc_chg"));
    assert!(
        !events.is_empty(),
        "ProtocolAllocationChangedEvent must be emitted on allocation change"
    );
}

#[test]
fn test_enable_multi_protocol_emits_mode_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (vault_id, _a, _o, _t, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    client.enable_multi_protocol(&true);

    let events = find_events_by_topic(env.events().all(), &env, symbol_short!("multi_md"));
    assert!(!events.is_empty(), "MultiProtocolModeChangedEvent must be emitted");
}

#[test]
fn test_set_protocol_apy_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let (_vault_id, client, _usdc, _b, _d, _user) = setup_multi_funded(&env, 100_000_000);

    client.set_protocol_apy(&symbol_short!("blend"), &800);

    let events = find_events_by_topic(env.events().all(), &env, symbol_short!("apy_upd"));
    assert!(!events.is_empty(), "ProtocolApyUpdatedEvent must be emitted");
}

// ===========================================================================
// BACKWARD COMPATIBILITY
// ===========================================================================

#[test]
fn test_legacy_single_protocol_rebalance_still_works() {
    let env = Env::default();
    env.mock_all_auths();
    let (vault_id, _a, _o, usdc, _b, _d) = setup_multi(&env);
    let client = NeuroWealthVaultClient::new(&env, &vault_id);

    let user = Address::generate(&env);
    let amount = 100_000_000_i128;
    mint_and_deposit(&env, &client, &usdc, &user, amount);

    client.rebalance(&symbol_short!("blend"), &800, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(client.get_deployed_to_blend(), amount);
    assert_eq!(client.get_deployed_to_dex(), 0);

    // Legacy switch remains mutually exclusive.
    client.rebalance(&symbol_short!("dex"), &900, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("dex"));
    assert_eq!(client.get_deployed_to_blend(), 0);
    assert_eq!(client.get_deployed_to_dex(), amount);
}

#[test]
fn test_solvency_invariant_holds_across_multi_protocol_cycle() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 100_000_000_i128;
    let (_vault_id, client, _usdc, _b, _d, user) = setup_multi_funded(&env, amount);

    let splits = [(6_000u32, 4_000u32), (2_000, 8_000), (10_000, 0), (0, 10_000), (3_333, 3_333)];
    for (b, d) in splits.iter() {
        client.rebalance_multi(b, d, &0_i128);
        let (blend, dex) = client.get_protocol_breakdown();
        let idle = client.get_idle_balance();
        assert_eq!(
            idle + blend + dex,
            amount,
            "solvency invariant violated at split {}/{}",
            b,
            d
        );
        assert!(blend >= 0 && dex >= 0);
    }

    // Everything is still redeemable.
    let returned = client.withdraw_all(&user);
    assert_eq!(returned, amount);
}
