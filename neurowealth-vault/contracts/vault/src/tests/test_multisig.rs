//! Issue #464 — multi-sig support for owner operations.
//!
//! Approach 1 (external multi-sig wrapper): the multi-sig contract's address
//! becomes the vault's `Owner` via `set_multisig`; owner-gated operations are
//! executed as multi-sig proposals. The timelock flows and pause/unpause
//! mechanism are unchanged — only the authorizing party changes.

use super::utils::*;
use crate::{MultisigSetEvent, TOPIC_MULTISIG_SET};
use neurowealth_multisig::{
    MultisigContract, MultisigContractClient, MultisigError, MultisigProposal,
    ProposalStatus,
};
use soroban_sdk::{
    testutils::Address as _,
    vec, Address, Env, IntoVal, Symbol, Val, Vec,
};

/// Two signers, threshold 2 — a conservative 2-of-2 production setup.
fn setup_multisig(env: &Env, signer_a: &Address, signer_b: &Address) -> (Address, MultisigContractClient<'static>) {
    let multisig_id = env.register(MultisigContract, ());
    let multisig_client = MultisigContractClient::new(env, &multisig_id);
    multisig_client.initialize(
        signer_a,
        &vec![env, signer_a.clone(), signer_b.clone()],
        &2u32,
    );
    (multisig_id, multisig_client)
}

fn multisig_set_event_count(env: &Env, contract_id: &Address) -> usize {
    let events = env.events().all();
    let topic = Symbol::new(env, "msig_set");
    let mut count = 0;
    for (addr, topics, _data) in events.iter() {
        if *addr == *contract_id && topics.len() > 0 && topics.get(0).unwrap() == topic.into_val(env) {
            count += 1;
        }
    }
    count
}

#[test]
fn set_multisig_migrates_governance_to_the_multisig_contract() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, agent, owner, usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    // The multisig contract (its signers are the governance committee).
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let (multisig_id, multisig_client) = setup_multisig(&env, &signer_a, &signer_b);

    // Single-sig → multi-sig migration.
    vault_client.set_multisig(&owner, &multisig_id);

    // The vault's owner is now the multisig contract.
    assert_eq!(vault_client.get_owner(), multisig_id);
    assert_eq!(vault_client.get_multisig(), Some(multisig_id.clone()));
    assert!(multisig_set_event_count(&env, &vault_id) >= 1);
    let _ = usdc_token;
    let _ = agent;
}

#[test]
fn set_multisig_requires_the_current_owner() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, _owner, _usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let (_multisig_id, multisig_client) = setup_multisig(&env, &signer_a, &signer_b);

    let impostor = Address::generate(&env);
    let result = vault_client.try_set_multisig(&impostor, &multisig_client.get_admin());
    assert!(result.is_err());
}

#[test]
fn set_multisig_rejects_an_account_address_governor() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    // An account address cannot authorize as a contract in cross-contract
    // invocations, so governance would be lost — reject it.
    let account_governor = Address::generate(&env);
    let result = vault_client.try_set_multisig(&owner, &account_governor);
    assert!(result.is_err());
}

#[test]
fn set_multisig_rejects_the_current_owner_as_target() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let result = vault_client.try_set_multisig(&owner, &owner);
    assert!(result.is_err());
}

#[test]
fn set_multisig_cannot_be_called_twice() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let (multisig_id, _multisig_client) = setup_multisig(&env, &signer_a, &signer_b);

    vault_client.set_multisig(&owner, &multisig_id);

    // A second migration is refused: governance is now held by the contract.
    let another_multisig = env.register(MultisigContract, ());
    let result = vault_client.try_set_multisig(&owner, &another_multisig);
    assert!(result.is_err());

    // Owner is still the first multisig.
    assert_eq!(vault_client.get_owner(), multisig_id);
}

#[test]
fn multisig_governor_can_pause_and_unpause_the_vault() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let (multisig_id, multisig_client) = setup_multisig(&env, &signer_a, &signer_b);

    vault_client.set_multisig(&owner, &multisig_id);

    // Pause proposal: pause(multisig_id) forwarded to the vault.
    let proposal_id = multisig_client.propose(
        &signer_a,
        &vault_id,
        &Symbol::new(&env, "pause"),
        &vec![&env, multisig_id.into_val(&env)],
    );
    multisig_client.approve(&signer_b, &proposal_id);
    multisig_client.execute(&proposal_id);

    // The vault is paused, and only the multisig contract can unpause.
    assert!(vault_client.is_paused());

    let unpause_proposal = multisig_client.propose(
        &signer_a,
        &vault_id,
        &Symbol::new(&env, "unpause"),
        &vec![&env, multisig_id.into_val(&env)],
    );
    multisig_client.approve(&signer_b, &unpause_proposal);
    multisig_client.execute(&unpause_proposal);

    assert!(!vault_client.is_paused());
}

#[test]
fn multisig_requires_the_signer_threshold_before_execution() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, _usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    // 3 signers, threshold 2: one signature is not enough.
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let signer_c = Address::generate(&env);
    let (multisig_id, multisig_client) = setup_multisig(&env, &signer_a, &signer_b);
    multisig_client.add_signer(&signer_a, &signer_c);
    multisig_client.set_threshold(&signer_a, &2u32);

    vault_client.set_multisig(&owner, &multisig_id);

    let proposal_id = multisig_client.propose(
        &signer_a,
        &vault_id,
        &Symbol::new(&env, "pause"),
        &vec![&env, multisig_id.into_val(&env)],
    );
    // Only the proposer signed (1 of 2) — execution must fail.
    let execute_result = multisig_client.try_execute(&proposal_id);
    assert!(execute_result.is_err());

    // Second signature unlocks execution.
    multisig_client.approve(&signer_b, &proposal_id);
    multisig_client.execute(&proposal_id);
    assert!(vault_client.is_paused());
}

#[test]
fn timelock_flows_are_preserved_under_multisig_governance() {
    let env = Env::default();
    env.mock_all_auths();

    let (vault_id, _agent, owner, usdc_token) = setup_vault_with_token(&env);
    let vault_client = NeuroWealthVaultClient::new(&env, &vault_id);

    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let (multisig_id, multisig_client) = setup_multisig(&env, &signer_a, &signer_b);

    vault_client.set_multisig(&owner, &multisig_id);

    // Two-step agent update through the multisig: propose → approve → execute
    // both `update_agent` and then `confirm_agent_update`.
    let new_agent = Address::generate(&env);
    let update_proposal = multisig_client.propose(
        &signer_a,
        &vault_id,
        &Symbol::new(&env, "update_agent"),
        &vec![&env, new_agent.into_val(&env)],
    );
    multisig_client.approve(&signer_b, &update_proposal);
    multisig_client.execute(&update_proposal);

    let confirm_proposal = multisig_client.propose(
        &signer_a,
        &vault_id,
        &Symbol::new(&env, "confirm_agent_update"),
        &Vec::new(&env),
    );
    multisig_client.approve(&signer_b, &confirm_proposal);
    multisig_client.execute(&confirm_proposal);

    // The timelock flow completed: the vault's agent is the new agent.
    assert_eq!(vault_client.get_agent(), new_agent);
}
