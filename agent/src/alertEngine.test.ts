import { describe, it, mock } from 'node:test';
import assert from 'node:assert';
import logger from './logger';
import { processEventForAlerts } from './alertEngine';
import { parseAlertEventPayload } from './alertEventPayload';

describe('alert event schema', () => {
  it('rejects missing or incorrectly typed event types', () => {
    assert.deepStrictEqual(parseAlertEventPayload({ amount: 10, user: null, ledger: 1 }), {
      ok: false,
      reason: 'alert event payload requires a type',
    });
    assert.deepStrictEqual(parseAlertEventPayload({ type: 42, amount: 10, user: null, ledger: 1 }), {
      ok: false,
      reason: 'alert event payload requires a type',
    });
  });

  it('propagates amount into alert thresholds', async () => {
    const below = await processEventForAlerts({ type: 'withdraw', amount: 100_000, user: null, ledger: 10 });
    const above = await processEventForAlerts({ type: 'withdraw', amount: 250_000, user: null, ledger: 11 });

    assert.deepStrictEqual(below, []);
    assert.deepStrictEqual(above, ['TVL_ANOMALY', 'LARGE_WITHDRAWAL']);
  });

  it('drops unknown event types with a warning', async () => {
    const warning = mock.method(logger, 'warn', () => logger);
    const triggered = await processEventForAlerts({ type: 'mystery', amount: 250_000, user: null, ledger: 12 });

    assert.deepStrictEqual(triggered, []);
    assert.strictEqual(warning.mock.callCount(), 1);
    warning.mock.restore();
  });

  it('supports configurable alert thresholds via environment (#758)', async () => {
    // Override threshold via env
    process.env.ALERT_LARGE_WITHDRAWAL_THRESHOLD = '50000';
    process.env.ALERT_TVL_DROP_PERCENTAGE = '10';

    try {
      // 60,000 is above 50,000 threshold (triggers LARGE_WITHDRAWAL)
      const triggered = await processEventForAlerts({ type: 'withdraw', amount: 60_000, user: null, ledger: 20 });
      assert.ok(triggered.includes('LARGE_WITHDRAWAL'));
    } finally {
      delete process.env.ALERT_LARGE_WITHDRAWAL_THRESHOLD;
      delete process.env.ALERT_TVL_DROP_PERCENTAGE;
    }
  });
});
