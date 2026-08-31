//! Tests for the agent-callable `harvest()` entrypoint (Issue #465 / PR #491).
//!
//! `harvest()` withdraws the vault's full position from `CurrentProtocol` and
//! immediately re-supplies it, compounding accrued yield in place. It is gated
//! by agent auth (unlike `emergency_harvest`, which is owner-gated) and shares
//! the rebalance cooldown window.
//!
//! Acceptance criteria covered here:
//!   1. Success path: withdraws and re-supplies to CurrentProtocol, emits
//!      `HarvestEvent` with the correct protocol and `amount_harvested`.
//!   2. Rejects when the vault is paused (`VaultError::Paused`, #35).
//!   3. Rejects when the caller is not the agent.
//!   4. Rejects a negative `min_out` (`VaultError::MinOutMustBeNonNegative`, #16).
//!   5. Rejects when `CurrentProtocol` is `"none"` (`VaultError::UnsupportedProtocol`, #17).
//!   6. Rejects when called before the rebalance cooldown elapses
//!      (`VaultError::RebalanceCooldownActive`, #43).
//!
//! There was previously no dedicated `test_harvest.rs`; harvest was only
//! exercised incidentally (budget, pause matrix, cooldown, concurrent).

extern crate std;

use super::utils::*;
use crate::{HarvestEvent, TOPIC_HARVEST};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, TryFromVal,
};

// ===========================================================================
// Helpers
// ===========================================================================

/// Sets up a vault with Blend, deposits `amount`, and deploys the full
/// position so harvest has an active protocol to compound against.
fn setup_with_deployed_blend(
    env: &Env,
    amount: i128,
) -> (Address, Address, Address, Address, Address) {
    let (contract_id, agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(env);
    let client = NeuroWealthVaultClient::new(env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, amount);
    client.rebalance(&symbol_short!("blend"), &850_i128, &0_i128);

    (contract_id, agent, owner, usdc_token, blend_pool)
}

/// Sets up a vault with a DEX pool, deposits `amount`, and deploys the full
/// position so harvest can be exercised against the DEX path.
fn setup_with_deployed_dex(
    env: &Env,
    amount: i128,
) -> (Address, Address, Address, Address, Address) {
    let (contract_id, agent, owner, usdc_token, dex_pool) =
        setup_vault_with_token_and_dex(env);
    let client = NeuroWealthVaultClient::new(env, &contract_id);

    client.set_dex_pool(&owner, &dex_pool);

    let user = Address::generate(env);
    mint_and_deposit(env, &client, &usdc_token, &user, amount);
    client.rebalance(&symbol_short!("dex"), &850_i128, &0_i128);

    (contract_id, agent, owner, usdc_token, dex_pool)
}

fn decode_harvest_event(env: &Env) -> HarvestEvent {
    let events = find_events_by_topic(env.events().all(), env, TOPIC_HARVEST);
    assert_eq!(
        events.len(),
        1,
        "exactly one HarvestEvent expected, got {}",
        events.len()
    );
    let (_, _, data) = &events[0];
    HarvestEvent::try_from_val(env, data).expect("HarvestEvent decode failed")
}

// ===========================================================================
// AC-1: Success path — withdraw + re-supply + HarvestEvent
// ===========================================================================

/// Harvesting a Blend position withdraws the full deployed amount and
/// immediately re-supplies it. The vault ends with the same protocol
/// allocation and emits `HarvestEvent { protocol: "blend", amount_harvested }`.
#[test]
fn test_harvest_withdraws_and_resupplies_blend() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit_amount = 10_000_000_i128;
    let (contract_id, _agent, _owner, usdc_token, blend_pool) =
        setup_with_deployed_blend(&env, deposit_amount);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let blend_client = MockBlendPoolClient::new(&env, &blend_pool);

    // Pre-harvest: everything is sitting in Blend.
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));
    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(blend_client.supplied(&usdc_token), deposit_amount);
    assert_eq!(token_client.balance(&blend_pool), deposit_amount);

    client.harvest(&0_i128);

    // Post-harvest: funds must be back in Blend (withdraw + re-supply).
    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("blend"),
        "harvest must re-supply to CurrentProtocol, leaving protocol unchanged"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        0,
        "vault idle balance must be zero after harvest re-supply"
    );
    assert_eq!(
        blend_client.supplied(&usdc_token),
        deposit_amount,
        "Blend must hold the full harvested amount after re-supply"
    );
    assert_eq!(token_client.balance(&blend_pool), deposit_amount);

    let event = decode_harvest_event(&env);
    assert_eq!(event.protocol, symbol_short!("blend"));
    assert_eq!(
        event.amount_harvested, deposit_amount,
        "amount_harvested must equal the withdrawn (and re-supplied) position"
    );
}

