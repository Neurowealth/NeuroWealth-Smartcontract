# Partial Rebalance Failure Recovery

This document describes what happens when a `rebalance()` call fails mid-flight,
what state is safe to rely on, how operators can diagnose the failure, and what
the retry and emergency recovery steps are.

---

## What Can Fail During a Rebalance

A rebalance involves up to two legs: **exit** (withdraw from the current
protocol) and **enter** (supply to the new protocol). Either leg can fail
independently.

| Failure point | Cause | Vault state after failure |
|---|---|---|
| Exit leg incomplete | Protocol has insufficient liquidity to return the full deployed amount | `CurrentProtocol` unchanged; `RebalanceFailedEvent` emitted; no state mutations |
| Exit leg panics | Pool contract reverts (e.g. `min_out` not met on withdrawal) | Transaction reverts entirely; no state change |
| Enter leg incomplete | Pool accepts less than the full vault balance (supply cap hit) | `CurrentProtocol` updated to new protocol; `RebalanceEvent` with `status = "partial"` emitted |
| Enter leg panics | `min_out` not met on supply | Transaction reverts entirely; protocol remains the exited value (`"none"` if exit succeeded) |

> **Key invariant.** The vault never writes partial state. Either the full
> rebalance completes (all mutations committed), it aborts gracefully via
> `RebalanceFailedEvent` (no mutations), or the transaction reverts (Soroban
> rolls back all changes). Users' shares and the exchange rate are never
> corrupted by a failed rebalance.

---

## What Users See

### During a failed rebalance

- Funds are **not at risk**. The vault's `TotalAssets` and per-user shares are
  unchanged.
- A `reb_fail` event appears on-chain. Users monitoring via Horizon or a
  dashboard will see this event against the vault contract address.
- Deposits and withdrawals **continue to work** unless the vault is paused.
  A failed rebalance does not trigger an automatic pause.

### Partial enter (supply cap hit)

- `RebalanceEvent.status == "partial"` on-chain.
- Some USDC remains idle in the vault rather than deployed.
- The exchange rate is unaffected — idle USDC still counts toward `TotalAssets`.
- The AI agent will attempt to deploy the remainder on the next rebalance cycle.

---

## Diagnosing a Failure

### 1. Check for `RebalanceFailedEvent` on-chain

```bash
# List recent events for the vault contract
stellar events \
  --network mainnet \
  --start-ledger <RECENT_LEDGER> \
  --contract-id $VAULT_CONTRACT_ID
```

Look for events with topic `reb_fail`. The event body contains:

```json
{
  "from_protocol": "blend",   // or "dex"
  "reason": "exit_fail"       // incomplete withdrawal from the exiting protocol
}
```

### 2. Check the active protocol and idle balance

```bash
# Which protocol is currently active?
stellar contract invoke \
  --id $VAULT_CONTRACT_ID --network mainnet \
  -- get_current_protocol

# How much USDC is sitting idle in the vault?
stellar contract invoke \
  --id $VAULT_CONTRACT_ID --network mainnet \
  -- get_total_assets
```

If `get_current_protocol` still returns the old protocol after a `reb_fail`
event, the exit leg failed and no funds moved. If it returns `"none"` but there
are still funds showing in the protocol's balance query, the enter leg failed
after a successful exit.

### 3. Check protocol liquidity

For Blend:

```bash
# Query how much liquidity is available for withdrawal in the pool
soroban contract invoke --id $BLEND_POOL --network mainnet \
  -- get_reserve_data --asset $USDC_ADDRESS
```

For the DEX pool:

```bash
soroban contract invoke --id $DEX_POOL --network mainnet \
  -- balance --asset $USDC_ADDRESS --user $VAULT_CONTRACT_ID
```

### 4. DEX-Specific Diagnostics

When `CurrentProtocol` is or was `"dex"`, use these additional queries.

**Verify the vault's DEX position:**

```bash
# How much USDC the DEX pool is holding for the vault
stellar contract invoke --id $DEX_POOL_ADDRESS --network mainnet \
  -- balance --asset $USDC_ADDRESS --user $VAULT_CONTRACT_ID
```

A non-zero result when `CurrentProtocol == "none"` means the exit leg transferred
funds back to the vault but the pool's internal accounting was not updated, or
the exit was never completed. Retry `rebalance("none", 0, 0)`.

**Check the `dex_sup` and `dex_wd` events for the failed rebalance:**

```bash
stellar events \
  --network mainnet \
  --start-ledger <LEDGER_BEFORE_FAILURE> \
  --contract-id $VAULT_CONTRACT_ID
```

Look for:
- `dex_sup` with `success = false` — the supply leg was rejected (pool cap,
  zero liquidity, or misreporting adapter).
- `dex_wd` with `success = false` — the withdrawal leg returned 0 (pool
  has no withdrawable balance for the vault).
- Absence of `dex_wd` after a `reb_fail` with `from_protocol = "dex"` —
  the exit was never attempted (cooldown still active or vault paused).

**Confirm the pool address is still the expected contract:**

```bash
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet \
  -- get_dex_pool
```

If `None` is returned but `CurrentProtocol` is `"dex"`, the pool address was
cleared (e.g. by a contract upgrade) after the rebalance was initiated. The
owner must call `set_dex_pool()` with the correct address before retrying.

---

## Retry Steps

### Case A — exit leg failed (`reason: exit_fail`)

The vault is still deployed to the old protocol. Retry when protocol liquidity
recovers:

