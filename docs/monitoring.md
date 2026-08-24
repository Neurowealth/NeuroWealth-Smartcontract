# NeuroWealth Vault — Monitoring & Audit Trail Strategy

Operations guide for running the NeuroWealth Vault in production.
All signals reference on-chain state read from the Stellar/Soroban ledger.

---

## 1. Routine Signals

Monitor these metrics continuously across every ledger window.

| Signal | How to Measure | Healthy Range |
|--------|----------------|---------------|
| TVL (TotalAssets) | `get_total_assets()` per ledger | Monotonically non-decreasing absent withdrawals |
| TVL growth rate | `(TotalAssets_now - TotalAssets_1h_ago) / TotalAssets_1h_ago` | Positive or flat; sharp drops warrant investigation |
| Deposit volume per ledger | Count `deposit()` calls + sum of amounts in ledger window | Tracks user inflow |
| Withdrawal volume per ledger | Count `withdraw()` + `withdraw_all()` calls + amounts | Tracks user outflow |
| Rebalance frequency | Count `rebalance()` calls per hour; compare to `MinRebalanceInterval` | Never more frequent than cooldown allows |
| Share price | `get_total_assets() / get_total_shares()` | Must be monotonically non-decreasing |
| Yield accrual | `get_total_assets()` before and after each `update_total_assets()` call | Delta ≥ 0 (no unexpected decrease) |
| TVL headroom | `(TvlCap - TotalAssets) / TvlCap` | Alert when < 5% headroom remains |

---

## 2. Warning Signals (Anomalies)

These conditions indicate abnormal behavior and require prompt investigation.

### TVL Drop Thresholds (Exploit Detection)

Define tiered alert rules based on TotalAssets decrease severity and event correlation:

| Severity | TVL Drop Condition | Event Correlation Check | Response |
|----------|-------------------|------------------------|----------|
| **Critical** | `TotalAssets_now < TotalAssets_prev * 0.95` (5%+ drop) | No matching `WithdrawEvent` in same ledger | **Immediate**: Page on-call, pause vault, investigate for exploit |
| **Critical** | `TotalAssets_now < TotalAssets_prev * 0.90` (10%+ drop) | Any condition | **Immediate**: Page on-call, pause vault, investigate for exploit |
| **High** | `TotalAssets_now < TotalAssets_prev * 0.98` (2%+ drop) | No matching `WithdrawEvent` or `RebalanceEvent` in same ledger | **Within 15 min**: Alert team, review transaction logs |
| **Medium** | `TotalAssets_now < TotalAssets_prev * 0.99` (1%+ drop) | No matching events AND repeated over 3+ consecutive ledgers | **Within 1 hour**: Review for gradual drain or accounting drift |

### Share Supply Anomalies

Monitor share supply changes that don't correlate with user actions:

| Anomaly | Condition | Severity | Response |
|---------|-----------|----------|----------|
| Unauthorized share minting | `get_total_shares()` increases without `DepositEvent` in same ledger | **Critical** | Immediate vault pause, exploit investigation |
| Unauthorized share burning | `get_total_shares()` decreases without `WithdrawEvent` in same ledger | **Critical** | Immediate vault pause, exploit investigation |
| Share-asset ratio manipulation | `(TotalAssets / TotalShares)` decreases by >0.1% without user events | **High** | Alert team, review for share price manipulation |

### General Anomalies

| Anomaly | Condition | Severity |
|---------|-----------|----------|
| Extended pause | `Paused == true` for more than 24 h | High |
| Withdrawal spike | `withdrawal_volume_1h > withdrawal_volume_30d_avg * 3` | High |
| Cap saturation | Repeated `Error(Contract, #41)` rejections | Medium |
| Cooldown violation attempt | `rebalance()` called before cooldown elapsed | Medium |
| Share price decrease | `current_share_price < previous_share_price` | Critical |
| `update_total_assets` reporting lower value | New value < stored TotalAssets without `allow_decrease=true` | High |
| Vault contract upgrade | `execute_upgrade()` called | High — requires governance sign-off |
| Upgrade scheduled | `schedule_upgrade()` called | High — initiates 24h timelock window |
| Agent update proposed | `update_agent()` called | High — initiates 24h timelock window |

