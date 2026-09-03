# NeuroWealth AI Decision Agent

The NeuroWealth agent is an autonomous background service that continuously monitors yield opportunities across Stellar DeFi protocols (Blend Protocol, Soroswap/Phoenix DEX pools) and executes rebalancing for vault participants.

---

## Key Modules

- **Yield Comparison Engine** (`src/yieldComparison.ts`): Aggregates real-time and historical (7d/30d/90d) APYs, calculating risk-adjusted return ratios (Sharpe-like metric) and enforcing the 0.5% minimum improvement threshold.
- **Risk Scoring Engine** (`src/riskScoring.ts`): Multi-dimensional risk evaluation (smart contract, liquidity, oracle, governance, centralization risks) to determine protocol eligibility and risk thresholds.
- **Intent Parser** (`src/intentParser.ts`): Natural language parsing for WhatsApp & chat commands.
- **Event Listener** (`src/eventListener.ts`): Real-time contract event ingestion.
- **Protocol Adapters** (`src/protocolAdapters.ts`): Uniform venue adapters (`supply`, `withdraw`, `get_apy`, `get_balance`) with a registry enforcing the owner-managed protocol whitelist and the risk-scoring gate (#656). Phase 2 venues: Phoenix orderbook DEX, Aquarius AMM — see `docs/PROTOCOL_ADAPTER_INTERFACE.md`.
- **Rebalancing Scheduler** (`src/scheduler.ts`): time-based and event-driven rebalancing with gas awareness (#651, below).
- **Metrics & Dashboard API** (`src/metrics.ts`, `src/metricsApi.ts`): performance-metrics aggregation + HTTP endpoints for the dashboard UI (#652).
- **State Backup & Failover** (`src/stateBackup.ts`, `src/failover.ts`): persistent agent state, lease-based hot standby (#655, `docs/DISASTER_RECOVERY.md`).

---

## Rebalancing scheduler (#651)

`RebalanceScheduler` complements the on-chain `MinRebalanceInterval` cooldown
by deciding *when* to propose rebalances off-chain.

### Configuration (`SchedulerConfig`)

```ts
{
  interval: 'hourly' | '4h' | 'daily',  // owner setting; default '4h'
  maxGasPriceStroops: 100_000,          // skip scheduled rebalances above this fee
  circuitBreakerThreshold: 3,           // consecutive failures that pause scheduling
  tvlChangeTriggerBps: 500,             // |ΔTVL| >= 5% enqueues an event-driven job
}
```

### Triggers and priority

Jobs enter a priority queue ordered: `utilization_spike` > `tvl_change` >
`manual` > `scheduled`. Feed observations from the event listener:

- `onTvlObservation(before, after)` — enqueues a `tvl_change` job when the
  move exceeds `tvlChangeTriggerBps`.
- `onUtilizationObservation(protocol, utilizationBps)` — enqueues a
  `utilization_spike` job when a protocol's utilization crosses its band.
- `setInterval('daily')` — owner changes the cadence at runtime.

### Execution semantics

`tick()` (called by `start(pollMs)`, default 60 s) executes due jobs:

- **Gas awareness**: if `gasPriceProvider()` returns more than
  `maxGasPriceStroops`, scheduled jobs are skipped (counted in
  `totalSkippedGas`); event-driven jobs still run.
- **Circuit breaker**: `circuitBreakerThreshold` consecutive failed
  executions pause scheduling; `resetCircuitBreaker()` re-arms it. Alerts
  fire via the `onAlert` hook (`missed_rebalance`, `failed_rebalance`,
  `circuit_breaker_tripped`, `gas_price_skipped`).
- **History & attribution**: every execution records before/after APY, gas
  cost, trigger, and outcome in `getHistory()`; `getStats()` aggregates
  totals, average gas, and average APY delta for the dashboard (#652).

```ts
const scheduler = new RebalanceScheduler(DEFAULT_SCHEDULER_CONFIG, {
  executor: executeRebalance,          // (job) => Promise<RebalanceExecutionResult>
  gasPriceProvider: fetchNetworkFee,
  onAlert: (a) => notifyOnCall(a),
});
scheduler.start();
```

Missed or failed scheduled rebalances are alerted through `onAlert`, which
the agent wires into the alert engine (`src/alertEngine.ts`).

---

## Metrics & monitoring endpoints (#652)

`startMetricsServer()` (env `METRICS_PORT`, default 9100) serves:

- `GET /health` — liveness/heartbeat status (`503` when degraded; doubles as
  the failover monitor target).
- `GET /api/metrics/snapshot` — TVL + yield, peak/drop, protocol allocation
  breakdown (pie chart data), rebalance history with before/after APY, gas
  cost analysis, uptime %, error rate, user deposit/withdrawal trends, and
  firing threshold alerts.
- `GET /api/metrics/alerts` — alerts only, for pagers/webhooks.

Thresholds (`AlertThresholds`) are configurable: TVL drop bps, error rate
bps, consecutive rebalance failures, heartbeat staleness.

---

## Disaster recovery (#655)

Agent state is backed up to pluggable stores (Postgres/Supabase, S3,
file; `MultiStateStore` fans out and falls back). A hot standby promotes
itself when the primary heartbeat (15 s interval, 60 s TTL) goes stale,
restores the latest backup, and resumes — worst case < 90 s, well within the
5-minute RTO. Runbooks for every failure scenario:
[`docs/DISASTER_RECOVERY.md`](../docs/DISASTER_RECOVERY.md).
