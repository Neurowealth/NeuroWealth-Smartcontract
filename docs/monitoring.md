# NeuroWealth Vault — Monitoring & Audit Trail Strategy

Operations guide for running the NeuroWealth Vault in production.
All signals reference on-chain state read from the Stellar/Soroban ledger.

---

## 1. Routine Signals

Monitor these metrics continuously across every ledger window.

| Signal                       | How to Measure                                                          | Healthy Range                                       |
| ---------------------------- | ----------------------------------------------------------------------- | --------------------------------------------------- |
| TVL (TotalAssets)            | `get_total_assets()` per ledger                                         | Monotonically non-decreasing absent withdrawals     |
| TVL growth rate              | `(TotalAssets_now - TotalAssets_1h_ago) / TotalAssets_1h_ago`           | Positive or flat; sharp drops warrant investigation |
| Deposit volume per ledger    | Count `deposit()` calls + sum of amounts in ledger window               | Tracks user inflow                                  |
| Withdrawal volume per ledger | Count `withdraw()` + `withdraw_all()` calls + amounts                   | Tracks user outflow                                 |
| Rebalance frequency          | Count `rebalance()` calls per hour; compare to `MinRebalanceInterval`   | Never more frequent than cooldown allows            |
| Share price                  | `get_total_assets() / get_total_shares()`                               | Must be monotonically non-decreasing                |
| Yield accrual                | `get_total_assets()` before and after each `update_total_assets()` call | Delta ≥ 0 (no unexpected decrease)                  |
| TVL headroom                 | `(TvlCap - TotalAssets) / TvlCap`                                       | Alert when < 5% headroom remains                    |

---

## 2. Warning Signals (Anomalies)

These conditions indicate abnormal behavior and require prompt investigation.

| Anomaly                                     | Condition                                                                           | Severity                             | Rationale                                                                                  |
| ------------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------ | ------------------------------------------------------------------------------------------ |
| Critical Unexplained TVL Drop               | `TotalAssets(k) < TotalAssets(k-1) * 0.95` without matching `WithdrawEvent`         | Critical (P0)                        | Immediate indication of active exploit, flash drain, or severe protocol insolvency       |
| High Unexplained TVL Drop                   | `TotalAssets(k) < TotalAssets(k-1) * 0.99` without matching `WithdrawEvent`         | High (P1)                            | Unreported loss, sudden bad-debt recognition, or uncontained slippage                      |
| Share Supply Drift                          | `TotalShares(k) != TotalShares(k-1)` without `DepositEvent` or `WithdrawEvent`      | Critical (P0)                        | Accounting invariant breach; indicates arbitrary state manipulation or storage corruption |
| Share Price Dilution                        | `(TotalAssets/TotalShares)_now < (TotalAssets/TotalShares)_prev * 0.999`            | Critical (P0)                        | Dilution/inflation attack or unauthorized asset devaluation                                |
| TVL / Share Invariant Breakdown             | `(TotalShares == 0 && TotalAssets > 0) || (TotalAssets == 0 && TotalShares > 0)`    | Critical (P0)                        | Insolvent state or division-by-zero trap                                                   |
| Extended pause                              | `Paused == true` for more than 24 h                                                 | High (P1)                            | Stalled operations; potential unhandled incident                                           |
| Withdrawal spike                            | `withdrawal_volume_1h > withdrawal_volume_30d_avg * 3`                              | High (P1)                            | Coordinated run or insider exit                                                            |
| Clustered `update_total_assets` Decreases   | `count(AssetsUpdatedEvent{delta < 0}) >= 2 in 1h` OR `sum(decrease_bps[1h]) > 150`  | High (P1)                            | Slow-bleed attack attempting to bypass single-event drop thresholds                        |
| Sustained 24h Near-Cap Bleed                | `sum(decrease_bps[24h]) > 300` OR `count(near_cap_decrease[24h]) >= 3`             | Critical (P0)                        | Sustained drain approaching single-event bps cap repeatedly                                |
| Cap saturation                              | Repeated `Error(Contract, #41)` rejections                                          | Medium (P2)                          | Demand exceeding configured TVL limit                                                      |
| Cooldown violation attempt                  | `rebalance()` called before cooldown elapsed                                        | Medium (P2)                          | Agent timing bug or spam attempt                                                           |
| Vault contract upgrade                      | `execute_upgrade()` called                                                          | High (P1) — requires sign-off        | Code swap on live contract                                                                 |
| Upgrade scheduled                           | `schedule_upgrade()` called                                                         | High (P1) — initiates 24h window     | Timelock opened; verify proposal hash against audited release                              |
| Agent update proposed                       | `update_agent()` called                                                             | High (P1) — initiates 24h window     | Timelock opened; verify proposed agent address                                             |

---

## 3. Audit Trail

Track these on-chain events and storage mutations. Soroban events are indexed by
topic; the vault emits structured events for every significant state change.

### Admin Actions

