# Circuit Breaker Design

The NeuroWealth Vault features an automated circuit breaker to protect user funds from anomalous behavior, smart contract exploits, and severe market turbulence.

## Triggers and Thresholds

The circuit breaker automatically pauses the vault when any of the following conditions are met:

1. **TVL Drop (Anomalous Withdrawals)**: 
   - Threshold: A single transaction withdraws more than 20% of the vault's Total Assets.
   - Mechanism: Withdrawals that exceed this limit will succeed, but instantly pause the vault for subsequent actions.

2. **Consecutive Failed Rebalances**:
   - Threshold: 5 consecutive failed rebalance attempts by the AI agent.
   - Mechanism: Rebalance actions increment the `ConsecutiveFailures` counter on failure. On success, the counter is reset. If it hits 5, the vault pauses.

3. **High Withdrawal Velocity**:
   - Threshold: More than 50% of the TVL is withdrawn within a 24-hour window (approx. 17,280 ledgers).
   - Mechanism: Triggers an automatic pause to allow the team to investigate potential issues.

## Events

When the circuit breaker is triggered or reset, the following events are emitted:

- `CircuitBreakerTriggeredEvent`: Emitted when the vault is automatically paused. Contains the `reason` (String) and `threshold_value` (i128).
- `CircuitBreakerResetEvent`: Emitted when the owner manually unpauses the vault and resets the circuit breaker.

## Operational Procedures

Once the circuit breaker is triggered, the vault enters a `Paused` state. In this state:
- Deposits are disabled.
- Withdrawals are disabled.
- Rebalances are disabled.

The owner must manually investigate the anomaly and call `reset_circuit_breaker()` to unpause the vault and resume operations.
