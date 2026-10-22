#![cfg(test)]
//! Minimal ownership smoke test (#989).
//!
//! This file previously tested a NEAR Protocol contract (`near_sdk`,
//! `Contract::new`, `VMContextBuilder`) with no relationship to this
//! Soroban vault's actual API, so none of its content was salvageable.
//! The two-step ownership-transfer flow it was trying to exercise
//! (`transfer_ownership` / `accept_ownership` / `cancel_ownership_transfer`
//! / `get_pending_owner`) already has thorough coverage in
//! `test_access_control.rs`'s "TWO-STEP OWNERSHIP TRANSFER" section; this
//! file is kept as a lightweight smoke test confirming the basic flow
//! works end to end, not a duplicate of that coverage.

use super::utils::*;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[test]
fn test_ownership_transfer_end_to_end_smoke_test() {
    let env = Env::default();
    env.mock_all_auths();

    let (contract_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let client = NeuroWealthVaultClient::new(&env, &contract_id);

    let new_owner = Address::generate(&env);

    client.transfer_ownership(&new_owner);
    assert_eq!(client.get_pending_owner(), Some(new_owner.clone()));
    assert_eq!(client.get_owner(), owner);

    client.accept_ownership(&new_owner);
    assert_eq!(client.get_owner(), new_owner);
    assert_eq!(client.get_pending_owner(), None);
}
