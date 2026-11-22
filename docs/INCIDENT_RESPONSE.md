# Incident Response Plan

This document defines the NeuroWealth incident response process for on-chain and operational events affecting the Soroban vault contract on Stellar. It applies to all environments (testnet and mainnet) but is primarily intended for mainnet operations.

## Overview

NeuroWealth holds user USDC in a non-custodial Soroban vault. Any security incident that could affect fund safety, contract integrity, or service availability must be handled with a structured, time-bound response.

This plan complements the existing runbooks:

- **[SECURITY.md — Owner-Compromise Response Runbook](../SECURITY.md)**: Step-by-step commands for pausing the vault, rotating the owner key, reverting attacker changes, and restoring safe operation when the owner keypair is suspected compromised.
- **[docs/UPGRADE_MIGRATION.md](UPGRADE_MIGRATION.md)**: Operational runbooks for scheduling, executing, and cancelling contract upgrades via the two-step timelock.

When an incident touches either of those areas, consult those runbooks alongside this plan.

---

## 1. Severity Levels

| Severity | Label | Definition | Examples | SLA |
|---|---|---|---|---|
| **SEV-1** | CRITICAL | Active fund loss or imminent drain risk — requires immediate on-chain action | Agent key confirmed compromised and malicious `rebalance` detected; malicious upgrade scheduled via `schedule_upgrade` | Pause vault within **15 min**; war room open within **30 min** |
| **SEV-2** | HIGH | Potential fund risk exists but no active loss yet — requires rapid triage and decision | Unexpected `rebalance` call routing funds to an unknown protocol; upgrade scheduled to an unrecognised WASM hash; agent rotation proposed to unknown address via `update_agent` | Triage within **1 h**; decision within **4 h** |
| **SEV-3** | MEDIUM | Operational issue with no immediate fund risk — requires investigation and resolution | Rebalance cooldown hit unexpectedly blocking agent operations; Blend/DEX pool responding slowly or returning errors; TVL cap unexpectedly reached, blocking new deposits | Triage within **4 h** |
| **SEV-4** | LOW | No fund risk — informational or minor operational issue | Discrepancy between docs/monitoring.md thresholds and live alerts; non-critical CI failure; minor documentation gap discovered | Triage within **24 h** |

---

## 2. Incident Declaration

### Who Can Declare

Any team member who observes an anomalous signal may raise a potential incident. **Formal declaration** (setting the severity and opening the war room) is the responsibility of the first available **Incident Commander** (IC).

Signals that should trigger declaration:

- Monitoring alert from `docs/monitoring.md` thresholds (unexplained TVL drop, unexpected protocol change event, pause event not initiated by owner)
- On-chain event scanner detecting `RebalanceEvent` with an unrecognised protocol destination
- `UpgradeScheduledEvent` with a WASM hash not matching a known, approved build
- `AgentUpdateProposedEvent` with an address not matching the expected AI agent
- User reports of failed withdrawals or unexpected balance changes

### How to Declare

1. **Open a war-room channel** immediately:
   - Telegram: create a private group named `incident-<YYYY-MM-DD>-<short-description>`, invite all on-call personnel
   - Discord: create a private channel under `#incidents` following the same naming convention
2. **Page on-call** via your configured alerting provider (PagerDuty / OpsGenie / direct message) for SEV-1 and SEV-2.
3. **Post the declaration message** in the war-room channel:
   ```
   INCIDENT DECLARED
   Severity: SEV-<N>
   IC: <name>
   Summary: <one-line description>
   Incident ID: INC-<YYYY-MM-DD>-<NNN>
   Time: <UTC timestamp>
   ```
4. Assign war-room roles (see Section 3).

### Escalation Threshold

| Condition | Action |
|---|---|
| SEV-3 investigation reveals active fund exposure | Escalate to SEV-2 immediately |
| SEV-2 investigation confirms active loss or ongoing drain | Escalate to SEV-1 immediately, invoke pause |
| SEV-1 persists > 2 h without mitigation | IC notifies all stakeholders and considers emergency ownership transfer |

---

## 3. War-Room Roles

Each declared incident (SEV-1 and SEV-2, recommended for SEV-3) should have the following roles filled at the start of the war room. One person may hold multiple roles for low-severity incidents.