| Action                      | Contract Function             | Event Topic | Who           |
| --------------------------- | ----------------------------- | ----------- | ------------- |
| Pause vault                 | `pause()`                     | `paused`    | Owner         |
| Unpause vault               | `unpause()`                   | `unpaused`  | Owner         |
| Emergency pause             | `emergency_pause()`           | `emerg`     | Owner         |
| Emergency harvest           | `emergency_harvest()`         | `em_harv`   | Owner         |
| Set TVL cap                 | `set_tvl_cap()`               | `tvl_cap`   | Owner         |
| Initiate ownership transfer | `transfer_ownership()`        | `own_init`  | Owner         |
| Accept ownership            | `accept_ownership()`          | `own_xfer`  | Pending Owner |
| Cancel ownership transfer   | `cancel_ownership_transfer()` | `own_cncl`  | Owner         |
| Propose agent update        | `update_agent()`              | `agt_prop`  | Owner         |
| Confirm agent update        | `confirm_agent_update()`      | `agt_conf`  | Owner         |
| Cancel agent update         | `cancel_agent_update()`       | `agt_cncl`  | Owner         |
| Schedule upgrade            | `schedule_upgrade()`          | `upg_sched` | Owner         |
| Execute upgrade             | `execute_upgrade()`           | `upgraded`  | Owner         |
| Cancel upgrade              | `cancel_upgrade()`            | `upg_cncl`  | Owner         |

### Parameter Changes

| Action                   | Contract Function              | What Changes                       |
| ------------------------ | ------------------------------ | ---------------------------------- |
| Set per-user deposit cap | `set_user_deposit_cap()`       | Max single-user cumulative deposit |
| Set minimum deposit      | `set_min_deposit()`            | Smallest accepted deposit amount   |
| Set Blend pool           | `set_blend_pool()`             | Target Blend pool address          |
| Set rebalance interval   | `set_min_rebalance_interval()` | Cooldown between rebalances        |

### Rebalance Executions

Each `rebalance()` call must be logged with:

- Source protocol (prior `CurrentProtocol`)
- Destination protocol (new `CurrentProtocol`)
- Amount moved
- Ledger sequence (timestamp proxy)
- Agent address

### Large Transactions

Flag any single `deposit()` or `withdraw()` where:

```
amount > get_total_assets() * 0.01
```

A deposit or withdrawal exceeding 1% of TVL in a single transaction warrants
manual review.

---

## 4. Anomaly Alert Specifications & Exploit Signatures

Concrete alert rules for automated indexers, Prometheus Alertmanager, and monitoring daemons.

### 4.1. Exploit & Anomaly Alert Definitions

#### ALERT: `unexplained_tvl_drop_critical` (Active Exploit Signature)
- **Severity**: `Critical` (P0 — Immediate Page)
- **Condition**: `TotalAssets(ledger_k) < TotalAssets(ledger_{k-1}) * 0.95` without a matching `WithdrawEvent` or `AssetsUpdatedEvent` in that ledger window.
- **Threshold**: Instantaneous drop `> 5%` within ≤ 1 ledger window (or `> 10%` in 5 minutes).
- **Rationale**: Vault accounting is invariant-preserving. Total assets can only legally decrease via user withdrawals (`withdraw`/`withdraw_all`) or co-signed yield decreases via `update_total_assets(allow_decrease=true)` (capped by bps). An uncorroborated drop indicates unauthorized token drain, rebalance bridge exploit, or ledger storage corruption.
- **PromQL Query**:
  ```promql
  (
    (neurowealth_vault_total_assets - neurowealth_vault_total_assets offset 1m) / neurowealth_vault_total_assets offset 1m < -0.05
  ) unless (
    increase(neurowealth_vault_withdraw_amount_total[1m]) > 0
    or
    increase(neurowealth_vault_assets_updated_decrease_total[1m]) > 0
  )
  ```
- **Horizon / SQL Event Query**:
  ```sql
  WITH ledger_delta AS (
    SELECT ledger, total_assets,
           LAG(total_assets) OVER (ORDER BY ledger) AS prev_assets
    FROM vault_ledger_snapshots
    WHERE contract_id = '$VAULT_CONTRACT_ID'
  )
  SELECT d.ledger, d.total_assets, d.prev_assets,
         ((d.prev_assets - d.total_assets)::float / d.prev_assets) AS drop_ratio
  FROM ledger_delta d
  LEFT JOIN contract_events e
    ON e.contract_id = '$VAULT_CONTRACT_ID'
   AND e.ledger = d.ledger
   AND e.topic_0 IN ('withdraw', 'assets')
  WHERE d.prev_assets > 0
    AND ((d.prev_assets - d.total_assets)::float / d.prev_assets) > 0.05
    AND e.id IS NULL;
  ```
