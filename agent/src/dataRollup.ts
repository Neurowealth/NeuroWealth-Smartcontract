/**
 * Nightly rollup/cleanup job for yield_snapshots and earnings_history
 * (Issue #765).
 *
 * `yield_snapshots` is written at high frequency (every hourly decision-loop
 * tick, see `index.ts`) purely to drive chart rendering and APY trend
 * calculation. Kept forever at that granularity it grows unbounded, but the
 * dashboard only ever needs daily resolution once a snapshot is more than a
 * few days old. This job:
 *
 *  1. **Rolls up** snapshots older than `rollupAfterDays` into one
 *     representative row per (user, day) — the last snapshot recorded that
 *     day, which is what a "close of day" chart point should show — and
 *     deletes the rest of that day's rows.
 *  2. **Deletes** snapshot rows older than `snapshotRetentionDays` outright
 *     (even the rolled-up ones), since the dashboard doesn't chart history
 *     that old.
 *  3. **Deletes** `earnings_history` rows older than `earningsRetentionDays`.
 *     Unlike snapshots there is already exactly one row per user per day
 *     (`unique_user_daily_earnings`), so there is nothing to roll up — only
 *     pruning applies.
 *
 * All three steps are plain idempotent SQL (safe to re-run, and safe if a
 * previous run partially completed) executed against the shared pg pool, so
 * the job itself only needs to own scheduling, error handling, and metrics.
 */

import logger from './logger';

export interface RollupConfig {
  /** Snapshots older than this are collapsed to one row per user/day. */
  rollupAfterDays: number;
  /** Snapshot rows (rolled up or not) older than this are deleted. */
  snapshotRetentionDays: number;
  /** earnings_history rows older than this are deleted. */
  earningsRetentionDays: number;
}

export const DEFAULT_ROLLUP_CONFIG: RollupConfig = {
  rollupAfterDays: 7,
  snapshotRetentionDays: 90,
  earningsRetentionDays: 400,
};

export interface RollupResult {
  runAt: number;
  snapshotsRolledUp: number;
  snapshotsDeleted: number;
  earningsDeleted: number;
  error?: string;
}

interface QueryResult {
  rowCount: number | null;
}

export interface RollupPool {
  query: (sql: string, values?: unknown[]) => Promise<QueryResult>;
}

/**
 * Collapses same-day yield_snapshots older than `rollupAfterDays` into a
 * single row per (user_id, day) — keeping the chronologically last snapshot
 * of that day — and removes the rest. Runs entirely in SQL so it is atomic
 * per invocation and safe to re-run (a day already collapsed to one row is a
 * no-op on the next run).
 *
 * `date_trunc` buckets in the connection's session timezone unless told
 * otherwise; the explicit `AT TIME ZONE 'UTC'` keeps "day" boundaries fixed
 * regardless of what `TimeZone` the pooled connection happens to have, so
 * the same timestamp always rolls up into the same day.
 */
async function rollupYieldSnapshots(pool: RollupPool, cutoffIso: string): Promise<number> {
  const result = await pool.query(
    `
    WITH ranked AS (
      SELECT
        id,
        ROW_NUMBER() OVER (
          PARTITION BY user_id, date_trunc('day', timestamp AT TIME ZONE 'UTC')
          ORDER BY timestamp DESC
        ) AS rn
      FROM yield_snapshots
      WHERE timestamp < $1
    )
    DELETE FROM yield_snapshots
    WHERE id IN (SELECT id FROM ranked WHERE rn > 1)
    `,
    [cutoffIso],
  );
  return result.rowCount ?? 0;
}

/** Deletes yield_snapshots older than `snapshotRetentionDays`. */
async function deleteOldSnapshots(pool: RollupPool, cutoffIso: string): Promise<number> {
  const result = await pool.query('DELETE FROM yield_snapshots WHERE timestamp < $1', [cutoffIso]);
  return result.rowCount ?? 0;
}