| Role | Responsibilities |
|---|---|
| **Incident Commander (IC)** | Owns the overall response end-to-end. Declares severity, drives time-boxed decisions, calls escalations, assigns actions, approves all external communications, schedules post-mortem. The IC does **not** execute on-chain transactions — that is On-Chain Ops' role. |
| **On-Chain Ops** | Executes all contract interactions: `pause`, `unpause`, `cancel_upgrade`, `cancel_agent_update`, `transfer_ownership`, `accept_ownership`, `set_blend_pool`, `set_dex_pool`, `set_caps`. Maintains a live log of every transaction hash in the war-room channel. |
| **Engineering Lead** | Drives root-cause analysis. Reviews contract events, on-chain state, and agent logs. Develops and reviews any patch. Responsible for confirming when the environment is safe. |
| **Comms Lead** | Drafts all external communications (user-facing status updates, social posts, email). No external message is sent without IC approval. Uses the templates in Section 5. |
| **Security Advisor** | Reviews proposed remediations (key rotation plan, patch diff, configuration changes) for correctness and safety **before** On-Chain Ops executes them. Has veto authority on remediation steps. |

---

## 4. Response Playbooks

### SEV-1 — CRITICAL (Active Fund Loss / Imminent Drain Risk)

**Target: Pause within 15 min. War room open within 30 min.**

1. **IC declares SEV-1**, posts declaration in war-room channel, pages all on-call. Assigns all war-room roles.

2. **On-Chain Ops: Pause the vault immediately.**

   ```bash
   stellar contract invoke \
     --id $VAULT_CONTRACT_ID \
     --source <OWNER_SECRET_KEY> \
     --network mainnet \
     -- pause
   ```

   Post the transaction hash to the war-room channel. Confirm `get_paused` returns `true`.

3. **On-Chain Ops: Check for and cancel malicious pending proposals.**

   ```bash
   # Check for a pending upgrade
   stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_pending_upgrade

   # If one exists with an unrecognised hash, cancel it
   stellar contract invoke \
     --id $VAULT_CONTRACT_ID --source <OWNER_SECRET_KEY> \
     --network mainnet -- cancel_upgrade

   # Check for a pending agent update
   stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_pending_agent_update

   # If one exists with an unrecognised address, cancel it
   stellar contract invoke \
     --id $VAULT_CONTRACT_ID --source <OWNER_SECRET_KEY> \
     --network mainnet -- cancel_agent_update
   ```

   Post all transaction hashes.

4. **Comms Lead: Post Initial Acknowledgement** (Template 1 in Section 5) within 30 min of declaration. All channels: status page, Discord, Twitter/X. IC must approve before posting.

5. **Engineering Lead: Begin root-cause analysis.**
   - Pull all contract events since the last known-good state.
   - Review agent logs for unexpected calls.
   - Determine: which key is compromised, what actions were taken, what funds are at risk.
   - Report findings to IC in the war-room channel.

6. **IC: Post hourly status updates** using Template 2 until the incident is resolved. Assign Comms Lead to draft each update.

