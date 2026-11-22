import { describe, it, after } from 'node:test';
import assert from 'node:assert';
import {
  pool,
  getPool,
  initPool,
  closePool,
  getPoolStatus,
  isPoolClosed,
  isPoolClosing,
  getPoolMetrics,
} from './db';

describe('Database Connection Pool Lifecycle (#711, #750)', () => {
  after(async () => {
    // Ensure pool is closed after tests complete
    await closePool(1000);
  });

  it('exports pool instance and metrics helper', () => {
    assert.ok(pool);
    const metrics = getPoolMetrics();
    assert.strictEqual(typeof metrics.totalCount, 'number');
    assert.strictEqual(typeof metrics.idleCount, 'number');
    assert.strictEqual(typeof metrics.waitingCount, 'number');
    assert.strictEqual(metrics.maxPoolSize, 10);
    assert.strictEqual(metrics.minPoolSize, 2);
    assert.strictEqual(metrics.status, 'active');
  });

  it('reports active lifecycle status initially', () => {
    assert.strictEqual(getPoolStatus(), 'active');
    assert.strictEqual(isPoolClosing(), false);
    assert.strictEqual(isPoolClosed(), false);
    assert.ok(getPool());
  });

  it('allows initializing pool with custom configuration', () => {
    const customPool = initPool({
      max: 5,
      min: 1,
      idleTimeoutMillis: 10000,
      connectionTimeoutMillis: 2000,
    });
    assert.ok(customPool);
    assert.strictEqual(getPoolStatus(), 'active');
    assert.strictEqual(getPool(), customPool);
  });

  it('closes pool gracefully and is idempotent', async () => {
    const p1 = closePool(2000);
    assert.strictEqual(isPoolClosing() || isPoolClosed(), true);
    const p2 = closePool(2000);
    assert.strictEqual(p1, p2); // idempotent promise

    await p1;
    assert.strictEqual(isPoolClosed(), true);
    assert.strictEqual(isPoolClosing(), false);
    assert.strictEqual(getPoolStatus(), 'closed');

    const closedMetrics = getPoolMetrics();
    assert.strictEqual(closedMetrics.totalCount, 0);
    assert.strictEqual(closedMetrics.status, 'closed');

    // Calling closePool when already closed is a no-op
    await closePool();
    assert.strictEqual(isPoolClosed(), true);
  });

  it('can reinitialize pool after closure', () => {
    const newPool = initPool({ max: 8 });
    assert.ok(newPool);
    assert.strictEqual(getPoolStatus(), 'active');
    assert.strictEqual(isPoolClosed(), false);
  });
});
