# Protocol Risk Scoring Model

NeuroWealth AI Agent uses a multi-dimensional risk scoring engine to evaluate and filter DeFi protocols before capital allocation.

---

## Risk Dimensions (1-100 Scale, Lower is Safer)

| Dimension | Default Weight | Key Evaluation Factors |
| --- | --- | --- |
| **Smart Contract Risk** | 30% | Audit status, formal verification, time in production, code complexity |
| **Liquidity & TVL Risk** | 25% | Pool TVL depth, 24h volume/liquidity ratio, withdrawal queue size |
| **Oracle Risk** | 20% | Price feed redundancy, update latency, DEX pool manipulation resistance |
| **Governance Risk** | 15% | Timelock delays on upgrades, multisig threshold, voting decentralization |
| **Centralization Risk** | 10% | Admin keys, pause triggers, privileged parameter modification rights |

---

## Composite Score Calculation

$$\text{Composite Risk} = \sum_{i} (\text{Score}_i \times \text{Weight}_i)$$

- **Low Risk (0 - 30)**: Eligible for Conservative, Balanced, and Growth strategies. Max allocation 50% TVL.
- **Medium Risk (31 - 60)**: Eligible for Balanced and Growth strategies. Max allocation 30% TVL.
- **High Risk (61 - 80)**: Eligible only for Growth strategy with strict 10% TVL cap.
- **Critical Risk (81 - 100)**: Prohibited from all strategies. Triggers immediate rebalance exit alert.
