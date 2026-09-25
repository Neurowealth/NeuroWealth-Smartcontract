//! NeuroWealth multi-sig governor (issue #464).
//!
//! An external M-of-N multi-sig contract that can be set as the vault's
//! owner via [`Vault::set_multisig`] (approach 1 from the issue: external
//! multi-sig wrapper). The vault's stored `Owner` becomes this contract's
//! address; every owner-gated vault operation (`pause`, `unpause`,
//! `schedule_upgrade`, `execute_upgrade`, `update_agent`, …) is then routed
//! through `execute` here, which gathers M signer approvals first and
//! forwards the invocation to the vault.
//!
//! Because the vault calls `owner.require_auth()` and `owner == stored_owner`,
//! and the invoker of the vault is this contract (whose own address matches
//! the stored owner), the vault's authorization checks pass unchanged —
//! preserving the two-step timelock flows (`schedule_upgrade` →
//! `execute_upgrade`, `update_agent` → `confirm_agent_update`) and the
//! pause/unpause mechanism exactly as before.
//!
//! Signature flow:
//! 1. `propose` — any signer proposes a call (`target`, `fn_name`, `args`).
//! 2. `approve` — M distinct signers (including the proposer when the
//!    threshold is met) approve the proposal.
//! 3. `execute` — anyone may execute once `approvals >= threshold`; the
//!    governor forwards the call to the target contract.
//!
//! Signers and threshold are managed on-chain (`add_signer`, `remove_signer`,
//! `set_threshold`), so signer rotation never requires a vault migration.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, Env,
    Symbol, Val, Vec,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Signers,
    Threshold,
    Proposals,
    NextProposalId,
    ProposalSigners(u32),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MultisigError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    ThresholdTooHigh = 4,
    ThresholdBelowMinimum = 5,
    DuplicateSigner = 6,
    SignerNotFound = 7,
    LastSignerRemoval = 8,
    ProposalNotFound = 9,
    ProposalExpired = 10,
    AlreadySigned = 11,
    ThresholdNotMet = 12,
    EmptySigners = 13,
    NothingToExecute = 14,
}

/// Default proposal expiry in ledgers (~2 days at 5s/ledger). Proposals not
/// executed within the window can be cancelled by any signer.
pub const PROPOSAL_TTL_LEDGERS: u32 = 34_560;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Executed,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct MultisigProposal {
    pub proposal_id: u32,
    pub target: Address,
    pub fn_name: Symbol,
    /// Serialized arguments forwarded to `target` on execution.
    pub args: Vec<Val>,
    pub approvals: u32,
    pub threshold: u32,
    pub proposed_by: Address,
    pub proposed_ledger: u32,
    pub status: ProposalStatus,
}

#[contract]
pub struct MultisigContract;