/// Harvesting a DEX position follows the same withdraw-then-re-supply
/// contract and emits `HarvestEvent` with `protocol == "dex"`.
#[test]
fn test_harvest_withdraws_and_resupplies_dex() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit_amount = 12_000_000_i128;
    let (contract_id, _agent, _owner, usdc_token, dex_pool) =
        setup_with_deployed_dex(&env, deposit_amount);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);
    let token_client = TestTokenClient::new(&env, &usdc_token);
    let dex_client = MockDexPoolClient::new(&env, &dex_pool);

    assert_eq!(client.get_current_protocol(), symbol_short!("dex"));
    assert_eq!(token_client.balance(&contract_id), 0);
    assert_eq!(
        dex_client.balance(&usdc_token, &contract_id),
        deposit_amount
    );

    client.harvest(&0_i128);

    assert_eq!(
        client.get_current_protocol(),
        symbol_short!("dex"),
        "harvest must re-supply to the DEX protocol"
    );
    assert_eq!(
        token_client.balance(&contract_id),
        0,
        "vault idle balance must be zero after DEX harvest re-supply"
    );
    assert_eq!(
        dex_client.balance(&usdc_token, &contract_id),
        deposit_amount,
        "DEX must hold the full harvested amount after re-supply"
    );

    let event = decode_harvest_event(&env);
    assert_eq!(event.protocol, symbol_short!("dex"));
    assert_eq!(event.amount_harvested, deposit_amount);
}

/// A successful harvest updates `LastRebalanceLedger` to the current ledger
/// so the shared cooldown window applies to the next harvest/rebalance.
#[test]
fn test_harvest_updates_last_rebalance_ledger() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let before = env.ledger().sequence();
    client.harvest(&0_i128);

    let stored = client.get_last_rebalance_ledger();
    assert!(
        stored >= before,
        "LastRebalanceLedger ({stored}) should be >= ledger at harvest time ({before})"
    );
}

/// Harvesting a Blend position that currently holds zero funds still
/// succeeds and emits `HarvestEvent` with `amount_harvested == 0`. This
/// covers the noop-rebalance-then-harvest path (`CurrentProtocol == blend`
/// but no deployed balance).
#[test]
fn test_harvest_zero_position_emits_zero_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);
    // No deposit — rebalance is a noop that still records CurrentProtocol.
    client.rebalance(&symbol_short!("blend"), &500_i128, &0_i128);
    assert_eq!(client.get_current_protocol(), symbol_short!("blend"));

    client.harvest(&0_i128);

    let event = decode_harvest_event(&env);
    assert_eq!(event.protocol, symbol_short!("blend"));
    assert_eq!(event.amount_harvested, 0_i128);
}

// ===========================================================================
// AC-2: Rejects when vault is paused
// ===========================================================================

#[test]
#[should_panic(expected = "Error(Contract, #35)")]
fn test_harvest_rejects_when_paused() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);
    assert!(client.is_paused());

    client.harvest(&0_i128);
}

