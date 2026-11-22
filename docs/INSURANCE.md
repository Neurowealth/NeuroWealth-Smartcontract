# Insurance Fund (Design Proposal)

> [!WARNING]
> **Status: Design Proposal Only / Not Implemented in Contract**
>
> This document describes a proposed design for an on-chain insurance fund for the NeuroWealth Vault. 
> The current smart contract implementation (`neurowealth-vault/contracts/vault/src/lib.rs`) **does not implement** an active insurance fund:
> - `neurowealth-vault/contracts/vault/src/lib.rs` has zero references to insurance mechanics.
> - `neurowealth-vault/contracts/vault/src/insurance.rs` provides standalone algorithmic calculation helpers and unit tests (`tests/test_insurance_fund.rs`), but its functions (`calculate_contribution`, `apply_payout`) are not wired into the vault's harvest or rebalance flows.
> - The topic symbol `TOPIC_INSURANCE_FUND_UPDATED` (`ins_fund`) is defined in `topics.rs` but is not emitted by any contract methods.
> - References in frontend packages (such as `packages/admin-panel/src/components/InsuranceConfiguration.tsx`) and monitoring service stubs are placeholders for this design proposal and do not manage an active on-chain fund.

---

## Proposed Overview
In this proposed design, the vault would maintain an insurance fund to cover potential losses from protocol failures or smart contract exploits. A configurable portion of yield would be allocated to the fund. When an incident occurs, the fund would backstop affected users up to a configurable maximum payout per incident.

## Proposed Configuration
- `contribution_rate_bps`: Contribution rate in basis points (e.g. 500 = 5% of yield).
- `max_payout_per_incident`: Maximum amount paid out per incident (raw units).
- `min_threshold`: Minimum fund balance; alerts are raised when balance falls below this.

In the proposed architecture, these parameters would be owner-controlled and stored in on-chain instance or persistent storage.

## Proposed Contribution Mechanics
On each harvest or rebalance event, `calculate_contribution` would be applied to the net yield earned. The resulting amount would be transferred into the dedicated insurance reserve balance.

## Proposed Payout Mechanics
When a protocol incident occurs, the contract owner or governance would trigger an authorized payout. The payout would be capped by the current fund balance and the `max_payout_per_incident` threshold.

## Monitoring & Alerting
Off-chain monitoring would check the reported insurance balance against `min_threshold` and trigger alerts whenever the balance drops below safety margins.

## Event Specification (Proposed)
`InsuranceFundUpdatedEvent` would be emitted whenever the fund config or balance changes.
Topic: `ins_fund` (`symbol_short!("ins_fund")`).

## Future Enhancements
- Integration with external decentralized coverage protocols
- Automated claim verification and user-triggered claims