- **Runbook Action**: **IMMEDIATE EMERGENCY PAUSE**. Execute `emergency_pause()` or `pause()`. Suspend off-chain agent rebalancing authority. Follow [`AGENT_KEY_COMPROMISE_RUNBOOK.md` — Containment](AGENT_KEY_COMPROMISE_RUNBOOK.md#phase-2-containment-t0-to-t30m).

---

#### ALERT: `unexplained_tvl_drop_high` (Subtle Loss / Unreported Bad Debt)
- **Severity**: `High` (P1 — 15m SLA)
- **Condition**: `TotalAssets(ledger_k) < TotalAssets(ledger_{k-1}) * 0.99` without matching `WithdrawEvent`.
- **Threshold**: Instantaneous drop `> 1%` in a single ledger absent user withdrawals.
- **Rationale**: Detects silent protocol-level socialized losses (such as Blend collateral liquidation / bad debt) or abnormal DEX execution slippage exceeding tolerance.
- **Runbook Action**: Verify external protocol status (`get_current_protocol`, Blend pool reserves). If unexplained, initiate temporary pause and audit transaction logs.

---

#### ALERT: `share_supply_unaccounted_drift` (Invariant Violation)
- **Severity**: `Critical` (P0 — Immediate Page)
- **Condition**: `TotalShares(ledger_k) != TotalShares(ledger_{k-1})` AND `count(DepositEvent) == 0` AND `count(WithdrawEvent) == 0` in `ledger_k`.
- **Threshold**: `delta(TotalShares) != 0` with zero mint/burn events.
- **Rationale**: `TotalShares` is the authoritative ledger of vault ownership. Shares can only be minted in `deposit()` and burned in `withdraw()` / `withdraw_all()`. Any supply change without matching events indicates arbitrary storage manipulation, replay attack, or catastrophic VM fault.
- **PromQL Query**:
  ```promql
  (changes(neurowealth_vault_total_shares[1m]) > 0)
  unless (
    increase(neurowealth_vault_deposit_events_total[1m]) > 0
    or
    increase(neurowealth_vault_withdraw_events_total[1m]) > 0
  )
  ```
- **Runbook Action**: **IMMEDIATE EMERGENCY PAUSE**. Call `emergency_pause()`. Halt all contract interactions and notify the security response team.

---

#### ALERT: `share_price_dilution_spike`
- **Severity**: `Critical` (P0 — Immediate Page)
- **Condition**: `(get_total_assets() / get_total_shares())_now < (get_total_assets() / get_total_shares())_prev * 0.999` without an authorized `AssetsUpdatedEvent`.
- **Threshold**: Share price drop `> 0.1%` in absence of owner-authorized loss report.
- **Rationale**: Vault share price must be monotonically non-decreasing during normal operation. A sudden share price dilution indicates an inflation/donation attack or unauthorized extraction.
- **Runbook Action**: Pause contract; audit recent deposit/withdraw sequences in the mempool and transaction traces.

---

#### ALERT: `tvl_share_asymmetry_broken_invariant`
- **Severity**: `Critical` (P0 — Immediate Page)
- **Condition**: `(TotalShares == 0 AND TotalAssets > 0) OR (TotalAssets == 0 AND TotalShares > 0)`
- **Threshold**: Non-zero assets with zero shares, or zero assets with non-zero shares.
- **Rationale**: Solvency invariant breakdown. Non-zero shares with zero assets causes division-by-zero or zero-value conversions. Non-zero assets with zero shares locks capital permanently.
- **Runbook Action**: Pause contract; inspect initialization or full-withdrawal flows.

---

### 4.2. Alert-to-Runbook Action Mapping

| Alert Identifier | Severity | Trigger Threshold | Primary Runbook Action | Escalation Target | SLA |
| ---------------- | -------- | ----------------- | ---------------------- | ----------------- | --- |
| `unexplained_tvl_drop_critical` | `Critical` (P0) | TVL drop > 5% without `WithdrawEvent` | Call `emergency_pause()`; freeze agent process; audit token balance | Incident Commander & Security Team | < 5 min |
| `unexplained_tvl_drop_high` | `High` (P1) | TVL drop > 1% without `WithdrawEvent` | Query `get_deployed_assets()`; verify Blend bad-debt cache | Lead DeFi Engineer | < 15 min |
| `share_supply_unaccounted_drift` | `Critical` (P0) | TotalShares delta with 0 deposit/withdraw events | Call `emergency_pause()`; halt indexers; check node RPC integrity | Protocol Engineering | < 5 min |
| `share_price_dilution_spike` | `Critical` (P0) | Share price drop > 0.1% without loss event | Call `pause()`; analyze recent contract transaction history | Smart Contract Auditor | < 10 min |
| `tvl_share_asymmetry_broken_invariant` | `Critical` (P0) | `TotalShares == 0 ^ TotalAssets == 0` | Call `emergency_pause()`; inspect storage keys | Core Tech Lead | < 5 min |
| `update_total_assets_hourly_decrease_cluster` | `High` (P1) | ≥ 2 decreases in 1h OR > 150 bps in 1h | Throttle agent bot; verify off-chain yield oracle feed | Operations On-Call | < 15 min |
| `update_total_assets_sustained_bleed_drain` | `Critical` (P0) | > 300 bps loss in 24h OR ≥ 3 near-cap decreases | Call `pause()`; initiate agent key rotation per runbook | Security Lead & Multisig Owners | < 10 min |
| `pause_duration_exceeded` | `High` (P1) | `Paused == true` for > 17,280 ledgers (24h) | Review post-incident investigation status; plan unpause | Product Operations | < 1 hour |
| `withdrawal_spike` | `High` (P1) | 1h withdrawal volume > 3x 30-day average | Check pool liquidity; review macro market conditions | Risk Analyst | < 30 min |

---

## 5. Suspicious Activity Indicators

These patterns may indicate manipulation, insider abuse, or a compromised key.

| Pattern                                     | Description                                                                              | Response                                                                                |
| ------------------------------------------- | ---------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Deposit-withdraw cycling                    | Multiple accounts depositing near the cap and immediately withdrawing                    | Investigate for fee extraction or share-price manipulation                              |
| Admin address change without delay          | `transfer_ownership()` / `accept_ownership()` called unexpectedly or in rapid succession | Verify legitimacy; check for owner key compromise                                       |
| Rapid emergency pause cycles                | `emergency_pause()` / `unpause()` called multiple times within 24 h                      | Treat as potential exploit attempt; freeze agent authority                              |
| `update_total_assets()` reporting decrease  | `allow_decrease=false` but a lower value was passed (would revert)                       | Indicates misconfigured yield reporter or off-chain bug                                 |
| Malicious agent update or upgrade scheduled | `update_agent()` or `schedule_upgrade()` called unexpectedly                             | Investigate immediately; prepare to call cancel/emergency pause during the 24h timelock |
| Agent calling non-agent functions           | Agent address calling `pause()`, `set_tvl_cap()`, etc.                                   | Key misuse; rotate agent key immediately                                                |
| TVL cap set to 0                            | `set_tvl_cap(0)` effectively blocks all deposits                                         | Verify intent; could be accidental denial-of-service                                    |

### Pause Event Disambiguation

The vault emits two distinct event topics when entering a paused state.
Indexers **must** use the topic to distinguish the pause cause:

| Pause Cause                | Function Called          | Event Topic | Event Type             |
| -------------------------- | ------------------------ | ----------- | ---------------------- |
| Circuit-breaker auto-pause | `rebalance()` (internal) | `emerg`     | `EmergencyPausedEvent` |
| Owner-initiated pause      | `pause()`                | `paused`    | `VaultPausedEvent`     |
| Owner emergency pause      | `emergency_pause()`      | `emerg`     | `EmergencyPausedEvent` |

**Key distinction**: Both circuit-breaker auto-pause and `emergency_pause()`
emit `EmergencyPausedEvent` with topic `emerg`. Only `pause()` emits
`VaultPausedEvent` with topic `paused`. To determine whether the vault was
paused by the circuit breaker or by the owner, check:

1. **Event topic**: `emerg` → circuit breaker or emergency pause; `paused` →
   owner-initiated pause
2. **Timing correlation**: If an `emerg` event coincides with a failed
   `rebalance` transaction, it was the circuit breaker. If it correlates with
   a standalone `emergency_pause` call, it was the owner.

### Emergency Harvest Event

`emergency_harvest()` emits `EmergencyHarvestEvent` (topic `em_harv`), which is
distinct from the regular `HarvestEvent` (topic `harvest`). This allows
indexers to differentiate owner-initiated emergency harvests from
agent-initiated harvests during monitoring and audit trails.

---

## 6. Timelock Monitoring (Admin Key Compromise Mitigation)

To mitigate the risk of an admin key compromise, updates to the authorized AI agent (`update_agent`) and upgrades to the contract's WASM logic (`schedule_upgrade`) are protected by a mandatory 24-hour timelock (17,280 ledgers).

Operations teams must monitor on-chain events during this delay window to detect and react to unauthorized or malicious proposals before they can be executed.

### Events to Watch

| Event Name                  | Topic       | Phase             | Key Fields                                   |
| --------------------------- | ----------- | ----------------- | -------------------------------------------- |
| `AgentUpdateProposedEvent`  | `agt_prop`  | Step 1: Proposal  | `old_agent`, `new_agent`, `effective_ledger` |
| `AgentUpdateConfirmedEvent` | `agt_conf`  | Step 2: Execution | `old_agent`, `new_agent`                     |
| `AgentUpdateCancelledEvent` | `agt_cncl`  | Escape Hatch      | `old_agent`, `proposed_new_agent`            |
| `UpgradeScheduledEvent`     | `upg_sched` | Step 1: Proposal  | `new_wasm_hash`, `effective_ledger`          |
| `UpgradedEvent`             | `upgraded`  | Step 2: Execution | `old_version`, `new_version`                 |
| `UpgradeCancelledEvent`     | `upg_cncl`  | Escape Hatch      | `cancelled_wasm_hash`                        |

### Suspicious Patterns

1. **Unexpected Proposals**: Any `AgentUpdateProposedEvent` or `UpgradeScheduledEvent` emitted outside of officially announced maintenance/upgrade schedules.
2. **Rapid Succession**: A proposal immediately scheduled after a cancellation, which might indicate a struggle for control.
3. **Execution Immediately on Expiry**: A proposal confirmed (`AgentUpdateConfirmedEvent` or `UpgradedEvent`) the exact ledger it becomes effective, especially if ownership transfer is also active.

### Response Window & Actions

- **Response Window**: 17,280 ledgers (approximately 24 hours).
- **Mitigation Action (Cancellation)**: If a proposal is unauthorized or suspicious, the contract owner must immediately invoke the escape hatch:
  - For agent updates: call `cancel_agent_update()` (emits `AgentUpdateCancelledEvent`).
  - For contract upgrades: call `cancel_upgrade()` (emits `UpgradeCancelledEvent`).
- **Emergency Response**: If the owner key itself is compromised, the owner (or multisig/governance wallet, if applicable) must cancel the malicious proposal, pause the vault via `emergency_pause()` or `pause()`, and prepare for key rotation.

---

## 7. DEX-Specific Monitoring

When `CurrentProtocol == "dex"`, the following additional signals should be tracked
alongside the routine signals in section 1.

### Metrics

| Signal                | How to Measure                                                           | Healthy Range                               |
| --------------------- | ------------------------------------------------------------------------ | ------------------------------------------- |
| DEX position balance  | `get_balance(vault_id)` on DEX pool contract                             | Matches expected deployed amount ± slippage |
| Rebalance slippage    | `(amount_intended - amount_actual) / amount_intended` in `dex_sup` event | < configured `min_out` floor                |
| Stuck liquidity       | `balance` on DEX pool unchanged across multiple rebalance cycles         | Should decrease to 0 after successful exit  |
| Pool address validity | `get_dex_pool()` returns expected address                                | Non-null and matches configured pool        |

### Alert Conditions

```
ALERT: dex_position_mismatch
  condition: DexPool.balance(vault_id) != expected_deployed_amount (±1%)
  severity: high
  action: Audit rebalance events; check for partial fill or pool accounting bug

ALERT: dex_abnormal_slippage
  condition: dex_sup event amount_actual < amount_intended * 0.99
             AND min_out was not triggered
  severity: medium
  action: Review pool depth; consider raising min_out or switching protocol

ALERT: dex_stuck_liquidity
  condition: CurrentProtocol == "none" AND DexPool.balance(vault_id) > 0
  severity: high
  action: Pool may not have fully returned funds on exit; check remove_liquidity
          return value and retry rebalance to "none"

ALERT: dex_pool_not_configured
  condition: get_dex_pool() returns None AND rebalance to "dex" attempted
  severity: critical
  action: Owner must call set_dex_pool() before DEX rebalances can proceed

ALERT: dex_supply_failed
  condition: dex_sup event emitted with success = false
  severity: high
  action: Pool rejected supply (cap hit or zero liquidity); rebalance to "none"
          or wait for pool capacity to recover
```

### Diagnosing Stuck DEX Liquidity

If a rebalance exit from DEX is suspected to have left funds in the pool:

```bash
# 1. Check on-chain protocol state
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet \
  -- get_current_protocol

# 2. Query pool balance directly
stellar contract invoke --id $DEX_POOL_ADDRESS --network mainnet \
  -- balance --asset $USDC_ADDRESS --user $VAULT_CONTRACT_ID

# 3. Look for dex_wd events and their actual amounts
stellar events --network mainnet --start-ledger <RECENT_LEDGER> \
  --contract-id $VAULT_CONTRACT_ID | grep dex_wd
```

If `get_current_protocol` returns `"none"` but the DEX pool still holds a
non-zero balance for the vault, the exit leg completed from the vault's
perspective but the pool accounting drifted. Retry `rebalance("none", 0, 0)`;
if the pool still reports a balance after that, escalate to the pool operator.

### Misconfigured Pool Address

A pool address set to a contract that does not implement `add_liquidity`,
`remove_liquidity`, and `balance` will cause the first `rebalance("dex", ...)` to
panic. Validate the pool address off-chain before calling `set_dex_pool()`:

```bash
stellar contract invoke --id $PROPOSED_DEX_POOL --network mainnet \
  -- balance --asset $USDC_ADDRESS --user $VAULT_CONTRACT_ID
```

A successful (even zero) response confirms the interface is compatible.

---

## 8. Rate-Based Monitoring & Repeated Near-Cap Decrease Alerts (`update_total_assets`)

The vault contract allows authorized yield updates via `update_total_assets()`. When reporting a loss (`new_total < old_total`), the contract enforces a basis-point cap:

```rust
let effective_cap_bps = max_decrease_bps.max(100); // floor: 100 bps = 1%
let max_decrease = old_total * effective_cap_bps / 10_000;
require(actual_decrease <= max_decrease, VaultError::DecreaseExceedsMaximumAllowedBps);
```

### Threat Model: Slow-Bleed Extraction

While instantaneous TVL drops (> 5%) trigger P0 anomaly alerts, a compromised agent (or rogue off-chain yield oracle) could attempt to bleed value incrementally. By issuing repeated decreases just below the single-event cap (e.g., 90–99 bps per call every few hours), an attacker could siphon significant vault value over a 24-hour period without tripping single-event thresholds.

Operations daemons must monitor the **rate, frequency, and clustering** of `AssetsUpdatedEvent` decreases.

### Rate-Based Alert Rules

#### ALERT: `update_total_assets_hourly_decrease_cluster`
- **Severity**: `High` (P1 — 15m SLA)
- **Condition**: `count(AssetsUpdatedEvent{delta < 0}) >= 2 in 1 hour (720 ledgers)` OR `sum(decrease_bps[1h]) > 150 bps`.
- **Threshold**: More than 1 loss report in an hour, or cumulative hourly loss > 1.5%.
- **PromQL Example**:
  ```promql
  sum_over_time(
    (neurowealth_vault_assets_updated_old_total - neurowealth_vault_assets_updated_new_total)
    / neurowealth_vault_assets_updated_old_total * 10000 [1h]
  ) > 150
  or
  count_over_time(neurowealth_vault_assets_updated_event{direction="decrease"}[1h]) >= 2
  ```
- **Horizon / SQL Event Query**:
  ```sql
  SELECT
    count(*) AS decrease_count,
    sum((old_total - new_total)::float / old_total * 10000) AS total_decrease_bps
  FROM contract_events
  WHERE contract_id = '$VAULT_CONTRACT_ID'
    AND topic_0 = 'assets'
    AND new_total < old_total
    AND ledger_sequence >= (current_ledger() - 720)
  HAVING count(*) >= 2 OR sum((old_total - new_total)::float / old_total * 10000) > 150;
  ```

---

#### ALERT: `update_total_assets_sustained_bleed_drain`
- **Severity**: `Critical` (P0 — Immediate Page)
- **Condition**: Cumulative decrease across all `update_total_assets` calls `> 300 bps (3%)` in 24 hours (17,280 ledgers), OR `count(decreases >= 0.80 * effective_cap_bps) >= 3` in 24 hours.
- **Threshold**: 24-hour cumulative loss > 3% OR clustering of near-cap decrease events.
- **PromQL Example**:
  ```promql
  sum_over_time(
    (neurowealth_vault_assets_updated_old_total - neurowealth_vault_assets_updated_new_total)
    / neurowealth_vault_assets_updated_old_total * 10000 [24h]
  ) > 300
  or
  count_over_time(neurowealth_vault_assets_updated_event{near_cap="true"}[24h]) >= 3
  ```
- **Horizon / SQL Event Query**:
  ```sql
  SELECT
    count(*) AS near_cap_count,
    sum((old_total - new_total)::float / old_total * 10000) AS total_decrease_bps_24h
  FROM contract_events
  WHERE contract_id = '$VAULT_CONTRACT_ID'
    AND topic_0 = 'assets'
    AND new_total < old_total
    AND (old_total - new_total)::float / old_total >= (max_decrease_bps * 0.80 / 10000)
    AND ledger_sequence >= (current_ledger() - 17280)
  HAVING count(*) >= 3 OR sum((old_total - new_total)::float / old_total * 10000) > 300;
  ```

---

### Documented Escalation Path & Pause Recommendation

If either near-cap decrease alert fires:

```
[Near-Cap Decrease Alert Triggered]
               │
               ▼
   1. TRIGGER EMERGENCY PAUSE
      (Call pause() or emergency_pause())
               │
               ▼
   2. SUSPEND OFF-CHAIN AGENT PROCESS
      (Kill running bot daemon / revoke signer)
               │
               ▼
   3. INDEPENDENT ON-CHAIN RECONCILIATION
      (Query live USDC token balance + Blend/DEX pool balances)
               │
      ┌────────┴────────┐
      ▼                 ▼
[External Loss Valid]  [Discrepancy / Exploitation Detected]
      │                 │
      ▼                 ▼
Document loss event    Initiate Agent Key Rotation via update_agent()
and resume operations  and escalate to Security Response Team
```

1. **Immediate Pause Recommendation**: The contract owner / multisig MUST immediately invoke `pause()` or `emergency_pause()`. Pausing freezes user deposits and withdrawals and prevents further asset updates from eroding share value.
2. **Freeze Off-Chain Agent Bot**: Terminate the agent process container to prevent automated scheduling of further loss updates.
3. **Audit Underlying Reserves**:
   - Query `token.balance(vault_contract)`.
   - Query `BlendPool.get_balance(vault_contract)` and `DexPool.get_balance(vault_contract)`.
   - Compute `actual_total = idle_usdc + blend_deployed + dex_deployed`.
   - If `actual_total < total_assets`, verify whether Blend incurred socialized bad debt or DEX pool suffered permanent impermanent loss.
4. **Key Rotation**: If the reported decreases do not match verifiable on-chain protocol balances, assume agent key compromise. Follow [`AGENT_KEY_COMPROMISE_RUNBOOK.md`](AGENT_KEY_COMPROMISE_RUNBOOK.md) to propose a new agent address via `update_agent()`.

---

## 9. Whale-Exit Concentration Risk (Issue #599)

A single account holding a large share of `TotalShares` introduces two
distinct risks when it exits: (1) rounding-driven share-price drift for
remaining users, and (2) a liquidity strain on `withdraw()`/`withdraw_all()`
if the exit is larger than what's currently idle/available — see
`docs/PARTIAL_WITHDRAWAL_BEHAVIOR.md` (Issue #600) for the fairness/DoS
mechanics of that second risk in detail. This section defines the
concentration metric, alert threshold, and a worked example for the first
risk.

### Concentration Metric

```
concentration_pct(user) = (get_shares(user) * 100) / get_total_shares()
```

Both `get_shares(user)` and `get_total_shares()` are free, read-only getters
(no TTL side effects — see "Persistent Storage TTL Policy" in
`ARCHITECTURE.md`), so this metric can be polled continuously without cost or
on-chain footprint.

### Alert Threshold

```
ALERT: whale_concentration
  condition: concentration_pct(user) > 20  (i.e., X = 20%)
  severity: medium at 20%, high at 35%, critical at 50%+
  action: Flag the account for exit-impact modeling (below); confirm the
          vault's idle + readily-withdrawable balance could absorb a full
          exit from this account without triggering the partial-withdrawal
          path documented in PARTIAL_WITHDRAWAL_BEHAVIOR.md
```

`X = 20%` is chosen as the default first-tier threshold because it is the
point at which a single account's exit starts to be a meaningfully larger
event than ordinary withdrawal volume (compare to the existing
`withdrawal_spike` alert in Section 4, which fires at 3× the 30-day average
volume in aggregate — a account above 20% concentration can trivially trigger
that alert alone). Operators with a smaller or more concentrated user base
may reasonably lower this threshold; operators with many similarly-sized
holders may raise it. There is no on-chain enforcement of this threshold —
`UserDepositCap` (owner-configurable, see `set_user_deposit_cap()`) is the
only contract-level lever that bounds how concentrated a single account *can*
become, and it bounds absolute USDC exposure, not percentage-of-pool, so it
does not by itself prevent concentration from rising as other users withdraw
even if no single deposit grows.

### Why concentration matters: rounding drift on exit

`convert_to_assets_internal` (the function backing `withdraw`,
`withdraw_all`, and `get_balance`) computes:

```
usdc_owed = floor(shares * TotalAssets / TotalShares)
```

Integer division **floors** — it always rounds toward zero, never up. Every
withdrawal therefore leaves behind a small remainder (at most
`TotalShares - 1` units of rounding dust, in the worst case) that is not
transferred to the withdrawing user. That dust remains in `TotalAssets`
while `TotalShares` has already been reduced by the exact number of shares
burned — which means the *effective* share price for everyone remaining
(`TotalAssets / TotalShares`) ticks up very slightly after every withdrawal,
including a whale's.

This is not a bug — it is the standard "no value leaks, all rounding favors
existing holders" pattern the vault's inflation-attack defenses rely on (see
`ARCHITECTURE.md`'s Rounding Rules and Overflow Safety sections). But the
*size* of the one-time rounding dust from a single exit scales with the size
of that exit: a whale liquidating a large share position in one transaction
concentrates what would otherwise be many small holders' worth of rounding
dust into a single event, which is the concrete mechanism by which "a
dominant depositor exiting can shift share price for remaining users" (as
the issue puts it) — it's a real, if small and directionally-favorable-to-
remaining-holders, effect worth being able to explain when a dashboard shows
a share-price tick right after a large withdrawal.

### Worked Example

Assume:
- `TotalShares = 1,000,000` (1:1 with USDC at initial deposit, before yield)
- `TotalAssets = 1,050,000` USDC (5% yield accrued since launch)
- Whale holds `250,000` shares — `concentration_pct = 25%`, above the 20%
  alert threshold

Whale calls `withdraw_all()`:

```
entitled_amount = floor(250,000 * 1,050,000 / 1,000,000)
                = floor(262,500,000,000 / 1,000,000)
                = floor(262,500.0)
                = 262,500 USDC   (exact — no rounding loss in this case
                                   because 250,000 * 1,050,000 divides evenly
                                   by 1,000,000)
```

Post-exit state (assuming full liquidity was available, no partial-fill
path triggered):

```
TotalShares = 1,000,000 - 250,000 = 750,000
TotalAssets = 1,050,000 - 262,500 = 787,500
new_share_price = 787,500 / 750,000 = 1.05   (unchanged — confirms no value
                                                was created or destroyed by
                                                a clean-dividing exit)
```

Now the same example with a whale holding a share count that does **not**
divide evenly — `250,001` shares (`concentration_pct ≈ 25.0001%`):

```
entitled_amount = floor(250,001 * 1,050,000 / 1,000,000)
                = floor(262,501,050,000 / 1,000,000)
                = floor(262,501.05)
                = 262,501 USDC   (0.05 of a unit lost to rounding — this
                                   example uses whole-USDC numbers for
                                   readability; real deposits are denominated
                                   in stroops, 1 USDC = 1,000,000 stroops per
                                   DEFAULT_MIN_DEPOSIT, so scale accordingly)

TotalShares = 1,000,000 - 250,001 = 749,999
TotalAssets = 1,050,000 - 262,501 = 787,499
new_share_price = 787,499 / 749,999 ≈ 1.05000007   (a ~0.000007% uptick versus
                                                      the pre-exit 1.05 price,
                                                      entirely from the 0.05
                                                      units of rounding dust
                                                      now redistributed pro-
                                                      rata across the smaller
                                                      remaining TotalShares)
```

The uptick is real but economically negligible at this scale — it only
becomes a monitoring concern when a whale's exit is large enough, or
`TotalShares` remaining afterward is small enough, that the same
floor-rounding math produces a visible share-price jump an off-chain
dashboard or the AI agent's yield reporting might otherwise misattribute to
an actual yield event. **Practical guidance**: when `share_price_decrease` or
an unexpected `share_price` jump alert (Section 4) fires within the same
ledger window as a `withdraw()`/`withdraw_all()` event from a flagged
whale-concentration account, attribute it to rounding first and confirm via
the arithmetic above before escalating as a solvency concern.

### Liquidity Strain on Exit

Separately from rounding, a whale's exit is a single large withdrawal
request against whatever liquidity is idle or immediately available from the
active protocol at that moment. If `entitled_amount` exceeds available
liquidity, the whale receives a partial fill under the exact mechanics in
`docs/PARTIAL_WITHDRAWAL_BEHAVIOR.md` — there is no special-cased handling
for large withdrawals in the contract. Operationally, this means a flagged
whale account should be cross-referenced against current idle balance
(`get_idle_balance()`) and deployed assets (`get_deployed_assets()`) before
any planned/announced exit, since the vault itself has no way to reserve
liquidity ahead of time for a specific account's future withdrawal.

---

## 10. Blend Protocol Bad-Debt Monitoring

When funds are deployed to Blend Protocol, the vault becomes sensitive to **socialized bad-debt events**. These occur when Blend recognizes collateral liquidations or defaults, reducing the total assets across all suppliers pro-rata.

Unlike vault-level withdrawals, bad-debt events do **not** emit vault-side events. Users see their balance drop without any transaction.

**See [`BLEND_INTEGRATION_RESEARCH.md` — Bad-Debt Analysis](BLEND_INTEGRATION_RESEARCH.md#bad-debt-analysis-socialized-loss-impact-on-vault-assets)** for:

- **Mechanism:** How socialization works and why share price drops silently
- **Detection signals:** Exchange rate monitoring, balance audits, user reports
- **Worked example:** $500k loss walkthrough with impact on users
- **Response options:** Silent acceptance, rebalance exit, user communication
- **Monitoring hooks:** Periodic polling and pre-rebalance audits

### Quick Detection Check

```bash
# Periodically sample (e.g., hourly):
RATE_NOW=$(stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet \
  -- get_exchange_rate)

RATE_LAST=$(cat .blend-rate-cache)

DELTA=$(( RATE_NOW - RATE_LAST ))

if (( DELTA < 0 )); then
  LOSS_PCT=$(( DELTA * 100 / RATE_LAST ))
  echo "Blend bad debt: $LOSS_PCT% loss detected"

  if (( LOSS_PCT < -200 )); then
    # > 2% loss: alert and consider rebalance exit
    echo "CRITICAL: Consider emergency_rebalance_exit()"
  fi
fi

echo "$RATE_NOW" > .blend-rate-cache
```

---

## 11. Ledger-to-Time Conversion Reference

Soroban does not expose wall-clock time natively. Use ledger sequence as a proxy.

| Duration | Approximate Ledger Count (5 s/ledger) |
| -------- | ------------------------------------- |
| 1 hour   | 720 ledgers                           |
| 6 hours  | 4 320 ledgers                         |
| 24 hours | 17 280 ledgers                        |
| 7 days   | 120 960 ledgers                       |
| 30 days  | 518 400 ledgers                       |

These are estimates. Use `env.ledger().sequence()` for precise comparisons in
contract code; cross-reference with Stellar Horizon for wall-clock mapping in
off-chain monitoring.

---

## 12. Rebalance APY Deviation & Frequency Monitoring (Rogue-Agent Detection)

The agent key is the only address that can call `rebalance()`. A compromised or
malfunctioning agent will usually reveal itself in one of two ways before funds
are at risk: the `expected_apy` it reports drifts away from what the underlying
protocols actually pay, or it starts rebalancing far more often than policy
allows. Both are observable from the `RebalanceEvent` stream alone
(topic `"rebalance"`, see [EVENTS.md](../EVENTS.md)), so indexers can enforce
these rules without any contract change.

### Rolling Confidence Band on `expected_apy`

Maintain a rolling statistical band over the `expected_apy` field (basis
points) of recent successful rebalances and flag any new value that falls
outside it:

| Parameter        | Recommended value                       | Rationale                                                          |
| ---------------- | --------------------------------------- | ------------------------------------------------------------------ |
| Window           | Trailing 30 days of `rebalance` events  | Long enough to smooth market moves, short enough to track regimes  |
| Minimum samples  | 10 events                               | Below this, fall back to the absolute bounds only                  |
| Band             | `mean ± 3 × stddev` of windowed values  | ~99.7% of honest values fall inside a 3σ band                      |
| Absolute floor   | `0` bps                                 | Contract already rejects negative values                           |
| Absolute ceiling | `2000` bps (20%)                        | Sustained APY above this on Blend/DEX USDC strategies is implausible |

Evaluation rule for each new `RebalanceEvent`:

1. If fewer than the minimum samples exist, alert only when
   `expected_apy > absolute ceiling`.
2. Otherwise alert when `expected_apy < max(floor, mean − 3σ)` or
   `expected_apy > min(ceiling, mean + 3σ)`.
3. Exclude `status = "failed"` events from the window (they never moved funds)
   but still evaluate them — a failed rebalance with an absurd APY claim is
   itself a signal.

### Rebalance-Frequency Rate Policy

`MinRebalanceInterval` already hard-blocks calls that arrive inside the
cooldown (`Error(Contract, #14)`), so on-chain state cannot churn faster than
the cooldown allows. The monitoring rule instead watches for an agent that
rebalances *at* the maximum allowed rate, which honest strategies rarely do:

| Signal                | Threshold                                              | Severity |
| --------------------- | ------------------------------------------------------ | -------- |
| Sustained max-rate    | > 6 rebalances in 24 h each landing < 10 min after cooldown expiry | high     |
| Frequency spike       | 24 h rebalance count > 4 × trailing 30-day daily average | medium   |
| Cooldown probing      | ≥ 3 `Error(Contract, #14)` failures in 1 h              | medium   |

### Alert Definitions

```
ALERT: apy_out_of_band
  condition: RebalanceEvent.expected_apy outside [mean - 3*stddev, mean + 3*stddev]
             over trailing 30d window (min 10 samples), or > 2000 bps absolute
  severity: high
  action: Page on-call; cross-check reported APY against Blend/DEX pool rates;
          if unexplained, treat agent key as compromised (see
          AGENT_KEY_COMPROMISE_RUNBOOK.md) and prepare emergency_pause

ALERT: rebalance_rate_spike
  condition: count(rebalance events, 24h) > 4 * avg_daily_count_30d
             OR sustained max-rate pattern (see table above)
  severity: medium
  action: Audit recent rebalance decisions against strategy policy; verify
          agent infrastructure has not been re-pointed or duplicated
```

### Example Indexer Pseudo-Query

Assuming rebalance events are indexed into an `events` table with the payload
decoded into columns:

```sql
-- One row per new rebalance event, flagged if outside the rolling band.
WITH window_stats AS (
  SELECT
    AVG(expected_apy)          AS mean_apy,
    STDDEV_SAMP(expected_apy)  AS sd_apy,
    COUNT(*)                   AS n
  FROM events
  WHERE topic = 'rebalance'
    AND status <> 'failed'
    AND ledger_closed_at >= NOW() - INTERVAL '30 days'
)
SELECT
  e.tx_hash,
  e.expected_apy,
  w.mean_apy,
  w.sd_apy,
  CASE
    WHEN e.expected_apy > 2000 THEN 'out_of_band'          -- absolute ceiling
    WHEN w.n < 10 THEN 'insufficient_history'
    WHEN e.expected_apy NOT BETWEEN GREATEST(0,   w.mean_apy - 3 * w.sd_apy)
                                AND LEAST(2000, w.mean_apy + 3 * w.sd_apy)
      THEN 'out_of_band'
    ELSE 'ok'
  END AS verdict
FROM events e, window_stats w
WHERE e.topic = 'rebalance'
  AND e.ledger_closed_at >= NOW() - INTERVAL '1 hour';
```

Frequency-spike variant:

```sql
SELECT COUNT(*) AS last_24h,
       (SELECT COUNT(*) / 30.0 FROM events
         WHERE topic = 'rebalance'
           AND ledger_closed_at >= NOW() - INTERVAL '30 days') AS daily_avg_30d
FROM events
WHERE topic = 'rebalance'
  AND ledger_closed_at >= NOW() - INTERVAL '24 hours'
HAVING COUNT(*) > 4 * (SELECT COUNT(*) / 30.0 FROM events
                        WHERE topic = 'rebalance'
                          AND ledger_closed_at >= NOW() - INTERVAL '30 days');
```

Tune the multipliers per deployment; record any changes to the band or rate
policy in the incident-response log so alert history stays interpretable.