---

## 3. Audit Trail

Track these on-chain events and storage mutations. Soroban events are indexed by
topic; the vault emits structured events for every significant state change.

### Admin Actions

| Action | Contract Function | Event Topic | Who |
|--------|------------------|-------------|-----|
| Pause vault | `pause()` | `paused` | Owner |
| Unpause vault | `unpause()` | `unpaused` | Owner |
| Emergency pause | `emergency_pause()` | `emerg` | Owner |
| Emergency harvest | `emergency_harvest()` | `em_harv` | Owner |
| Set TVL cap | `set_tvl_cap()` | `tvl_cap` | Owner |
| Initiate ownership transfer | `transfer_ownership()` | `own_init` | Owner |
| Accept ownership | `accept_ownership()` | `own_xfer` | Pending Owner |
| Cancel ownership transfer | `cancel_ownership_transfer()` | `own_cncl` | Owner |
| Propose agent update | `update_agent()` | `agt_prop` | Owner |
| Confirm agent update | `confirm_agent_update()` | `agt_conf` | Owner |
| Cancel agent update | `cancel_agent_update()` | `agt_cncl` | Owner |
| Schedule upgrade | `schedule_upgrade()` | `upg_sched` | Owner |
| Execute upgrade | `execute_upgrade()` | `upgraded` | Owner |
| Cancel upgrade | `cancel_upgrade()` | `upg_cncl` | Owner |

### Parameter Changes

| Action | Contract Function | What Changes |
|--------|------------------|--------------|
| Set per-user deposit cap | `set_user_deposit_cap()` | Max single-user cumulative deposit |
| Set minimum deposit | `set_min_deposit()` | Smallest accepted deposit amount |
| Set Blend pool | `set_blend_pool()` | Target Blend pool address |
| Set rebalance interval | `set_min_rebalance_interval()` | Cooldown between rebalances |

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

## 4. Alert Examples

### TVL Drop & Exploit Detection Alerts

```
ALERT: tvl_critical_drop_5pct_no_withdraw
  condition: get_total_assets() < TotalAssets_prev * 0.95 
             AND no WithdrawEvent in current ledger
  severity: critical
  action: Page on-call immediately; pause vault via emergency_pause(); 
          investigate for active exploit or contract vulnerability

ALERT: tvl_critical_drop_10pct
  condition: get_total_assets() < TotalAssets_prev * 0.90
  severity: critical  
  action: Page on-call immediately; pause vault via emergency_pause();
          investigate for exploit regardless of withdraw events

ALERT: tvl_high_drop_2pct_no_events
  condition: get_total_assets() < TotalAssets_prev * 0.98
             AND no WithdrawEvent or RebalanceEvent in current ledger
  severity: high
  action: Alert team within 15 minutes; review transaction logs;
          prepare to pause if pattern continues

ALERT: tvl_medium_gradual_drain
  condition: get_total_assets() < TotalAssets_prev * 0.99
             AND no matching events AND repeated over 3+ consecutive ledgers
  severity: medium
  action: Review within 1 hour for gradual drain or accounting drift;
          check yield calculation accuracy

ALERT: unauthorized_share_minting
  condition: get_total_shares() > previous_total_shares 
             AND no DepositEvent in current ledger
  severity: critical
  action: Immediate vault pause; investigate for share inflation exploit

ALERT: unauthorized_share_burning  
  condition: get_total_shares() < previous_total_shares
             AND no WithdrawEvent in current ledger
  severity: critical
  action: Immediate vault pause; investigate for unauthorized share destruction

ALERT: share_price_manipulation
  condition: (get_total_assets() / get_total_shares()) < previous_share_price * 0.999
             AND no user events (deposit/withdraw) in current ledger
  severity: high
  action: Alert team; review for share price manipulation attempts
```

### General Operational Alerts

