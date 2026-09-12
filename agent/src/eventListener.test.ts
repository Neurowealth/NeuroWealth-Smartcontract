import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';

/**
 * Unit tests for the event listener (#688).
 *
 * The module polls Soroban on a 5 second interval, so the tests capture the
 * interval callback and invoke it directly instead of waiting. Its dependencies
 * are stubbed where they are exported - the RPC client through the module's own
 * exported server, and the alert engine, yield evaluator, logger and pool
 * through their module objects - so no mocking framework is needed and the
 * listener is exercised exactly as written.
 */

const VAULT_ID = 'CVAULTTESTCONTRACTID00000000000000000000000000000000000';

process.env.VAULT_CONTRACT_ID = VAULT_ID;
process.env.SOROBAN_RPC_URL = 'https://rpc.example.test';

type PollCycle = () => Promise<void>;
let capturedInterval: PollCycle | null = null;
let intervalMs: number | null = null;
let clearedIds: unknown[] = [];

const realSetInterval = global.setInterval;
const realClearInterval = global.clearInterval;

let alertCalls: Array<{ type: string; amount: number }> = [];
let yieldCalls: Array<[string, string, number]> = [];
let logLines: string[] = [];
let dbQueries: Array<{ sql: string; params: unknown[] }> = [];
let eventsResponse: { events: any[] } | null = null;
let eventsError: Error | null = null;
let requestedStartLedgers: number[] = [];
let getLatestLedgerCalls = 0;

function event(ledger: number, topic: string, id: string) {
  return { id, ledger, topic: [{ toString: () => topic }] };
}

async function startAndPoll(): Promise<void> {
  const listener = await import('./eventListener');
  await listener.startEventListener();
  assert.ok(capturedInterval, 'the listener should have registered its interval');
  await capturedInterval!();
}

beforeEach(async () => {
  capturedInterval = null;
  intervalMs = null;
  clearedIds = [];
  alertCalls = [];
  yieldCalls = [];
  logLines = [];
  dbQueries = [];
  eventsResponse = { events: [] };
  eventsError = null;
  requestedStartLedgers = [];
  getLatestLedgerCalls = 0;

  (global as any).setInterval = (fn: PollCycle, ms: number) => {
    capturedInterval = fn;
    intervalMs = ms;
    return 12345;
  };
  (global as any).clearInterval = (id: unknown) => {
    clearedIds.push(id);
  };

  const alertEngine = require('./alertEngine');
  alertEngine.processEventForAlerts = async (payload: { type: string; amount: number }) => {
    alertCalls.push(payload);
  };

  const yieldComparison = require('./yieldComparison');
  yieldComparison.evaluateYield = async (strategy: string, protocol: string, amount: number) => {
    yieldCalls.push([strategy, protocol, amount]);
    return { shouldRebalance: false };
  };

  const loggerModule = require('./logger');
  const logger = loggerModule.default ?? loggerModule;
  logger.info = (...args: unknown[]) => {
    logLines.push(args.map((a) => (typeof a === 'string' ? a : JSON.stringify(a?.targetProtocol ?? a))).join(' '));
  };
  logger.warn = (...args: unknown[]) => {
    logLines.push('WARN ' + args.map(String).join(' '));
  };
  logger.error = (...args: unknown[]) => {
    logLines.push('ERROR ' + args.map(String).join(' '));
  };

  const dbModule = require('./db');
  dbModule.pool.query = async (sql: string, params: unknown[]) => {
    dbQueries.push({ sql, params });
    return { rows: [] };
  };

  const listener = await import('./eventListener');
  listener.server.getLatestLedger = async () => {
    getLatestLedgerCalls += 1;
    return { sequence: 1000 };
  };
  listener.server.getEvents = async (request: { startLedger: number }) => {
    requestedStartLedgers.push(request.startLedger);
    if (eventsError) throw eventsError;
    return eventsResponse;
  };
});

afterEach(async () => {
  (global as any).setInterval = realSetInterval;
  (global as any).clearInterval = realClearInterval;
  const listener = await import('./eventListener');
  listener.stopEventListener();
  delete process.env.DATABASE_URL;
});

describe('startEventListener wiring (#688)', () => {
  it('starts from the latest ledger and polls every five seconds', async () => {
    eventsResponse = { events: [] };
    await startAndPoll();

    assert.strictEqual(getLatestLedgerCalls, 1);
    assert.deepStrictEqual(requestedStartLedgers, [1000]);
    assert.strictEqual(intervalMs, 5000);
  });

  it('filters the poll to the configured contract', async () => {
    eventsResponse = { events: [] };
    await startAndPoll();

    const listener = await import('./eventListener');
    let captured: any = null;
    listener.server.getEvents = async (request: any) => {
      captured = request;
      return { events: [] };
    };
    await capturedInterval!();

    assert.deepStrictEqual(captured.filters, [
      { type: 'contract', contractIds: [VAULT_ID] },
    ]);
    assert.strictEqual(captured.limit, 100);
  });
});

