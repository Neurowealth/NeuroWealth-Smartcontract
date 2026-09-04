/**
 * Automated rebalancing scheduler with configurable intervals (Issue #651).
 *
 * Features:
 *  - Configurable intervals: `hourly`, `4h`, `daily` (owner setting)
 *  - Gas-price awareness: scheduled rebalances are skipped while network fees
 *    exceed `maxGasPriceStroops` (an alert is emitted instead of a rebalance)
 *  - Event-driven triggers: large TVL changes and protocol utilization spikes
 *  - Priority-ordered queue: event-driven jobs preempt scheduled ones
 *  - Circuit breaker: pauses scheduling after N consecutive failures
 *  - History logging with before/after APY and gas-cost attribution
 *  - Alert hooks for failed / missed / gas-skipped rebalances
 *
 * The scheduler is transport-agnostic: rebalance execution is injected via
 * `hooks.executor`, so unit tests can stub it and production can bind it to
 * the vault client's `rebalance` entrypoint.
 */

import logger from './logger';

export type RebalanceInterval = 'hourly' | '4h' | 'daily';

/** Interval length in milliseconds. */
export const INTERVAL_MS: Record<RebalanceInterval, number> = {
  hourly: 3_600_000,
  '4h': 14_400_000,
  daily: 86_400_000,
};

export type RebalanceTrigger =
  | 'scheduled'
  | 'tvl_change'
  | 'utilization_spike'
  | 'manual';

/** A queued rebalance request. Lower `priority` runs first. */
export interface RebalanceJob {
  id: string;
  trigger: RebalanceTrigger;
  priority: number;
  dueAt: number;
  createdAt: number;
  payload?: Record<string, unknown>;
}

/** Outcome of one executed (or skipped) job. */
export interface RebalanceExecution {
  jobId: string;
  trigger: RebalanceTrigger;
  executedAt: number;
  success: boolean;
  skipped?: 'gas_price_too_high';
  beforeApy?: number;
  afterApy?: number;
  apyDelta?: number;
  gasCostStroops?: number;
  error?: string;
}

export interface RebalanceExecutionResult {
  success: boolean;
  beforeApy?: number;
  afterApy?: number;
  gasCostStroops?: number;
  error?: string;
}

export type RebalanceExecutor = (
  job: RebalanceJob,
) => RebalanceExecutionResult | Promise<RebalanceExecutionResult>;

export type SchedulerAlertType =
  | 'circuit_breaker_tripped'
  | 'rebalance_failed'
  | 'rebalance_skipped_gas'
  | 'scheduled_rebalance_missed';

export interface SchedulerAlert {
  type: SchedulerAlertType;
  message: string;
  at: number;
  context?: Record<string, unknown>;
}

export interface SchedulerConfig {
  interval: RebalanceInterval;
  /** Scheduled rebalances are skipped above this gas price (stroops). */
  maxGasPriceStroops?: number;
  /** Consecutive failures before the circuit breaker pauses scheduling. */
  circuitBreakerThreshold: number;
  /** Enqueue an event-driven job when |ΔTVL| >= this many bps. */
  tvlChangeTriggerBps: number;
}

export const DEFAULT_SCHEDULER_CONFIG: SchedulerConfig = {
  interval: '4h',
  maxGasPriceStroops: 100_000,
  circuitBreakerThreshold: 3,
  tvlChangeTriggerBps: 500,
};

export interface SchedulerHooks {
  executor: RebalanceExecutor;
  gasPriceProvider?: () => Promise<number>;
  onAlert?: (alert: SchedulerAlert) => void;
  /** Injectable clock for deterministic tests. */
  now?: () => number;
}


/** Aggregated scheduler statistics for dashboards (#652). */
export interface SchedulerStats {
  totalEnqueued: number;
  totalExecuted: number;
  totalSucceeded: number;
  totalFailed: number;
  totalSkippedGas: number;
  avgGasCostStroops: number;
  avgApyDelta: number;
  consecutiveFailures: number;
  circuitBreakerTripped: boolean;
  queueDepth: number;
}

/** Priority ordering: event-driven > manual > scheduled. */
const PRIORITY: Record<RebalanceTrigger, number> = {
  utilization_spike: 0,
  tvl_change: 1,
  manual: 2,
  scheduled: 3,
};

