import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
  INTERVAL_MS,
  RebalanceScheduler,
  SchedulerAlert,
  RebalanceExecutionResult,
  RebalanceJob,
} from './scheduler';

const HOUR = INTERVAL_MS.hourly;

function makeScheduler(
  executorResults: RebalanceExecutionResult[],
  now: { t: number },
  opts: { gasPrice?: number; interval?: 'hourly' | '4h' | 'daily'; threshold?: number } = {},
) {
  const alerts: SchedulerAlert[] = [];
  let call = 0;
  const executedJobs: RebalanceJob[] = [];
  const scheduler = new RebalanceScheduler(
    {
      interval: opts.interval ?? '4h',
      maxGasPriceStroops: 100_000,
      circuitBreakerThreshold: opts.threshold ?? 3,
      tvlChangeTriggerBps: 500,
    },
    {
      executor: (job) => {
        executedJobs.push(job);
        const result = executorResults[Math.min(call, executorResults.length - 1)];
        call += 1;
        return result;
      },
      gasPriceProvider: opts.gasPrice !== undefined ? async () => opts.gasPrice! : undefined,
      onAlert: (a) => alerts.push(a),
      now: () => now.t,
    },
  );
  return { scheduler, alerts, executedJobs };
}

const OK: RebalanceExecutionResult = {
  success: true,
  beforeApy: 0.06,
  afterApy: 0.085,
  gasCostStroops: 12_000,
};
const FAIL: RebalanceExecutionResult = { success: false, error: 'pool_exhausted' };

describe('Rebalance scheduler (Issue #651)', () => {
  it('maps configured intervals to milliseconds', () => {
    assert.strictEqual(INTERVAL_MS.hourly, 3_600_000);
    assert.strictEqual(INTERVAL_MS['4h'], 14_400_000);
    assert.strictEqual(INTERVAL_MS.daily, 86_400_000);
  });

  it('executes scheduled rebalances only after the interval elapses', async () => {
    const now = { t: 1_000_000 };
    const { scheduler, executedJobs } = makeScheduler([OK], now);

    const first = await scheduler.tick(now.t);
    assert.strictEqual(first.length, 1);
    assert.strictEqual(first[0].trigger, 'scheduled');

    // 1h later with a 4h interval: nothing due.
    now.t += HOUR;
    assert.strictEqual((await scheduler.tick(now.t)).length, 0);

    // 4h after the first run: due again.
    now.t += 3 * HOUR;
    const second = await scheduler.tick(now.t);
    assert.strictEqual(second.length, 1);
    assert.strictEqual(executedJobs.length, 2);
  });

  it('skips scheduled rebalances when gas price is too high', async () => {
    const now = { t: 1_000_000 };
    const { scheduler, alerts, executedJobs } = makeScheduler([OK], now, { gasPrice: 250_000 });

    const executions = await scheduler.tick(now.t);
    assert.strictEqual(executions.length, 1);
    assert.strictEqual(executions[0].skipped, 'gas_price_too_high');
    assert.strictEqual(executedJobs.length, 0);
    assert.ok(alerts.some((a) => a.type === 'rebalance_skipped_gas'));
    assert.strictEqual(scheduler.getStats().totalSkippedGas, 1);
  });


  it('event-driven TVL changes enqueue high-priority jobs', async () => {
    const now = { t: 1_000_000 };
    const { scheduler } = makeScheduler([OK], now);

    // 4% change: below the 500 bps (5%) trigger.
    assert.strictEqual(scheduler.onTvlObservation(1_000_000, 1_040_000), false);
    // 10% change: triggers.
    assert.strictEqual(scheduler.onTvlObservation(1_000_000, 1_100_000), true);

    const executions = await scheduler.tick(now.t);
    assert.strictEqual(executions.length, 2); // tvl_change + scheduled
    assert.strictEqual(executions[0].trigger, 'tvl_change');
    assert.strictEqual(executions[1].trigger, 'scheduled');
  });

  it('orders the queue by priority (utilization spike before scheduled)', async () => {
    const now = { t: 1_000_000 };
    const { scheduler } = makeScheduler([OK, OK], now);

    scheduler.enqueue('scheduled');
    scheduler.onUtilizationObservation('phoenix', 9_800);

    const executions = await scheduler.tick(now.t);
    assert.deepStrictEqual(
      executions.map((e) => e.trigger),
      ['utilization_spike', 'scheduled'],
    );
  });

  it('trips the circuit breaker after consecutive failures and pauses', async () => {
    const now = { t: 1_000_000 };
    const { scheduler, alerts } = makeScheduler([FAIL], now, { threshold: 3 });

    await scheduler.tick(now.t); // failure 1
    now.t += 4 * HOUR;
    await scheduler.tick(now.t); // failure 2
    now.t += 4 * HOUR;
    await scheduler.tick(now.t); // failure 3 -> trips

    assert.strictEqual(scheduler.isCircuitBreakerTripped(), true);
    assert.ok(alerts.some((a) => a.type === 'circuit_breaker_tripped'));

    // Paused: even with due jobs nothing executes.
    now.t += 4 * HOUR;
    const executions = await scheduler.tick(now.t);
    assert.strictEqual(executions.length, 0);

    // Operator resets the breaker.
    scheduler.resetCircuitBreaker();
    assert.strictEqual(scheduler.isCircuitBreakerTripped(), false);
  });

  it('logs history with before/after APY attribution and stats', async () => {
    const now = { t: 1_000_000 };
    const { scheduler } = makeScheduler([OK, FAIL], now, { threshold: 99 });

    await scheduler.tick(now.t);
    now.t += 4 * HOUR;
    await scheduler.tick(now.t);

    const history = scheduler.getHistory();
    assert.strictEqual(history.length, 2);
    assert.strictEqual(history[0].success, true);
    assert.strictEqual(history[0].beforeApy, 0.06);
    assert.strictEqual(history[0].afterApy, 0.085);
    assert.ok(Math.abs((history[0].apyDelta ?? 0) - 0.025) < 1e-12);
    assert.strictEqual(history[1].success, false);
    assert.strictEqual(history[1].error, 'pool_exhausted');

    const stats = scheduler.getStats();
    assert.strictEqual(stats.totalSucceeded, 1);
    assert.strictEqual(stats.totalFailed, 1);
    // (12000 + 0) / 2 executions
    assert.strictEqual(stats.avgGasCostStroops, 6_000);
    assert.strictEqual(stats.queueDepth, 0);
  });

  it('alerts when scheduled rebalances are missed for 2x the interval', async () => {
    const now = { t: 1_000_000 };
    const { scheduler, alerts } = makeScheduler([OK, FAIL], now, { threshold: 99 });

    await scheduler.tick(now.t); // establishes lastRunAt
    now.t += 2 * INTERVAL_MS['4h'];
    await scheduler.tick(now.t);

    assert.ok(alerts.some((a) => a.type === 'scheduled_rebalance_missed'));
  });

  it('supports owner-configurable interval changes at runtime', async () => {
    const now = { t: 1_000_000 };
    const { scheduler, executedJobs } = makeScheduler([OK], now, { interval: 'daily' });

    await scheduler.tick(now.t);
    now.t += INTERVAL_MS['4h'];
    assert.strictEqual((await scheduler.tick(now.t)).length, 0); // not due under daily

    scheduler.setInterval('hourly');
    assert.strictEqual(scheduler.getInterval(), 'hourly');
    now.t += HOUR;
    const executions = await scheduler.tick(now.t);
    assert.strictEqual(executions.length, 1);
    assert.strictEqual(executedJobs.length, 2);
  });
});
