# Insurance Fund

## Overview
The vault maintains an insurance fund to cover potential losses from protocol failures or smart contract exploits. A configurable portion of yield is allocated to the fund. When an incident occurs, the fund backstops affected users up to a configurable maximum payout per incident.

## Configuration
- `contribution_rate_bps`: Contribution rate in basis points (e.g. 500 = 5% of yield).
- `max_payout_per_incident`: Maximum amount paid out per incident (raw units).
- `min_threshold`: Minimum fund balance; alerts are raised when balance falls below this.

These are owner-controlled and stored on-chain.

## Contribution
On each harvest/rebalance, `calculate_contribution` is applied to the yield earned. The amount is transferred to the insurance balance.

## Payout
When a protocol incident occurs, the owner (or governance) can trigger a payout. The payout is capped by the fund balance and the maximum per incident.

## Monitoring
The monitoring service checks the fund balance against `min_threshold` and emits an alert if it falls.

## Event
`InsuranceFundUpdatedEvent` is emitted whenever the fund config or balance changes.
Topic: `ins_fund`.

## Future Enhancements
- Integration with an external insurance protocol
- User-triggered claims