export class RebalanceScheduler {
  private queue: RebalanceJob[] = [];
  private history: RebalanceExecution[] = [];
  private consecutiveFailures = 0;
  private circuitBreakerTripped = false;
  private jobCounter = 0;
  private lastRunAt: number | null = null;
  private lastMissedCheckAt: number | null = null;
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private config: SchedulerConfig = DEFAULT_SCHEDULER_CONFIG,
    private readonly hooks: SchedulerHooks,
  ) {}

  /** Starts the wall-clock loop (production entrypoint). */
  start(pollMs = 60_000): void {
    if (this.timer) return;
    this.timer = setInterval(() => this.tick(), pollMs);
    logger.info({ interval: this.config.interval, pollMs }, 'Rebalance scheduler started');
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
      logger.info('Rebalance scheduler stopped');
    }
  }

  /** Enqueues a rebalance job; the queue stays sorted by (priority, dueAt). */
  enqueue(
    trigger: RebalanceTrigger,
    dueAt?: number,
    payload?: Record<string, unknown>,
  ): RebalanceJob {
    const now = this.now();
    const job: RebalanceJob = {
      id: `job_${++this.jobCounter}`,
      trigger,
      priority: PRIORITY[trigger],
      dueAt: dueAt ?? now,
      createdAt: now,
      payload,
    };
    this.queue.push(job);
    this.queue.sort((a, b) => a.priority - b.priority || a.dueAt - b.dueAt);
    logger.debug({ jobId: job.id, trigger }, 'Rebalance job enqueued');
    return job;
  }

  /**
   * Event-driven hook: call after every TVL observation. Enqueues a
   * high-priority job when the TVL moved by at least `tvlChangeTriggerBps`.
   */
  onTvlObservation(tvlBefore: number, tvlAfter: number): boolean {
    if (tvlBefore <= 0) return false;
    const changeBps = Math.round(
      (Math.abs(tvlAfter - tvlBefore) / tvlBefore) * 10_000,
    );
    if (changeBps >= this.config.tvlChangeTriggerBps) {
      this.enqueue('tvl_change', undefined, { tvlBefore, tvlAfter, changeBps });
      return true;
    }
    return false;
  }

  /** Event-driven hook: protocol utilization spike (bps of optimal). */
  onUtilizationObservation(protocol: string, utilizationBps: number): boolean {
    if (utilizationBps >= 9_500) {
      this.enqueue('utilization_spike', undefined, { protocol, utilizationBps });
      return true;
    }
    return false;
  }

  /**
   * Processes due jobs and emits the scheduled job when the interval elapsed.
   * Safe to call repeatedly; the wall-clock loop calls it on a timer.
   */
  async tick(nowOverride?: number): Promise<RebalanceExecution[]> {
    const now = nowOverride ?? this.now();

    if (this.circuitBreakerTripped) {
      logger.warn('Circuit breaker open: scheduling paused');
      return [];
    }

    // Interval-based schedule emission. Avoid stacking duplicate scheduled
    // jobs when one is already queued (e.g. manually enqueued or carried
    // over from a previous partial tick).
    const hasPendingScheduled = this.queue.some((j) => j.trigger === 'scheduled');
    if (
      !hasPendingScheduled &&
      (this.lastRunAt === null ||
        now - this.lastRunAt >= INTERVAL_MS[this.config.interval])
    ) {
      this.enqueue('scheduled');
    }

    // Missed-schedule detection: no run for 2x the interval while healthy.
    if (this.lastRunAt !== null) {
      const lastCheck = this.lastMissedCheckAt ?? 0;
      if (
        now - this.lastRunAt >= 2 * INTERVAL_MS[this.config.interval] &&
        now - lastCheck >= INTERVAL_MS[this.config.interval]
      ) {
        this.emitAlert(
          'scheduled_rebalance_missed',
          `No rebalance for ${now - this.lastRunAt}ms`,
          { lastRunAt: this.lastRunAt },
        );
        this.lastMissedCheckAt = now;
      }
    }

    const executed: RebalanceExecution[] = [];
    while (
      this.queue.length > 0 &&
      this.queue[0].dueAt <= now &&
      !this.circuitBreakerTripped
    ) {
      const job = this.queue.shift()!;
      executed.push(await this.runJob(job, now));
    }
    return executed;
  }

  /** Owner setting: change the rebalance interval at runtime. */
  setInterval(interval: RebalanceInterval): void {
    this.config = { ...this.config, interval };
    logger.info({ interval }, 'Rebalance interval updated');
  }

  getInterval(): RebalanceInterval {
    return this.config.interval;
  }

  /** Manually resets the circuit breaker (operator action). */
  resetCircuitBreaker(): void {
    this.circuitBreakerTripped = false;
    this.consecutiveFailures = 0;
    logger.info('Scheduler circuit breaker reset');
  }

  getHistory(): RebalanceExecution[] {
    return [...this.history];
  }

  getQueue(): RebalanceJob[] {
    return [...this.queue];
  }

  isCircuitBreakerTripped(): boolean {
    return this.circuitBreakerTripped;
  }

  getStats(): SchedulerStats {
    const executed = this.history.filter((h) => !h.skipped);
    const succeeded = executed.filter((h) => h.success);
    const failed = executed.filter((h) => !h.success);
    const skippedGas = this.history.filter((h) => h.skipped === 'gas_price_too_high');
    const gasCosts = executed.map((h) => h.gasCostStroops ?? 0);
    const apyDeltas = executed
      .map((h) => h.apyDelta)
      .filter((d): d is number => typeof d === 'number');

    const avg = (xs: number[]) =>
      xs.length === 0 ? 0 : xs.reduce((a, b) => a + b, 0) / xs.length;

    return {
      totalEnqueued: this.history.length + this.queue.length,
      totalExecuted: executed.length,
      totalSucceeded: succeeded.length,
      totalFailed: failed.length,
      totalSkippedGas: skippedGas.length,
      avgGasCostStroops: avg(gasCosts),
      avgApyDelta: avg(apyDeltas),
      consecutiveFailures: this.consecutiveFailures,
      circuitBreakerTripped: this.circuitBreakerTripped,
      queueDepth: this.queue.length,
    };
  }

  private now(): number {
    return this.hooks.now ? this.hooks.now() : Date.now();
  }

  private async runJob(job: RebalanceJob, now: number): Promise<RebalanceExecution> {
    // Gas-price awareness: skip scheduled rebalances while fees are high.
    if (
      job.trigger === 'scheduled' &&
      this.config.maxGasPriceStroops !== undefined &&
      this.hooks.gasPriceProvider
    ) {
      try {
        const gasPrice = await this.hooks.gasPriceProvider();
        if (gasPrice > this.config.maxGasPriceStroops) {
          logger.info({ jobId: job.id, gasPrice }, 'Skipping scheduled rebalance: gas too high');
          const skipped: RebalanceExecution = {
            jobId: job.id,
            trigger: job.trigger,
            executedAt: now,
            success: false,
            skipped: 'gas_price_too_high',
          };
          this.history.push(skipped);
          this.emitAlert(
            'rebalance_skipped_gas',
            `Gas ${gasPrice} above ceiling ${this.config.maxGasPriceStroops}`,
            { jobId: job.id, gasPrice },
          );
          return skipped;
        }
      } catch (err) {
        logger.warn(
          { err: err instanceof Error ? err.message : err },
          'Gas price probe failed; proceeding with rebalance',
        );
      }
    }

    let execution: RebalanceExecution;
    try {
      const result = await this.hooks.executor(job);
      const success = result.success;
      execution = {
        jobId: job.id,
        trigger: job.trigger,
        executedAt: now,
        success,
        beforeApy: result.beforeApy,
        afterApy: result.afterApy,
        apyDelta:
          typeof result.beforeApy === 'number' && typeof result.afterApy === 'number'
            ? result.afterApy - result.beforeApy
            : undefined,
        gasCostStroops: result.gasCostStroops,
        error: result.error,
      };
    } catch (err) {
      execution = {
        jobId: job.id,
        trigger: job.trigger,
        executedAt: now,
        success: false,
        error: err instanceof Error ? err.message : String(err),
      };
    }

    this.history.push(execution);
    this.lastRunAt = now;

    if (execution.success) {
      this.consecutiveFailures = 0;
      logger.info(
        {
          jobId: job.id,
          trigger: job.trigger,
          apyDelta: execution.apyDelta,
          gasCostStroops: execution.gasCostStroops,
        },
        'Rebalance executed',
      );
    } else {
      this.consecutiveFailures += 1;
      logger.warn(
        { jobId: job.id, trigger: job.trigger, error: execution.error, consecutiveFailures: this.consecutiveFailures },
        'Rebalance failed',
      );
      this.emitAlert('rebalance_failed', execution.error || 'rebalance failed', {
        jobId: job.id,
        trigger: job.trigger,
      });

      if (this.consecutiveFailures >= this.config.circuitBreakerThreshold) {
        this.circuitBreakerTripped = true;
        this.emitAlert(
          'circuit_breaker_tripped',
          `${this.consecutiveFailures} consecutive rebalance failures`,
          { threshold: this.config.circuitBreakerThreshold },
        );
      }
    }

    return execution;
  }

  private emitAlert(type: SchedulerAlertType, message: string, context?: Record<string, unknown>): void {
    const alert: SchedulerAlert = { type, message, at: this.now(), context };
    logger.warn({ alert }, 'Scheduler alert');
    if (this.hooks.onAlert) this.hooks.onAlert(alert);
  }
}