```
ALERT: pause_duration_exceeded
  condition: Paused == true AND current_ledger > pause_start_ledger + 17280
  note: 17280 ledgers ≈ 24 h at ~5 s/ledger
  severity: high
  action: Notify owner; investigate reason for extended pause

ALERT: withdrawal_spike
  condition: withdrawal_volume_1h > withdrawal_volume_30d_avg * 3
  severity: high
  action: Review for coordinated exit; check protocol health

ALERT: tvl_cap_approach
  condition: get_total_assets() > TvlCap * 0.95
  severity: medium
  action: Consider raising cap or preparing user communication

ALERT: rapid_rebalance_attempts
  condition: rebalance() called more than once within MinRebalanceInterval
  severity: medium
  action: Audit agent key; verify no unauthorized rebalance calls
```

---

## 4.1. Exploit Detection Methodology

### Event Correlation Analysis

To detect potential exploits, monitoring systems must correlate `TotalAssets` changes with corresponding on-chain events:

#### Expected Event Patterns
- **TotalAssets increase**: Should correlate with `DepositEvent` or yield accrual via `update_total_assets()`
- **TotalAssets decrease**: Should correlate with `WithdrawEvent` or `RebalanceEvent` (moving funds to lower-yielding protocol)
- **Share supply increase**: Should correlate with `DepositEvent` 
- **Share supply decrease**: Should correlate with `WithdrawEvent`

#### Suspicious Patterns (Potential Exploit Indicators)
1. **Unexplained TVL drops**: TotalAssets decrease without matching withdrawal events
2. **Share inflation**: Share supply increases without deposit events  
3. **Share deflation**: Share supply decreases without withdrawal events
4. **Yield manipulation**: Share price (TotalAssets/TotalShares) decreases without user activity
5. **Flash loan attacks**: Large same-ledger deposit → rebalance → withdrawal sequences

### Monitoring Implementation

```typescript
interface ExploitDetector {
  // Check for TVL drops without corresponding events
  checkTVLIntegrity(currentLedger: number): {
    severity: 'critical' | 'high' | 'medium' | 'none';
    reason: string;
    recommendedAction: string;
  };
  
  // Verify share supply changes have matching events  
  checkShareSupplyIntegrity(currentLedger: number): boolean;
  
  // Detect abnormal transaction patterns
  checkFlashLoanAttacks(currentLedger: number): boolean;
}
```

### Response Procedures by Severity

#### Critical Severity Response (< 5 minutes)
1. **Automatic**: Trigger `emergency_pause()` if configured
2. **Manual**: Page on-call engineer immediately  
3. **Investigation**: Check recent transactions for:
   - Unauthorized function calls
   - Unexpected rebalance patterns
   - Share price manipulation
   - Flash loan attack patterns
4. **Communication**: Notify users of temporary pause via status page
5. **Recovery**: Only unpause after thorough investigation and fix deployment

#### High Severity Response (< 15 minutes)  
1. **Alert**: Notify monitoring team
2. **Analysis**: Review transaction logs and event correlation
3. **Preparation**: Prepare emergency pause if pattern escalates
4. **Documentation**: Log findings for pattern analysis

#### Medium Severity Response (< 1 hour)
1. **Review**: Analyze for gradual drains or accounting drift
2. **Validation**: Verify yield calculation accuracy
3. **Monitoring**: Increase alerting sensitivity for 24 hours
4. **Reporting**: Document findings in incident log

---

## 5. Suspicious Activity Indicators

These patterns may indicate manipulation, insider abuse, or a compromised key.

| Pattern | Description | Response |
|---------|-------------|----------|
| Deposit-withdraw cycling | Multiple accounts depositing near the cap and immediately withdrawing | Investigate for fee extraction or share-price manipulation |
| Admin address change without delay | `transfer_ownership()` / `accept_ownership()` called unexpectedly or in rapid succession | Verify legitimacy; check for owner key compromise |
| Rapid emergency pause cycles | `emergency_pause()` / `unpause()` called multiple times within 24 h | Treat as potential exploit attempt; freeze agent authority |
| `update_total_assets()` reporting decrease | `allow_decrease=false` but a lower value was passed (would revert) | Indicates misconfigured yield reporter or off-chain bug |
| Malicious agent update or upgrade scheduled | `update_agent()` or `schedule_upgrade()` called unexpectedly | Investigate immediately; prepare to call cancel/emergency pause during the 24h timelock |
| Agent calling non-agent functions | Agent address calling `pause()`, `set_tvl_cap()`, etc. | Key misuse; rotate agent key immediately |
| TVL cap set to 0 | `set_tvl_cap(0)` effectively blocks all deposits | Verify intent; could be accidental denial-of-service |

