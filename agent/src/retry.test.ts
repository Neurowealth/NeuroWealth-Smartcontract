import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  DatabaseRetryDeadlineError,
  getDatabaseRetryMetrics,
  isRetryableDatabaseError,
  resetDatabaseRetryMetrics,
  withDatabaseRetry,
} from './retry';

describe('database retry policy (#865)', () => {
  it('retries serialization, deadlock, and connection failures while keeping operation identity stable', async () => {
    for (const code of ['40001', '40P01', '08006', 'ECONNRESET']) {
      resetDatabaseRetryMetrics();
      let calls = 0;
      const operationId = `operation-${code}`;
      const seenIds: string[] = [];
      const result = await withDatabaseRetry(async () => {
        calls += 1;
        seenIds.push(operationId);
        if (calls === 1) throw Object.assign(new Error('temporary database failure'), { code });
        return 'committed';
      }, 'test.transaction', {
        baseDelayMs: 1,
        maxDelayMs: 2,
        random: () => 0,
        sleep: async () => undefined,
      });

      assert.strictEqual(result, 'committed');
      assert.deepStrictEqual(seenIds, [operationId, operationId]);
      assert.strictEqual(getDatabaseRetryMetrics().recovered, 1);
    }
  });

  it('does not retry permanent errors', async () => {
    resetDatabaseRetryMetrics();
    let calls = 0;
    const error = Object.assign(new Error('invalid query'), { code: '23505' });
    await assert.rejects(withDatabaseRetry(async () => {
      calls += 1;
      throw error;
    }, 'permanent', { sleep: async () => undefined }), error);
    assert.strictEqual(calls, 1);
    assert.strictEqual(getDatabaseRetryMetrics().permanentFailures, 1);
    assert.strictEqual(isRetryableDatabaseError(error), false);
  });

  it('enforces a configurable total deadline and records the terminal outcome', async () => {
    resetDatabaseRetryMetrics();
    let now = 0;
    let calls = 0;
    await assert.rejects(withDatabaseRetry(async () => {
      calls += 1;
      throw Object.assign(new Error('deadlock'), { code: '40P01' });
    }, 'deadline', {
      totalDeadlineMs: 5,
      baseDelayMs: 10,
      maxDelayMs: 10,
      random: () => 0,
      now: () => now,
      sleep: async (delayMs) => { now += delayMs; },
    }), DatabaseRetryDeadlineError);
    assert.strictEqual(calls, 1);
    assert.strictEqual(getDatabaseRetryMetrics().deadlineExceeded, 1);
  });

  it('awaits the in-flight operation before propagating the deadline error', async () => {
    resetDatabaseRetryMetrics();
    let operationSettled = false;
    const operation = () => new Promise<string>((resolve) => {
      setTimeout(() => {
        operationSettled = true;
        resolve('done');
      }, 20);
    });

    await assert.rejects(
      withDatabaseRetry(operation, 'test.deadline-await', {
        totalDeadlineMs: 5,
        baseDelayMs: 10,
        maxDelayMs: 10,
        random: () => 0,
        sleep: async () => undefined,
      }),
      DatabaseRetryDeadlineError,
    );
    assert.strictEqual(operationSettled, true);
    assert.strictEqual(getDatabaseRetryMetrics().deadlineExceeded, 1);
  });
});