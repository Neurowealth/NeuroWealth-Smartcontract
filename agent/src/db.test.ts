import { describe, it } from 'node:test';
import assert from 'node:assert';
import { pool, getPoolMetrics } from './db';

describe('Database Connection Pool (#711)', () => {
  it('exports pool instance and metrics helper', () => {
    assert.ok(pool);
    const metrics = getPoolMetrics();
    assert.strictEqual(typeof metrics.totalCount, 'number');
    assert.strictEqual(typeof metrics.idleCount, 'number');
    assert.strictEqual(typeof metrics.waitingCount, 'number');
    assert.strictEqual(metrics.maxPoolSize, 10);
    assert.strictEqual(metrics.minPoolSize, 2);
  });
});
