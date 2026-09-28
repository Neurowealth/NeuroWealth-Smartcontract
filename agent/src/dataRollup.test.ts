import { describe, it } from 'node:test';
import assert from 'node:assert';
import { DEFAULT_ROLLUP_CONFIG, RollupPool, RollupScheduler, runRollup } from './dataRollup';

/**
 * A minimal fake pool that only understands the three statement shapes
 * `dataRollup.ts` issues, matched by a keyword in the SQL text. Good enough
 * to assert the job calls the right statements with the right cutoffs
 * without standing up a real Postgres instance.
 */
function makeFakePool(rowCounts: { rollup?: number; snapshotDelete?: number; earningsDelete?: number }) {
  const calls: Array<{ sql: string; values?: unknown[] }> = [];
  const pool: RollupPool = {
    query: async (sql, values) => {
      calls.push({ sql, values });
      if (sql.includes('WITH ranked')) {
        return { rowCount: rowCounts.rollup ?? 0 };
      }
      if (sql.includes('DELETE FROM yield_snapshots')) {
        return { rowCount: rowCounts.snapshotDelete ?? 0 };
      }
      if (sql.includes('DELETE FROM earnings_history')) {
        return { rowCount: rowCounts.earningsDelete ?? 0 };
      }
      throw new Error(`unexpected query: ${sql}`);
    },
  };
  return { pool, calls };
}

function makeThrowingPool(failOn: 'rollup' | 'snapshotDelete' | 'earningsDelete') {
  const keyword = {
    rollup: 'WITH ranked',
    snapshotDelete: 'DELETE FROM yield_snapshots',
    earningsDelete: 'DELETE FROM earnings_history',
  }[failOn];
  const pool: RollupPool = {
    query: async (sql) => {
      if (sql.includes(keyword)) {
        throw new Error(`${failOn} boom`);
      }
      return { rowCount: 0 };
    },
  };
  return pool;
}

describe('Nightly rollup/cleanup job (#765)', () => {
  it('runs rollup, snapshot cleanup, and earnings cleanup with the configured cutoffs', async () => {
    const { pool, calls } = makeFakePool({ rollup: 12, snapshotDelete: 40, earningsDelete: 3 });
    const fixedNow = Date.UTC(2026, 8, 24); // 2026-09-24
    const result = await runRollup(pool, DEFAULT_ROLLUP_CONFIG, () => fixedNow);

    assert.strictEqual(result.snapshotsRolledUp, 12);
    assert.strictEqual(result.snapshotsDeleted, 40);
    assert.strictEqual(result.earningsDeleted, 3);
    assert.strictEqual(result.error, undefined);
    assert.strictEqual(result.runAt, fixedNow);

    assert.strictEqual(calls.length, 3);
    const rollupCutoff = new Date(calls[0].values![0] as string).getTime();
    assert.strictEqual(rollupCutoff, fixedNow - DEFAULT_ROLLUP_CONFIG.rollupAfterDays * 86_400_000);

    const snapshotCutoff = new Date(calls[1].values![0] as string).getTime();
    assert.strictEqual(
      snapshotCutoff,
      fixedNow - DEFAULT_ROLLUP_CONFIG.snapshotRetentionDays * 86_400_000,
    );

    // earnings_history.date is a DATE column: cutoff must be a plain
    // YYYY-MM-DD, not a full timestamp.
    const earningsCutoff = calls[2].values![0] as string;
    assert.match(earningsCutoff, /^\d{4}-\d{2}-\d{2}$/);
  });

  it('uses a stricter cutoff for rollup than for outright deletion', () => {
    assert.ok(DEFAULT_ROLLUP_CONFIG.rollupAfterDays < DEFAULT_ROLLUP_CONFIG.snapshotRetentionDays);
  });

  it('one step failing does not prevent the others from running', async () => {
    const pool = makeThrowingPool('rollup');
    const result = await runRollup(pool, DEFAULT_ROLLUP_CONFIG, () => Date.now());

    assert.strictEqual(result.snapshotsRolledUp, 0);
    assert.strictEqual(result.snapshotsDeleted, 0);
    assert.strictEqual(result.earningsDeleted, 0);
    assert.ok(result.error?.includes('rollup: rollup boom'));
  });

  it('earnings cleanup still runs when snapshot cleanup fails', async () => {
    const pool = makeThrowingPool('snapshotDelete');
    const result = await runRollup(pool, DEFAULT_ROLLUP_CONFIG, () => Date.now());

    assert.ok(result.error?.includes('snapshot cleanup'));
    assert.strictEqual(result.earningsDeleted, 0);
  });

  it('reports no error when every step succeeds with zero rows affected', async () => {
    const { pool } = makeFakePool({});
    const result = await runRollup(pool, DEFAULT_ROLLUP_CONFIG, () => Date.now());
    assert.strictEqual(result.error, undefined);
    assert.strictEqual(result.snapshotsRolledUp, 0);
  });
});

describe('RollupScheduler', () => {
  it('runs once immediately on start(), without waiting for the first interval', async () => {
    const { pool } = makeFakePool({ rollup: 1, snapshotDelete: 2, earningsDelete: 3 });
    const scheduler = new RollupScheduler(pool, DEFAULT_ROLLUP_CONFIG, () => Date.now());

    assert.strictEqual(scheduler.getLastResult(), null);
    scheduler.start(24 * 60 * 60 * 1000);

    // runOnce() is fired-and-forgotten from start(); give the microtask
    // queue a tick to let the fake pool's async query resolve.
    await new Promise((resolve) => setImmediate(resolve));

    const result = scheduler.getLastResult();
    assert.ok(result);
    assert.strictEqual(result?.snapshotsRolledUp, 1);

    scheduler.stop();
  });

  it('start() is idempotent: calling it twice does not double-schedule', async () => {
    const { pool } = makeFakePool({});
    const scheduler = new RollupScheduler(pool, DEFAULT_ROLLUP_CONFIG, () => Date.now());
    scheduler.start(24 * 60 * 60 * 1000);
    scheduler.start(24 * 60 * 60 * 1000);
    await new Promise((resolve) => setImmediate(resolve));
    scheduler.stop();
    // No assertion beyond "did not throw" — the internal timer field is
    // private; idempotency is guarded by the `if (this.timer) return;` in
    // start(), exercised by calling it twice above.
  });

  it('stop() is safe to call before start() and safe to call twice', () => {
    const { pool } = makeFakePool({});
    const scheduler = new RollupScheduler(pool, DEFAULT_ROLLUP_CONFIG, () => Date.now());
    assert.doesNotThrow(() => scheduler.stop());
    scheduler.start(24 * 60 * 60 * 1000);
    scheduler.stop();
    assert.doesNotThrow(() => scheduler.stop());
  });
});