/// `try_harvest` returns the typed `Paused` error so callers can distinguish
/// it from other rejections (auth, cooldown, protocol).
#[test]
fn test_harvest_while_paused_returns_paused_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.pause(&owner);

    assert_eq!(
        client.try_harvest(&0_i128),
        Err(Ok(soroban_sdk::Error::from_contract_error(35))),
        "harvest must return VaultError::Paused (#35) while paused"
    );

    // No harvest event may leak from a rejected call.
    let events = find_events_by_topic(env.events().all(), &env, TOPIC_HARVEST);
    assert!(
        events.is_empty(),
        "no HarvestEvent may be emitted when harvest is rejected"
    );
}

// ===========================================================================
// AC-3: Rejects when caller is not the agent
// ===========================================================================

/// With no authorization available, `harvest` must fail because
/// `require_is_agent` cannot satisfy `agent.require_auth()`. Guards against
/// a dropped auth check (same style as `test_rebalance_requires_agent_auth`).
#[test]
fn test_harvest_requires_agent_auth() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[]);

    let result = client.try_harvest(&0_i128);
    assert!(
        result.is_err(),
        "harvest must fail without the agent's authorization"
    );

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_HARVEST);
    assert!(
        events.is_empty(),
        "no HarvestEvent may be emitted when harvest is unauthorized"
    );
}

/// A non-agent signer that *does* hold a valid signature for `harvest` is
/// still rejected: `require_is_agent` demands the stored agent's auth, not
/// an arbitrary caller's.
#[test]
fn test_harvest_rejects_non_agent_signer() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let attacker = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "harvest",
            args: (0_i128,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    let result = client.try_harvest(&0_i128);
    assert!(
        result.is_err(),
        "harvest must require the stored agent's authorization, not another signer's"
    );
}

/// Positive counterpart: with auth scoped to *only* the agent for *only*
/// `harvest`, the call succeeds. Proves the entrypoint accepts the agent's
/// authorization.
#[test]
fn test_agent_harvest_with_scoped_auth() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    env.mock_auths(&[MockAuth {
        address: &agent,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "harvest",
            args: (0_i128,).into_val(&env),
            sub_invokes: &[],
        },
    }]);

    client.harvest(&0_i128);

    let event = decode_harvest_event(&env);
    assert_eq!(event.protocol, symbol_short!("blend"));
    assert_eq!(event.amount_harvested, 10_000_000_i128);
}

// ===========================================================================
// AC-4: Rejects negative min_out
// ===========================================================================

#[test]
#[should_panic(expected = "Error(Contract, #16)")]
fn test_harvest_rejects_negative_min_out() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.harvest(&-1_i128);
}

/// `try_harvest` surfaces `MinOutMustBeNonNegative` (#16) for any negative
/// slippage floor, including values far below -1.
#[test]
fn test_harvest_negative_min_out_returns_typed_error() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(
        client.try_harvest(&-1_i128),
        Err(Ok(soroban_sdk::Error::from_contract_error(16))),
        "harvest(-1) must return VaultError::MinOutMustBeNonNegative (#16)"
    );
    assert_eq!(
        client.try_harvest(&i128::MIN),
        Err(Ok(soroban_sdk::Error::from_contract_error(16))),
        "harvest(i128::MIN) must return VaultError::MinOutMustBeNonNegative (#16)"
    );

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_HARVEST);
    assert!(
        events.is_empty(),
        "no HarvestEvent may be emitted for a negative min_out"
    );
}

// ===========================================================================
// AC-5: Rejects when CurrentProtocol is none (UnsupportedProtocol)
// ===========================================================================

#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn test_harvest_rejects_when_protocol_is_none() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Fresh vault: CurrentProtocol defaults to "none".
    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    client.harvest(&0_i128);
}

/// Harvest also rejects after an explicit rebalance back to `"none"`.
#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn test_harvest_rejects_after_rebalance_to_none() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, usdc_token, blend_pool) =
        setup_vault_with_token_and_blend(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_blend_pool(&owner, &blend_pool);
    let user = Address::generate(&env);
    mint_and_deposit(&env, &client, &usdc_token, &user, 10_000_000_i128);
    client.rebalance(&symbol_short!("blend"), &850_i128, &0_i128);
    client.rebalance(&symbol_short!("none"), &0_i128, &0_i128);

    assert_eq!(client.get_current_protocol(), symbol_short!("none"));
    client.harvest(&0_i128);
}

