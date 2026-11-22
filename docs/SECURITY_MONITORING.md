# Security Monitoring & Alerting

The NeuroWealth agent is equipped with a real-time security monitoring system. The `eventListener` constantly monitors vault events (deposits, withdrawals, etc.) and routes them to the `alertEngine`.

## Detection Rules

1. **TVL Anomaly (CRITICAL)**: A single withdrawal exceeds 20% of total assets.
2. **Large Withdrawal (HIGH)**: A single withdrawal exceeds 100,000 USDC.
3. **Authentication Failure (MEDIUM)**: Multiple invalid auth attempts on vault operations.

## Notification Channels

Based on severity, the alert engine can push alerts to:
- **Email**: Daily digests or high-priority notifications.
- **Telegram/Discord**: Real-time channel messages for medium+ severity.
- **PagerDuty**: Immediate escalation for critical events (e.g. TVL drops).

## Integration

The rules are defined in `agent/src/alertEngine.ts`. To add new rules, implement the `AlertRule` interface and append to the `alertRules` array.