```bash
# Retry with the same protocol transition, no slippage floor (min_out = 0)
stellar contract invoke \
  --id $VAULT_CONTRACT_ID \
  --source <AGENT_SECRET_KEY> \
  --network mainnet \
  -- rebalance \
  --protocol <NEW_PROTOCOL> \
  --expected_apy <APY_IN_BPS> \
  --min_out 0
```

If liquidity is unlikely to recover (e.g. the pool is winding down), the agent
should rebalance to `"none"` first to pull whatever is available, wait for the
remainder, then redeploy:

```bash
# Pull available funds to idle
stellar contract invoke \
  --id $VAULT_CONTRACT_ID --source <AGENT_SECRET_KEY> --network mainnet \
  -- rebalance --protocol none --expected_apy 0 --min_out 0
```

### Case B — enter leg partial (supply cap hit)

The vault deployed a partial amount. The remaining idle USDC is safe inside the
vault. The AI agent can retry the enter leg on the next cycle — the supply leg
will deploy the remainder:

```bash
# Retry entering the same protocol
stellar contract invoke \
  --id $VAULT_CONTRACT_ID --source <AGENT_SECRET_KEY> --network mainnet \
  -- rebalance --protocol <SAME_PROTOCOL> --expected_apy <APY_IN_BPS> --min_out 0
```

---

## Emergency Steps

### If the agent key is unavailable

The owner can pause the vault to prevent further rebalances until the agent key
is restored:

```bash
stellar contract invoke \
  --id $VAULT_CONTRACT_ID --source <OWNER_SECRET_KEY> --network mainnet \
  -- pause
```

See [`SECURITY.md`](../SECURITY.md) for the full owner-compromise and emergency
pause runbook.

### If funds are stuck in a protocol with no liquidity

1. **Pause** the vault (owner or agent `emergency_pause`).
2. **Monitor** the protocol's liquidity; retry `rebalance --protocol none` as
   liquidity becomes available.
3. If the protocol is permanently broken, coordinate with the protocol team for
   a direct recovery path. The vault cannot force a withdrawal beyond what the
   protocol allows.
4. **Unpause** once funds are recovered or a safe partial-recovery path is confirmed.

---

## Invariants That Always Hold

The following are guaranteed by the contract regardless of how a rebalance fails:

- `TotalShares` × `exchange_rate` = `TotalAssets` at all times.
- Per-user shares are never modified by a rebalance.
- A failed rebalance never reduces `TotalAssets`.
- Users can always withdraw their proportional share of whatever assets the
  vault currently holds (idle USDC + any protocol-recoverable balance), subject
  only to the active protocol's own liquidity constraints.

---

## MEV Sandwich Failures on DEX Rebalances

### When a Sandwich Attack Causes `min_out` Rejection

If the agent backend computes `min_out` off-chain based on current pool state but
a sandwich attack inflates the pool price before the rebalance lands on-chain, the
vault may reject the transaction:

- **Failure mode**: `MinOutNotMet` (#42) error, transaction reverts, no state change.
- **Recovery**: The vault is unharmed; funds remain in the current protocol (or
  idle if the exit leg succeeded). The agent should wait and retry with a
  recomputed `min_out` based on current conditions.

### Example Scenario

1. Agent observes pool at 100 USDC : 10 BLND.
2. Agent computes `min_out = 80,000` for a 1M USDC supply (assuming 20% slippage).
3. Attacker front-runs and buys 500k BLND, pushing the rate to 100 USDC : 8 BLND.
4. Vault's `rebalance()` supplies 1M USDC, receives only 80,000 BLND.
5. `80,000 >= min_out` (80,000), so the transaction succeeds with minimal loss.

However, if the attacker's price movement exceeds the agent's tolerance:

1. Pool at 100 USDC : 10 BLND.
2. Agent computes `min_out = 95,000` (assuming 5% slippage tolerance).
3. Attacker buys 1M BLND, pushing rate to 100 USDC : 5 BLND (50% slippage).
4. Vault's `rebalance()` supplies 1M USDC, receives only 50,000 BLND.
5. `50,000 < min_out` (95,000), transaction reverts.

### Agent Backend Guidance

To reduce sandwich-induced failures:

- **Recompute `min_out` just before submitting** the transaction (within 1-2 blocks).
- **Use conservative slippage tolerances** (0.5% - 2%) on thin pools; increase on
  deep pools.
- **Randomize rebalance timing** to reduce predictability. Scheduled, fixed-time
  rebalances (e.g., every hour) are easy targets for sandwich attacks.
- **For large deployments**, consider breaking them into multiple smaller
  rebalances over time, reducing the profit surface for any single sandwich.
- **Monitor pool depth** before the rebalance. If the pool is shallower than
  expected, increase slippage tolerance or defer the rebalance.

See [`DEX_INTEGRATION.md`](DEX_INTEGRATION.md) for detailed MEV analysis and
`min_out` computation best practices.

---

## Related Documents

- [`SECURITY.md`](../SECURITY.md) — emergency pause and owner runbook
- [`DEX_INTEGRATION.md`](DEX_INTEGRATION.md) — DEX slippage, exit-first routing, and MEV sandwich risk analysis
- [`BLEND_INTEGRATION_RESEARCH.md`](BLEND_INTEGRATION_RESEARCH.md) — Blend partial-fill behaviour
- [`../EVENTS.md`](../EVENTS.md) — `RebalanceEvent` and `RebalanceFailedEvent` schemas