/** Deletes earnings_history rows older than `earningsRetentionDays`. */
async function deleteOldEarnings(pool: RollupPool, cutoffDate: string): Promise<number> {
  const result = await pool.query('DELETE FROM earnings_history WHERE date < $1', [cutoffDate]);
  return result.rowCount ?? 0;
}

function daysAgoIso(now: number, days: number): string {
  return new Date(now - days * 86_400_000).toISOString();
}

function daysAgoDateOnly(now: number, days: number): string {
  return daysAgoIso(now, days).slice(0, 10);
}

/**
 * Runs the full rollup+cleanup pass once. Each step is independent: a
 * failure in one is logged and surfaced via `error`, but does not prevent
 * the others from running, so a transient failure on one table doesn't
 * block cleanup of the other.
 */
export async function runRollup(
  pool: RollupPool,
  config: RollupConfig = DEFAULT_ROLLUP_CONFIG,
  now: () => number = () => Date.now(),
): Promise<RollupResult> {
  const runAt = now();
  const errors: string[] = [];

  let snapshotsRolledUp = 0;
  try {
    snapshotsRolledUp = await rollupYieldSnapshots(pool, daysAgoIso(runAt, config.rollupAfterDays));
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    logger.error({ error: message }, 'yield_snapshots rollup failed');
    errors.push(`rollup: ${message}`);
  }

  let snapshotsDeleted = 0;
  try {
    snapshotsDeleted = await deleteOldSnapshots(pool, daysAgoIso(runAt, config.snapshotRetentionDays));
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    logger.error({ error: message }, 'yield_snapshots retention cleanup failed');
    errors.push(`snapshot cleanup: ${message}`);
  }

  let earningsDeleted = 0;
  try {
    earningsDeleted = await deleteOldEarnings(pool, daysAgoDateOnly(runAt, config.earningsRetentionDays));
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    logger.error({ error: message }, 'earnings_history retention cleanup failed');
    errors.push(`earnings cleanup: ${message}`);
  }

  const result: RollupResult = {
    runAt,
    snapshotsRolledUp,
    snapshotsDeleted,
    earningsDeleted,
    ...(errors.length > 0 ? { error: errors.join('; ') } : {}),
  };

  if (errors.length > 0) {
    logger.warn({ result }, 'Nightly rollup completed with errors');
  } else {
    logger.info({ result }, 'Nightly rollup completed');
  }

  return result;
}

/**
 * Wall-clock scheduler for `runRollup`, mirroring the start/stop lifecycle
 * used by `RebalanceScheduler` and `StateBackupManager`. Defaults to a
 * 24-hour cadence (the "nightly" job); the first run fires immediately on
 * `start()` rather than waiting a full day, so a freshly deployed agent
 * doesn't accumulate a full day of un-rolled-up snapshots before its first
 * cleanup.
 */
export class RollupScheduler {
  private timer: ReturnType<typeof setInterval> | null = null;
  private lastResult: RollupResult | null = null;

  constructor(
    private readonly pool: RollupPool,
    private readonly config: RollupConfig = DEFAULT_ROLLUP_CONFIG,
    private readonly now: () => number = () => Date.now(),
  ) {}

  start(intervalMs = 24 * 60 * 60 * 1000): void {
    if (this.timer) return;
    void this.runOnce();
    this.timer = setInterval(() => {
      void this.runOnce();
    }, intervalMs);
    logger.info({ intervalMs }, 'Nightly rollup scheduler started');
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
      logger.info('Nightly rollup scheduler stopped');
    }
  }

  private async runOnce(): Promise<void> {
    try {
      this.lastResult = await runRollup(this.pool, this.config, this.now);
    } catch (err) {
      logger.error(
        { error: err instanceof Error ? err.message : err },
        'Nightly rollup run threw unexpectedly',
      );
    }
  }

  getLastResult(): RollupResult | null {
    return this.lastResult;
  }
}