### Pause Event Disambiguation

The vault emits two distinct event topics when entering a paused state.
Indexers **must** use the topic to distinguish the pause cause:

| Pause Cause | Function Called | Event Topic | Event Type |
|-------------|-----------------|-------------|------------|
| Circuit-breaker auto-pause | `rebalance()` (internal) | `emerg` | `EmergencyPausedEvent` |
| Owner-initiated pause | `pause()` | `paused` | `VaultPausedEvent` |
| Owner emergency pause | `emergency_pause()` | `emerg` | `EmergencyPausedEvent` |

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

| Event Name | Topic | Phase | Key Fields |
|------------|-------|-------|------------|
| `AgentUpdateProposedEvent` | `agt_prop` | Step 1: Proposal | `old_agent`, `new_agent`, `effective_ledger` |
| `AgentUpdateConfirmedEvent` | `agt_conf` | Step 2: Execution | `old_agent`, `new_agent` |
| `AgentUpdateCancelledEvent` | `agt_cncl` | Escape Hatch | `old_agent`, `proposed_new_agent` |
| `UpgradeScheduledEvent` | `upg_sched` | Step 1: Proposal | `new_wasm_hash`, `effective_ledger` |
| `UpgradedEvent` | `upgraded` | Step 2: Execution | `old_version`, `new_version` |
| `UpgradeCancelledEvent` | `upg_cncl` | Escape Hatch | `cancelled_wasm_hash` |

### Suspicious Patterns

1. **Unexpected Proposals**: Any `AgentUpdateProposedEvent` or `UpgradeScheduledEvent` emitted outside of officially announced maintenance/upgrade schedules.
2. **Rapid Succession**: A proposal immediately scheduled after a cancellation, which might indicate a struggle for control.
3. **Execution Immediately on Expiry**: A proposal confirmed (`AgentUpdateConfirmedEvent` or `UpgradedEvent`) the exact ledger it becomes effective, especially if ownership transfer is also active.

### Response Window & Actions

* **Response Window**: 17,280 ledgers (approximately 24 hours).
* **Mitigation Action (Cancellation)**: If a proposal is unauthorized or suspicious, the contract owner must immediately invoke the escape hatch:
  * For agent updates: call `cancel_agent_update()` (emits `AgentUpdateCancelledEvent`).
  * For contract upgrades: call `cancel_upgrade()` (emits `UpgradeCancelledEvent`).
* **Emergency Response**: If the owner key itself is compromised, the owner (or multisig/governance wallet, if applicable) must cancel the malicious proposal, pause the vault via `emergency_pause()` or `pause()`, and prepare for key rotation.

---

## 7. DEX-Specific Monitoring

When `CurrentProtocol == "dex"`, the following additional signals should be tracked
alongside the routine signals in section 1.

### Metrics

| Signal | How to Measure | Healthy Range |
|--------|----------------|---------------|
| DEX position balance | `get_balance(vault_id)` on DEX pool contract | Matches expected deployed amount ± slippage |
| Rebalance slippage | `(amount_intended - amount_actual) / amount_intended` in `dex_sup` event | < configured `min_out` floor |
| Stuck liquidity | `balance` on DEX pool unchanged across multiple rebalance cycles | Should decrease to 0 after successful exit |
| Pool address validity | `get_dex_pool()` returns expected address | Non-null and matches configured pool |

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

## 8. Ledger-to-Time Conversion Reference

Soroban does not expose wall-clock time natively. Use ledger sequence as a proxy.

| Duration | Approximate Ledger Count (5 s/ledger) |
|----------|---------------------------------------|
| 1 hour   | 720 ledgers                           |
| 6 hours  | 4 320 ledgers                         |
| 24 hours | 17 280 ledgers                        |
| 7 days   | 120 960 ledgers                       |
| 30 days  | 518 400 ledgers                       |

These are estimates. Use `env.ledger().sequence()` for precise comparisons in
contract code; cross-reference with Stellar Horizon for wall-clock mapping in
off-chain monitoring.