7. **On-Chain Ops + Security Advisor: Execute remediation** (Security Advisor must approve before execution):
   - If owner key is compromised: follow the full [Owner-Compromise Response Runbook](../SECURITY.md#owner-compromise-response-runbook) — pause → assess → rotate owner key → revert attacker changes → restore.
   - If agent key is compromised: rotate via `update_agent` (propose) + `confirm_agent_update` (after timelock).
   - If pool addresses were altered: reset via `set_blend_pool` / `set_dex_pool` to audited contract addresses.

8. **Comms Lead: Post Resolution notice** (Template 3 in Section 5) once Engineering Lead and Security Advisor confirm the vault is safe and has been unpaused.

9. **IC: Schedule post-mortem** within 72 hours of resolution (see Section 6).

---

### SEV-2 — HIGH (Potential Fund Risk, Not Yet Active)

**Target: Triage within 1 h. Decision within 4 h.**

1. **IC declares SEV-2**, opens war-room channel, assigns IC / On-Chain Ops / Engineering Lead / Security Advisor. Comms Lead on standby.

2. **On-Chain Ops: Capture current on-chain state** — run the full assessment checklist from [SECURITY.md Step 2](../SECURITY.md#step-2--assess-exposure). Post results to war-room.

3. **Engineering Lead: Investigate the trigger** — review event history, agent logs, and any pending timelocks. Determine whether the signal is a false alarm or a genuine threat.

4. **IC decision point (within 4 h):**
   - If investigation confirms active or imminent fund risk → escalate to SEV-1 immediately.
   - If the threat is a pending malicious proposal → On-Chain Ops cancels it (`cancel_upgrade` / `cancel_agent_update`) with Security Advisor approval, then downgrades to SEV-3.
   - If the signal is a false alarm → document finding, downgrade to SEV-4, close incident.

5. **Comms Lead:** Draft an acknowledgement (Template 1) if the incident is externally visible or user-impacting. IC decides whether to publish.

6. **IC: Schedule post-mortem** if genuine threat was confirmed, even if mitigated before fund loss.

---

### SEV-3 — MEDIUM (Operational Issue, No Immediate Fund Risk)

**Target: Triage within 4 h.**

1. **IC (or on-call engineer) acknowledges the incident**, creates a tracking ticket, assigns Engineering Lead.

2. **Engineering Lead investigates**: rebalance cooldown issues, pool connectivity, TVL cap hit. Identifies root cause.

3. **On-Chain Ops applies operational fix** if needed (e.g., `set_rebalance_cooldown`, `set_tvl_cap`, `set_blend_pool`) with IC approval.

4. **IC closes the incident** once the operational issue is resolved and monitoring confirms stability. No external communication required unless user-visible disruption occurred.

5. **Engineering Lead documents** findings in a brief internal incident note (can be a GitHub issue or Slack/Discord thread). No full post-mortem required unless the IC decides otherwise.

---

### SEV-4 — LOW (No Fund Risk)

**Target: Triage within 24 h.**

1. **On-call engineer** acknowledges, creates a GitHub issue or task ticket with the relevant details.

2. **Assign to the appropriate owner** (docs team, DevOps, engineering) for resolution in the normal sprint cycle.

3. No war-room, no formal external communication, no post-mortem required.

---

## 5. Communication Templates

Use these templates verbatim. Replace all `<PLACEHOLDER>` tokens before sending. No external message is sent without IC approval.

### Template 1 — Initial Acknowledgement

```
Subject: [NeuroWealth] Security Incident – Initial Acknowledgement – <DATE>

We are aware of <BRIEF_DESCRIPTION> affecting the NeuroWealth vault on Stellar.
All user funds are <SAFE / AT RISK>.
We have <paused the vault / are investigating>.
Next update: <TIME>.
Tracking: <INCIDENT_ID>
```

*Publish to: status page, Discord #announcements, Twitter/X. Target: within 30 min of SEV-1 declaration.*

---

### Template 2 — Status Update

```
Subject: [NeuroWealth] Security Incident Update <N> – <DATE>

Status: <INVESTIGATING / MITIGATING / MONITORING>
Actions taken: <LIST>
Funds status: <SAFE / PARTIAL LOSS / FULL LOSS>
Next update: <TIME>
```

*Publish to: same channels as Initial Acknowledgement. Frequency: hourly for SEV-1, every 4 h for SEV-2.*

---

### Template 3 — Resolution

```
Subject: [NeuroWealth] Security Incident – Resolved – <DATE>

The incident has been resolved as of <DATETIME>.
Root cause: <SUMMARY>
Impact: <USERS_AFFECTED, FUNDS_AT_RISK, ACTUAL_LOSS>
Remediation: <ACTIONS_TAKEN>
Post-mortem: to be published at <URL> within 72 hours.
```

*Publish to: same channels as Initial Acknowledgement.*

---

## 6. Post-Mortem Requirements

A post-mortem is required for all SEV-1 incidents and any SEV-2 incident where a genuine threat was confirmed.

### Deadline

Published within **72 hours** of incident resolution.

### Required Content

The post-mortem must address the following five areas (5Ws format):

| Section | Questions to Answer |
|---|---|
| **Who** | Which roles/systems were involved? Who detected the incident? Who executed the response? |
| **What** | What happened? What was the exact sequence of on-chain events? What was the actual impact (users affected, funds at risk, actual loss)? |
| **When** | Full timeline from first signal to resolution, with UTC timestamps for each key action. |
| **Where** | Which contract, network, and protocol was affected? Which monitoring system or alert fired (or should have fired)? |
| **Why** | Root cause (technical and process). Why did existing controls not prevent or detect sooner? |

Additionally include:

- **Remediation Steps Taken**: enumerate each action taken during the incident response with transaction hashes.
- **Future Mitigations**: concrete action items with owners and target dates to prevent recurrence or improve detection speed.

### Publication

Post-mortems are published as a document in the `docs/` directory (e.g., `docs/postmortem-INC-<YYYY-MM-DD>-<NNN>.md`) and linked from the GitHub incident tracking issue.

---

## 7. Tabletop Exercise Log

Tabletop exercises should be conducted at least once per quarter. Record each exercise below.

| Date | Scenario | Participants | Outcome | Action Items |
|---|---|---|---|---|
| `<YYYY-MM-DD>` | Owner-key compromise drill | TBD | Not yet run | Schedule first tabletop |

---

## 8. Related Documents

| Document | Purpose |
|---|---|
| [SECURITY.md](../SECURITY.md) | Trust model, threat analysis, and owner-compromise response runbook |
| [docs/UPGRADE_MIGRATION.md](UPGRADE_MIGRATION.md) | Upgrade operational runbooks (schedule, execute, cancel) |
| [docs/monitoring.md](monitoring.md) | Alert thresholds and monitoring setup |
| [docs/MAINNET_CHECKLIST.md](MAINNET_CHECKLIST.md) | Pre-mainnet deployment checklist |