describe('event detection (#688)', () => {
  it('alerts for a deposit event and evaluates yield', async () => {
    eventsResponse = { events: [event(1000, 'deposit', 'evt-1')] };
    await startAndPoll();

    assert.deepStrictEqual(alertCalls, [{ type: 'deposit', amount: 150000 }]);
    assert.deepStrictEqual(yieldCalls, [['balanced', 'none', 0]]);
  });

  it('alerts for a withdraw event without evaluating yield', async () => {
    eventsResponse = { events: [event(1001, 'withdraw', 'evt-2')] };
    await startAndPoll();

    assert.deepStrictEqual(alertCalls, [{ type: 'withdraw', amount: 150000 }]);
    assert.deepStrictEqual(yieldCalls, []);
  });

  it('ignores events whose topics are neither deposit nor withdraw', async () => {
    eventsResponse = { events: [event(1002, 'rebalance', 'evt-3'), event(1003, 'init', 'evt-4')] };
    await startAndPoll();

    assert.deepStrictEqual(alertCalls, []);
    assert.deepStrictEqual(dbQueries, [], 'an ignored event should not be persisted either');
  });

  it('handles several events in one poll, each exactly once', async () => {
    eventsResponse = {
      events: [
        event(1010, 'deposit', 'evt-10'),
        event(1011, 'withdraw', 'evt-11'),
        event(1012, 'deposit', 'evt-12'),
      ],
    };
    await startAndPoll();

    assert.deepStrictEqual(alertCalls, [
      { type: 'deposit', amount: 150000 },
      { type: 'withdraw', amount: 150000 },
      { type: 'deposit', amount: 150000 },
    ]);
  });

  it('advances the cursor past the highest ledger seen', async () => {
    eventsResponse = { events: [event(1005, 'deposit', 'evt-20')] };
    await startAndPoll();
    assert.deepStrictEqual(requestedStartLedgers, [1000]);

    eventsResponse = { events: [] };
    await capturedInterval!();
    assert.deepStrictEqual(requestedStartLedgers, [1000, 1006], 'next poll starts after the event ledger');
  });

  it('never moves the cursor backwards', async () => {
    eventsResponse = { events: [event(1005, 'deposit', 'evt-30')] };
    await startAndPoll();

    // A response with an older ledger must not rewind the cursor.
    eventsResponse = { events: [event(900, 'deposit', 'evt-31')] };
    await capturedInterval!();
    eventsResponse = { events: [] };
    await capturedInterval!();

    assert.deepStrictEqual(requestedStartLedgers, [1000, 1006, 1006]);
  });
});

describe('persistence and error handling (#688)', () => {
  it('writes detected events to the database when a connection is configured', async () => {
    process.env.DATABASE_URL = 'postgres://example.test/db';
    eventsResponse = { events: [event(1020, 'deposit', 'evt-40')] };
    await startAndPoll();

    assert.strictEqual(dbQueries.length, 1);
    assert.match(dbQueries[0].sql, /INSERT INTO vault_events/);
    assert.deepStrictEqual(dbQueries[0].params, ['evt-40', 'deposit', 1020]);
  });

  it('skips persistence entirely when no database is configured', async () => {
    delete process.env.DATABASE_URL;
    eventsResponse = { events: [event(1021, 'deposit', 'evt-41')] };
    await startAndPoll();

    assert.deepStrictEqual(dbQueries, []);
    assert.deepStrictEqual(alertCalls, [{ type: 'deposit', amount: 150000 }], 'the alert still happens');
  });

  it('survives a failed poll and keeps polling', async () => {
    eventsResponse = { events: [] };
    await startAndPoll();

    eventsError = new Error('rpc unavailable');
    await capturedInterval!();
    assert.ok(logLines.some((l) => l.startsWith('ERROR')), 'the failure is logged');

    eventsError = null;
    eventsResponse = { events: [event(1100, 'deposit', 'evt-50')] };
    await capturedInterval!();
    assert.deepStrictEqual(alertCalls, [{ type: 'deposit', amount: 150000 }], 'the next poll still runs');
  });

  it('keeps going when the database write fails', async () => {
    process.env.DATABASE_URL = 'postgres://example.test/db';
    const dbModule = require('./db');
    dbModule.pool.query = async () => {
      throw new Error('connection refused');
    };

    eventsResponse = { events: [event(1030, 'deposit', 'evt-60')] };
    await startAndPoll();

    assert.deepStrictEqual(alertCalls, [{ type: 'deposit', amount: 150000 }]);
    assert.ok(logLines.some((l) => l.startsWith('ERROR')), 'the db failure is logged');
  });
});

describe('shutdown (#688)', () => {
  it('clears the interval and logs that it stopped', async () => {
    eventsResponse = { events: [] };
    await startAndPoll();

    const listener = await import('./eventListener');
    listener.stopEventListener();

    assert.deepStrictEqual(clearedIds, [12345]);
    assert.ok(logLines.includes('Event listener stopped'));
  });

  it('does nothing when no listener is running', async () => {
    const listener = await import('./eventListener');
    listener.stopEventListener();
    assert.deepStrictEqual(clearedIds, []);
  });
});

describe('missing configuration (#688)', () => {
  it('warns and does not register an interval when no contract id is set', async () => {
    // The module reads the contract id once at import time, so this case needs
    // a fresh copy of the module rather than a new import of the same one.
    const modulePath = require.resolve('./eventListener');
    delete require.cache[modulePath];
    const previous = process.env.VAULT_CONTRACT_ID;
    delete process.env.VAULT_CONTRACT_ID;

    try {
      const fresh = require('./eventListener');
      await fresh.startEventListener();
      assert.strictEqual(capturedInterval, null, 'no interval without a contract id');
      assert.ok(logLines.some((l) => l.includes('VAULT_CONTRACT_ID is not set')));
    } finally {
      process.env.VAULT_CONTRACT_ID = previous;
      delete require.cache[modulePath];
      require('./eventListener');
    }
  });
});