#[test]
fn test_harvest_none_protocol_returns_unsupported_protocol() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    assert_eq!(
        client.try_harvest(&0_i128),
        Err(Ok(soroban_sdk::Error::from_contract_error(17))),
        "harvest with CurrentProtocol=none must return UnsupportedProtocol (#17)"
    );

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_HARVEST);
    assert!(
        events.is_empty(),
        "no HarvestEvent may be emitted when CurrentProtocol is none"
    );
}

// ===========================================================================
// AC-6: Rejects when called before the rebalance cooldown elapses
// ===========================================================================

/// Calling harvest immediately after a rebalance that wrote
/// `LastRebalanceLedger`, with a non-zero cooldown, must panic with
/// `RebalanceCooldownActive` (#43).
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_harvest_rejects_before_cooldown_elapses() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // setup_with_deployed_blend already rebalanced (and wrote LastRebalanceLedger)
    // in this same ledger. Enable a cooldown so the subsequent harvest is blocked.
    client.set_rebalance_cooldown(&10_u32);
    client.harvest(&0_i128);
}

/// Two harvests in the same ledger with cooldown > 0: the first succeeds
/// (after advancing past the rebalance's window) and the second panics.
#[test]
#[should_panic(expected = "Error(Contract, #43)")]
fn test_second_harvest_within_cooldown_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    // Advance past the rebalance's LastRebalanceLedger so the first harvest
    // is not blocked by the setup rebalance.
    let last = client.get_last_rebalance_ledger();
    env.ledger().with_mut(|li| {
        li.sequence_number = last + 20;
    });

    client.set_rebalance_cooldown(&10_u32);
    client.harvest(&0_i128);
    // Same ledger — elapsed == 0 < 10.
    client.harvest(&0_i128);
}

/// `try_harvest` returns the typed cooldown error and does not update
/// `LastRebalanceLedger` or emit a harvest event.
#[test]
fn test_harvest_within_cooldown_returns_error_without_side_effects() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, 10_000_000_i128);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    client.set_rebalance_cooldown(&10_u32);
    let ledger_before = client.get_last_rebalance_ledger();

    assert_eq!(
        client.try_harvest(&0_i128),
        Err(Ok(soroban_sdk::Error::from_contract_error(43))),
        "harvest within cooldown must return RebalanceCooldownActive (#43)"
    );

    assert_eq!(
        client.get_last_rebalance_ledger(),
        ledger_before,
        "LastRebalanceLedger must not change when the cooldown check fails"
    );

    let events = find_events_by_topic(env.events().all(), &env, TOPIC_HARVEST);
    assert!(
        events.is_empty(),
        "no HarvestEvent may be emitted when harvest is blocked by cooldown"
    );
}

/// After the cooldown window elapses, harvest succeeds again.
#[test]
fn test_harvest_succeeds_after_cooldown_elapses() {
    let env = Env::default();
    env.mock_all_auths();

    let deposit_amount = 10_000_000_i128;
    let (contract_id, _agent, _owner, _usdc_token, _blend_pool) =
        setup_with_deployed_blend(&env, deposit_amount);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let interval = 5_u32;
    client.set_rebalance_cooldown(&interval);
    let last = client.get_last_rebalance_ledger();

    env.ledger().with_mut(|li| {
        li.sequence_number = last + interval;
    });

    client.harvest(&0_i128);

    let event = decode_harvest_event(&env);
    assert_eq!(event.protocol, symbol_short!("blend"));
    assert_eq!(event.amount_harvested, deposit_amount);

    let new_last = client.get_last_rebalance_ledger();
    assert!(
        new_last >= last + interval,
        "LastRebalanceLedger should advance after a successful post-cooldown harvest"
    );
}
