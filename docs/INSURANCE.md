# Insurance Fund (Design Proposal)

> [!IMPORTANT]
> **Status**: Architectural Proposal / Planned Feature (Unimplemented)
>
> The insurance fund described in this specification is **not currently implemented in the live vault contract** (`neurowealth-vault/src/lib.rs`). In the current release, no on-chain fund allocation is performed during harvest/rebalance, and `TOPIC_INSURANCE_FUND_UPDATED` is not published. This document serves as a design proposal for future protocol phases. (Reference: Issue #1043)

## Overview
In this proposed design, the vault would maintain an insurance fund to backstop potential losses from external protocol failures or smart contract exploits. A configurable portion of yield would be allocated to the fund. When an incident occurs, the fund would cover affected users up to a configurable maximum payout per incident.

## Proposed Configuration
- `contribution_rate_bps`: Contribution rate in basis points (e.g. 500 = 5% of yield).
- `max_payout_per_incident`: Maximum amount paid out per incident (raw units).
- `min_threshold`: Minimum fund balance; alerts are raised when balance falls below this.

These parameters are intended to be owner-controlled and stored in contract storage once implemented.

## Proposed Contribution Flow
On each harvest/rebalance, `calculate_contribution` would be applied to the yield earned. The calculated amount would be transferred to the insurance balance.

## Proposed Payout Flow
When a protocol incident occurs, the owner (or governance) would trigger a payout. The payout would be capped by the fund balance and the maximum per incident.

## Proposed Monitoring
The monitoring service would check the fund balance against `min_threshold` and emit an alert if it falls below the minimum.

## Proposed Event Specification
An `InsuranceFundUpdatedEvent` would be emitted whenever the fund config or balance changes.
Topic: `ins_fund`.

## Future Enhancements
- Integration with an external insurance protocol
- User-triggered claims