#[contractimpl]
impl MultisigContract {
    /// One-time setup: `signers` is the initial N-set and `threshold` is the
    /// required M-of-N. Duplicates are rejected.
    pub fn initialize(env: Env, admin: Address, signers: Vec<Address>, threshold: u32) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic_with_error!(env, MultisigError::AlreadyInitialized);
        }
        admin.require_auth();
        Self::validate_signer_set(&env, &signers, threshold);

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Signers, &signers);
        env.storage().instance().set(&DataKey::Threshold, &threshold);
        env.storage().instance().set(&DataKey::NextProposalId, &0u32);
    }

    pub fn get_signers(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::NotInitialized))
    }

    pub fn get_threshold(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::NotInitialized))
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::NotInitialized))
    }

    /// Adds a signer. Admin-gated; the threshold must remain satisfiable.
    pub fn add_signer(env: Env, caller: Address, signer: Address) {
        Self::require_admin(&env, &caller);

        let mut signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::NotInitialized));
        let threshold: u32 = env.storage().instance().get(&DataKey::Threshold).unwrap();

        if Self::contains_signer(&signers, &signer) {
            panic_with_error!(env, MultisigError::DuplicateSigner);
        }
        signers.push_back(signer);
        // Keep the threshold satisfiable at all times.
        if threshold > signers.len() {
            panic_with_error!(env, MultisigError::ThresholdTooHigh);
        }
        env.storage().instance().set(&DataKey::Signers, &signers);
    }

    /// Removes a signer. Refuses removal that would leave fewer signers than
    /// the threshold.
    pub fn remove_signer(env: Env, caller: Address, signer: Address) {
        Self::require_admin(&env, &caller);

        let mut signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::NotInitialized));
        let threshold: u32 = env.storage().instance().get(&DataKey::Threshold).unwrap();

        if !Self::contains_signer(&signers, &signer) {
            panic_with_error!(env, MultisigError::SignerNotFound);
        }
        if signers.len() <= threshold {
            panic_with_error!(env, MultisigError::LastSignerRemoval);
        }

        let mut filtered = Vec::new(&env);
        for existing in signers.iter() {
            if existing != signer {
                filtered.push_back(existing);
            }
        }
        env.storage().instance().set(&DataKey::Signers, &filtered);
    }

    /// Adjusts the required approval count (1..=signer_count).
    pub fn set_threshold(env: Env, caller: Address, threshold: u32) {
        Self::require_admin(&env, &caller);

        let signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::NotInitialized));
        Self::validate_threshold(&signers, threshold);

        env.storage().instance().set(&DataKey::Threshold, &threshold);
    }

    /// Proposes an owner operation. `target` is the governed contract (e.g.
    /// the vault), `fn_name`/`args` describe the forwarded call. Any signer
    /// may propose; the proposal also counts as the proposer's approval.
    pub fn propose(
        env: Env,
        caller: Address,
        target: Address,
        fn_name: Symbol,
        args: Vec<Val>,
    ) -> u32 {
        Self::require_signer(&env, &caller);

        let proposal_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextProposalId)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::NextProposalId, &(proposal_id + 1));

        let threshold: u32 = env.storage().instance().get(&DataKey::Threshold).unwrap();
        let proposal = MultisigProposal {
            proposal_id,
            target,
            fn_name,
            args,
            approvals: 1,
            threshold,
            proposed_by: caller.clone(),
            proposed_ledger: env.ledger().sequence(),
            status: ProposalStatus::Pending,
        };
        env.storage()
            .instance()
            .set(&DataKey::Proposals, &Self::upsert(&env, proposal));

        // Record the proposer's approval.
        let approvals = Vec::new(&env);
        approvals.push_back(caller);
        env.storage()
            .instance()
            .set(&DataKey::ProposalSigners(proposal_id), &approvals);

        proposal_id
    }

    /// Approves a pending proposal. One approval per signer per proposal.
    pub fn approve(env: Env, caller: Address, proposal_id: u32) {
        Self::require_signer(&env, &caller);

        let mut proposal = Self::get_proposal(env.clone(), proposal_id);
        if proposal.status != ProposalStatus::Pending {
            panic_with_error!(env, MultisigError::ThresholdNotMet);
        }
        if env.ledger().sequence().saturating_sub(proposal.proposed_ledger)
            > PROPOSAL_TTL_LEDGERS
        {
            proposal.status = ProposalStatus::Expired;
            env.storage()
                .instance()
                .set(&DataKey::Proposals, &Self::upsert(&env, proposal.clone()));
            panic_with_error!(env, MultisigError::ProposalExpired);
        }

        let mut approvals: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ProposalSigners(proposal_id))
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::ProposalNotFound));

        for signer in approvals.iter() {
            if signer == caller {
                panic_with_error!(env, MultisigError::AlreadySigned);
            }
        }

        approvals.push_back(caller);
        proposal.approvals = approvals.len();
        env.storage()
            .instance()
            .set(&DataKey::ProposalSigners(proposal_id), &approvals);
        env.storage()
            .instance()
            .set(&DataKey::Proposals, &Self::upsert(&env, proposal));
    }

    /// Executes a proposal once `threshold` approvals are collected. On
    /// success the proposal is marked `Executed`; the forwarded invocation
    /// runs with the governor's own authority, so the vault's
    /// `owner.require_auth()` (stored owner == this governor) passes.
    pub fn execute(env: Env, proposal_id: u32) {
        let proposal = Self::get_proposal(env.clone(), proposal_id);
        if proposal.status != ProposalStatus::Pending {
            panic_with_error!(env, MultisigError::ThresholdNotMet);
        }

        let approvals: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ProposalSigners(proposal_id))
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::ProposalNotFound));

        // Distinct-signer approval count must meet the threshold.
        let mut distinct: Vec<Address> = Vec::new(&env);
        for signer in approvals.iter() {
            if !Self::contains_signer(&distinct, &signer) {
                distinct.push_back(signer);
            }
        }
        if distinct.len() < proposal.threshold {
            panic_with_error!(env, MultisigError::ThresholdNotMet);
        }

        let mut executed = proposal.clone();
        executed.status = ProposalStatus::Executed;
        env.storage()
            .instance()
            .set(&DataKey::Proposals, &Self::upsert(&env, executed));

        env.invoke_contract::<()>(
            &proposal.target,
            &proposal.fn_name,
            proposal.args,
        );
    }

    pub fn get_proposal(env: Env, proposal_id: u32) -> MultisigProposal {
        let proposals: Vec<MultisigProposal> = env
            .storage()
            .instance()
            .get(&DataKey::Proposals)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::ProposalNotFound));
        Self::find_proposal(&proposals, proposal_id)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::ProposalNotFound))
    }

    /// Cancels a pending proposal. The proposer or the admin may cancel;
    /// expired proposals are cancellable by any signer.
    pub fn cancel(env: Env, caller: Address, proposal_id: u32) {
        Self::require_signer(&env, &caller);

        let proposals: Vec<MultisigProposal> = env
            .storage()
            .instance()
            .get(&DataKey::Proposals)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::ProposalNotFound));
        let mut proposal = Self::find_proposal(&proposals, proposal_id)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::ProposalNotFound));

        let expired = env.ledger().sequence().saturating_sub(proposal.proposed_ledger)
            > PROPOSAL_TTL_LEDGERS;

        if caller != proposal.proposed_by && caller != Self::get_admin(&env) && !expired {
            panic_with_error!(env, MultisigError::Unauthorized);
        }

        proposal.status = ProposalStatus::Expired;
        env.storage()
            .instance()
            .set(&DataKey::Proposals, &Self::upsert(&env, proposal));
    }

    // ── Helpers ─────────────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::NotInitialized));
        if admin != *caller {
            panic_with_error!(env, MultisigError::Unauthorized);
        }
        caller.require_auth();
    }

    fn require_signer(env: &Env, caller: &Address) {
        let signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap_or_else(|| panic_with_error!(env, MultisigError::NotInitialized));
        if !Self::contains_signer(&signers, caller) {
            panic_with_error!(env, MultisigError::Unauthorized);
        }
        caller.require_auth();
    }

    fn contains_signer(signers: &Vec<Address>, signer: &Address) -> bool {
        for existing in signers.iter() {
            if existing == *signer {
                return true;
            }
        }
        false
    }

    fn validate_threshold(signers: &Vec<Address>, threshold: u32) {
        if threshold == 0 {
            panic_with_error!(MultisigError::ThresholdBelowMinimum);
        }
        if threshold > signers.len() {
            panic_with_error!(MultisigError::ThresholdTooHigh);
        }
    }

    fn validate_signer_set(env: &Env, signers: &Vec<Address>, threshold: u32) {
        if signers.len() == 0 {
            panic_with_error!(env, MultisigError::EmptySigners);
        }
        for i in 0..signers.len() {
            for j in (i + 1)..signers.len() {
                if signers.get(i) == signers.get(j) {
                    panic_with_error!(env, MultisigError::DuplicateSigner);
                }
            }
        }
        Self::validate_threshold(signers, threshold);
    }

    fn find_proposal(
        proposals: &Vec<MultisigProposal>,
        proposal_id: u32,
    ) -> Option<MultisigProposal> {
        for proposal in proposals.iter() {
            if proposal.proposal_id == proposal_id {
                return Some(proposal);
            }
        }
        None
    }

    /// Replaces the stored proposal (or appends it) in the proposals vector.
    fn upsert(env: &Env, proposal: MultisigProposal) -> Vec<MultisigProposal> {
        let mut proposals: Vec<MultisigProposal> = env
            .storage()
            .instance()
            .get(&DataKey::Proposals)
            .unwrap_or_else(|| Vec::new(env));
        for i in 0..proposals.len() {
            if proposals.get(i).unwrap().proposal_id == proposal.proposal_id {
                proposals.set(i, proposal);
                return proposals;
            }
        }
        proposals.push_back(proposal);
        proposals
    }
}
