# Agent-Key Compromise Response Runbook

This runbook provides step-by-step procedures for responding to a suspected or confirmed compromise of the AI agent key. It complements the owner-compromise runbook in SECURITY.md with agent-specific detection, timelock rotation, and user communication procedures.

## Executive Summary

The AI agent key has elevated permissions to rebalance funds and report asset values but cannot directly withdraw user funds. However, a compromised agent can cause economic damage through malicious rebalances, incorrect asset reporting, or griefing via repeated failed operations. This runbook covers detection, timelock-based rotation, monitoring freezes, and user communication.

## Prerequisites

- Access to the owner key (or multisig threshold)
- Access to on-chain monitoring/alerting infrastructure
- Pre-configured legitimate agent address ready for rotation
- Communication channels for user notifications (Twitter, Discord, email)

---

## Step 1 — Detection & Initial Assessment

### Detection Signals

Monitor for the following indicators of agent-key compromise:

| Indicator | Description | Severity |
|-----------|-------------|----------|
| **Rogue rebalances** | Unexpected protocol switches or large position movements | HIGH |
| **Asset reporting anomalies** | Sudden, unexplained total asset decreases | HIGH |
| **Rebalance spam** | Rapid-fire rebalance attempts (possible griefing) | MEDIUM |
| **Failed rebalance streak** | Consecutive failures approaching circuit-breaker threshold | MEDIUM |

### Immediate Assessment Commands

```bash
# Check current agent address
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_agent

# Check for pending agent update (timelock in progress)
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_pending_agent_update

# Check current protocol and deployed funds
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_current_protocol

# Check recent rebalance activity (via events or indexer)
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_last_rebalance_ledger

# Check vault pause state
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_paused
```

### Decision Tree

```
DETECTION SIGNALS
├── Confirmed malicious activity observed?
│   ├── YES → Proceed to Step 2 (Emergency Response)
│   └── NO → Continue assessment
├── Suspicious but not confirmed?
│   ├── YES → Consider watch mode (Step 3b)
│   └── NO → Continue monitoring
└── No indicators?
    └── Continue normal monitoring
```

---

## Step 2 — Emergency Response (Confirmed Compromise)

### 2a — Immediate Pause (Within Minutes)

If malicious activity is confirmed, pause the vault immediately to prevent further damage:

```bash
stellar contract invoke \
  --id $VAULT_CONTRACT_ID \
  --source <OWNER_SECRET_KEY> \
  --network mainnet \
  -- pause
```

**Requires**: owner auth **[owner]**

> **Note**: If the owner key is also compromised, follow the owner-compromise runbook in SECURITY.md first.

### 2b — Initiate Agent Rotation via Timelock

