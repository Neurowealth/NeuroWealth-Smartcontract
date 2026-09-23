import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';
import { alertRules, processEventForAlerts } from './alertEngine';

/**
 * Unit tests for the agent alert engine (#690).
 *
 * processEventForAlerts is the only exported entry point; the transport helpers
 * it fans out to are module-private and only console.log, so the dispatch path
 * is observed through console output rather than by exporting internals for the
 * sake of a test.
 */

const originalLog = console.log;
let lines: string[] = [];

beforeEach(() => {
  lines = [];
  console.log = (...args: unknown[]) => {
    lines.push(args.map(String).join(' '));
  };
});

afterEach(() => {
  console.log = originalLog;
});

const alertLines = () => lines.filter((l) => l.startsWith('[ALERT]'));
/** Rule name out of a line like '[ALERT] [HIGH] LARGE_WITHDRAWAL: ...'. */
const raisedRules = () =>
  alertLines()
    .map((l) => l.match(/\[ALERT\] \[[A-Z]+\] ([A-Z_]+):/)?.[1])
    .filter((name): name is string => Boolean(name));
const sends = (channel: string) => lines.some((l) => l.includes(channel));

describe('alert rules (#690)', () => {
  it('defines the three documented rules with their severities', () => {
    assert.deepStrictEqual(
      alertRules.map((r) => [r.name, r.severity]),
      [
        ['TVL_ANOMALY', 'CRITICAL'],
        ['LARGE_WITHDRAWAL', 'HIGH'],
        ['AUTH_FAILURE', 'MEDIUM'],
      ],
    );
  });

  it('gives every rule a description and a predicate', () => {
    for (const rule of alertRules) {
      assert.ok(rule.description.length > 0, rule.name + ' has no description');
      assert.strictEqual(typeof rule.check, 'function', rule.name + ' has no check');
    }
  });
});

describe('processEventForAlerts dispatch (#690)', () => {
  it('raises the large-withdrawal alert for a withdrawal over the threshold', async () => {
    await processEventForAlerts({ type: 'withdraw', amount: 150_000 });

    assert.deepStrictEqual(alertLines(), [
      '[ALERT] [HIGH] LARGE_WITHDRAWAL: Large withdrawal detected.',
    ]);
  });

  it('raises both withdrawal rules and pages for a withdrawal that is also a TVL anomaly', async () => {
    await processEventForAlerts({ type: 'withdraw', amount: 250_000 });

    const raised = [...raisedRules()].sort();
    assert.deepStrictEqual(raised, ['LARGE_WITHDRAWAL', 'TVL_ANOMALY']);
    // CRITICAL is the only severity that escalates to PagerDuty.
    assert.ok(sends('PagerDuty'), 'a CRITICAL alert should page');
  });

  it('treats exactly 20% of the mocked vault total as not yet anomalous', async () => {
    // The mock state in processEventForAlerts is { totalAssets: 1_000_000 },
    // and the rule compares with '>', so 200_000 is the boundary that must not fire.
    await processEventForAlerts({ type: 'withdraw', amount: 200_000 });

    const raised = raisedRules();
    assert.ok(!raised.includes('TVL_ANOMALY'), 'boundary withdrawal must not be anomalous');
    assert.ok(raised.includes('LARGE_WITHDRAWAL'));
    assert.ok(!sends('PagerDuty'), 'HIGH must not page');
  });

  it('raises the auth-failure alert for an auth_failure event', async () => {
    await processEventForAlerts({ type: 'auth_failure' });

    assert.deepStrictEqual(alertLines(), [
      '[ALERT] [MEDIUM] AUTH_FAILURE: Authentication failure on vault operations.',
    ]);
    assert.ok(!sends('PagerDuty'), 'MEDIUM must not page');
  });

  it('sends every configured channel for a matched rule', async () => {
    await processEventForAlerts({ type: 'withdraw', amount: 150_000 });

    assert.ok(sends('Email sent'), 'email channel missing');
    assert.ok(sends('Telegram message sent'), 'telegram channel missing');
    assert.ok(sends('Discord webhook sent'), 'discord channel missing');
  });

  it('stays silent for an event no rule matches', async () => {
    await processEventForAlerts({ type: 'deposit', amount: 1_000 });

    assert.deepStrictEqual(alertLines(), []);
    assert.deepStrictEqual(lines, []);
  });

  it('does not treat a non-withdrawal event with a large amount as either withdrawal rule', async () => {
    await processEventForAlerts({ type: 'deposit', amount: 999_999 });

    assert.deepStrictEqual(alertLines(), []);
  });

  it('ignores a withdrawal with no amount instead of raising on undefined', async () => {
    await processEventForAlerts({ type: 'withdraw' });

    assert.deepStrictEqual(alertLines(), []);
  });
});