The agent update follows a two-step timelock process (Issue #317):

```bash
# Step 1: Schedule the agent update [owner]
stellar contract invoke \
  --id $VAULT_CONTRACT_ID \
  --source <OWNER_SECRET_KEY> \
  --network mainnet \
  -- update_agent \
  --new_agent <LEGITIMATE_AGENT_ADDRESS>

# Step 2: Confirm after timelock expires [owner]
# Timelock duration: AGENT_TIMELOCK_LEDGERS (8,640 ledgers ≈ 12 hours)
stellar contract invoke \
  --id $VAULT_CONTRACT_ID \
  --source <OWNER_SECRET_KEY> \
  --network mainnet \
  -- confirm_agent_update
```

**Timelock Constants**:
- `AGENT_TIMELOCK_LEDGERS`: 8,640 ledgers (~12 hours on mainnet)
- This delay provides a window for monitoring and community notification

### 2c — Cancel Malicious Pending Agent Update

If the attacker has already initiated an agent update:

```bash
stellar contract invoke \
  --id $VAULT_CONTRACT_ID \
  --source <OWNER_SECRET_KEY> \
  --network mainnet \
  -- cancel_agent_update
```

**Requires**: owner auth **[owner]**

---

## Step 3 — Monitoring Mode (Suspicious but Unconfirmed)

If activity is suspicious but not clearly malicious, consider watch mode instead of immediate pause:

### 3a — Enhanced Monitoring

- Increase polling frequency for rebalance events
- Set up alerts for any protocol switches
- Monitor for rapid-fire rebalance attempts
- Track total asset changes closely

### 3b — Prepare Rotation (Without Executing)

Have the legitimate agent address ready and test the rotation flow on testnet:

```bash
# On testnet: verify the rotation flow works
TESTNET_VAULT_CONTRACT_ID=...
LEGITIMATE_AGENT_ADDRESS=...
stellar contract invoke \
  --id $TESTNET_VAULT_CONTRACT_ID \
  --source owner \
  --network testnet \
  -- update_agent \
  --new_agent $LEGITIMATE_AGENT_ADDRESS
```

### 3c — Decision Criteria for Escalation

Escalate to emergency response if:
- Any protocol switch to unknown/unverified pool
- Total asset decrease > 10% in single rebalance
- Consecutive failed rebalances > threshold
- Any indication of fund diversion

---

## Step 4 — Post-Rotation Verification

After completing the agent rotation:

### 4a — Verify New Agent Address

```bash
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_agent
```

Expected result: Should return the legitimate agent address.

### 4b — Verify No Pending Updates

```bash
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_pending_agent_update
```

Expected result: Should return `null` (no pending update).

### 4c — Test Rebalance Functionality

On testnet or with small amounts, verify the new agent can successfully rebalance:

```bash
# Small test rebalance via new agent
stellar contract invoke \
  --id $VAULT_CONTRACT_ID \
  --source <NEW_AGENT_SECRET_KEY> \
  --network mainnet \
  -- rebalance \
  --strategy blend \
  --amount 1000 \
  --min_out 0
```

---

## Step 5 — Unpause Decision

### Decision Framework

**Unpause immediately if**:
- Agent rotation completed successfully
- No pending malicious updates remain
- Protocol addresses verified (Blend/DEX pools)
- Monitoring infrastructure in place

**Keep paused if**:
- Uncertainty about compromise scope
- Additional investigation needed
- Want to coordinate with community first

### Unpause Command

```bash
stellar contract invoke \
  --id $VAULT_CONTRACT_ID \
  --source <OWNER_SECRET_KEY> \
  --network mainnet \
  -- unpause
```

**Requires**: owner auth **[owner]**

---

## Step 6 — User Communication

### 6a — Immediate Notification (Within 1 Hour)

**Channels**: Twitter, Discord, Email announcement

**Template**:

```
🚨 SECURITY NOTICE: Agent Key Rotation in Progress

We have detected suspicious activity from the AI agent key and are initiating
a security rotation. User funds remain safe.

What's happening:
- Vault paused (emergency measure)
- Agent key rotation via 12-hour timelock in progress
- No direct access to user funds (agent permissions limited)

What this means for users:
- Deposits/withdrawals temporarily paused
- Funds remain secure in contract
- Normal operations resume after rotation

Next update: [TIME] or when rotation completes
```

### 6b — Post-Rotation Update (After Timelock Completes)

**Template**:

```
✅ Agent Key Rotation Complete

The agent key rotation has been successfully completed. The vault is now
operating with the verified agent address.

Status:
- New agent address: [ADDRESS]
- Vault: [PAUSED/UNPAUSED]
- Normal operations: [RESUMING/RESUMED]

Thank you for your patience. Your funds remain secure.
```

### 6c — Post-Mortem (Within 72 Hours)

Publish a detailed post-mortem including:
- Timeline of the incident
- What was detected
- Actions taken
- Impact assessment (user funds affected = 0)
- Preventive measures for the future

---

## Step 7 — Post-Incident Hardening

### 7a — Credential Rotation

- Rotate all credentials that were co-located with the compromised agent key
- Review and update secret management practices
- Consider hardware security modules (HSM) for agent key storage

### 7b — Monitoring Enhancements

- Add alerts for all detection signals listed in Step 1
- Implement real-time rebalance monitoring
- Set up automated alerts for asset value changes
- Consider external security monitoring services

### 7c — Process Review

- Review why the compromise was not detected earlier
- Update detection thresholds based on incident data
- Conduct a security audit of the agent infrastructure
- Consider implementing agent key rotation schedule

---

## Appendix: Timelock Timing Reference

| Operation | Timelock Duration | Mainnet Wall Time |
|-----------|------------------|-------------------|
| Agent update (`update_agent` → `confirm_agent_update`) | 8,640 ledgers | ~12 hours |
| Contract upgrade (`schedule_upgrade` → `execute_upgrade`) | 17,280 ledgers | ~24 hours |

**Note**: These are mainnet constants. Testnet may have different timing due to ledger production rate.

---

## Appendix: Command Reference

### Agent Update Flow

```bash
# Check current agent
get_agent() -> Address

# Check pending update
get_pending_agent_update() -> (Address, u64) | null

# Schedule update (starts timelock)
update_agent(new_agent: Address) -> (pending_agent, expiry_ledger)

# Confirm after timelock expires
confirm_agent_update() -> success

# Cancel pending update
cancel_agent_update() -> success
```

### Related Getter Functions

```bash
get_agent()              # Current agent address
get_pending_agent_update()  # Pending agent and expiry, if any
get_last_rebalance_ledger()  # Most recent rebalance sequence
get_current_protocol()   # "idle", "blend", or "dex"
get_paused()             # Boolean pause state
```

---

## Related Documentation

- [SECURITY.md](../SECURITY.md) - Owner-compromise runbook and security model
- [MAINNET_CHECKLIST.md](MAINNET_CHECKLIST.md) - Section 6: Emergency procedures
- [REBALANCE_FAILURE_RECOVERY.md](REBALANCE_FAILURE_RECOVERY.md) - Rebalance failure handling
- [monitoring.md](monitoring.md) - Monitoring infrastructure setup

---

**Last Updated**: 2025-01-XX
**Version**: 1.0
**Maintained By**: NeuroWealth Security Team