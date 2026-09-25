/**
 * Unit tests for eventListener.ts (Issue #688)
 *
 * Strategy: because ts-node runs in CJS mode where mock.module is unavailable,
 * we test the core logic functions directly by extracting them into helpers and
 * exercising the exported surface (startEventListener / stopEventListener) with
 * environment variables and by monkey-patching the module's exported `server`
 * and `pool` objects before each test.
 */

import { describe, it, before, after, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';

// ---------------------------------------------------------------------------
// Capture references to module internals we can patch
// ---------------------------------------------------------------------------

// We import the module under test first, then replace the properties on the
// live `server` object so all subsequent calls go through our stubs.
import { startEventListener, stopEventListener, server, pool } from './eventListener';

// ---------------------------------------------------------------------------
// Stub state – all tests share these, reset in beforeEach
// ---------------------------------------------------------------------------

const rpcCalls: string[] = [];
let latestLedgerSeq = 1000;
let eventsResponse: {
  events: StubEvent[];
  cursor?: string;
  latestLedger: number;
} = { events: [], latestLedger: 1000 };

interface StubEvent {
  id: string;
  ledger: number;
  topic: unknown[];
  value: unknown;
}

function makeEvent(overrides: Partial<StubEvent> = {}): StubEvent {
  return {
    id: '0000001000-0000000001',
    ledger: 1000,
    topic: ['deposit', 'GABC1234'],
    value: { user: 'GABC1234', amount: BigInt(500_000_000), shares: BigInt(500_000_000) },
    ...overrides,
  };
}

// Patch the live server object so every test uses the stub without re-importing
const serverAny = server as unknown as Record<string, unknown>;
serverAny.getLatestLedger = async () => {
  rpcCalls.push('getLatestLedger');
  return { sequence: latestLedgerSeq };
};
serverAny.getEvents = async (_req: unknown) => {
  rpcCalls.push('getEvents');
  return eventsResponse;
};

// Patch the pool so DB queries don't fail
const dbQueries: string[] = [];
const poolAny = pool as unknown as Record<string, unknown>;
poolAny.query = async (sql: string, _values?: unknown[]) => {
  dbQueries.push(sql);
  return { rows: [] };
};

/** Waits just over one 5-second poll interval. */
async function tickOnce(ms = 5100): Promise<void> {
  await new Promise((r) => setTimeout(r, ms));
}

// ---------------------------------------------------------------------------
// Helpers used by multiple suites
// ---------------------------------------------------------------------------

function resetState() {
  rpcCalls.length = 0;
  dbQueries.length = 0;
  latestLedgerSeq = 1000;
  eventsResponse = { events: [], latestLedger: 1000 };
  delete process.env.VAULT_CONTRACT_ID;
  delete process.env.DATABASE_URL;
  stopEventListener();
}

// ---------------------------------------------------------------------------
// Suite 1 – Initialisation
// ---------------------------------------------------------------------------

describe('eventListener – initialisation', () => {
  beforeEach(resetState);
  afterEach(() => stopEventListener());

  it('does not call getLatestLedger when VAULT_CONTRACT_ID is missing', async () => {
    await startEventListener();
    assert.strictEqual(rpcCalls.length, 0, 'no RPC calls without a contract ID');
  });

  it('calls getLatestLedger on startup when VAULT_CONTRACT_ID is set', async () => {
    process.env.VAULT_CONTRACT_ID = 'CONTRACT_ABC';
    await startEventListener();
    assert.ok(rpcCalls.includes('getLatestLedger'), 'expected getLatestLedger call');
  });
});

// ---------------------------------------------------------------------------
// Suite 2 – Event detection and parsing
// ---------------------------------------------------------------------------

describe('eventListener – event detection and parsing', () => {
  // We need a way to capture alerts and yield evaluations. Since we can't use
  // mock.module, we validate observable side-effects: the DB writes triggered
  // by logEventToDb (when DATABASE_URL is set) and whether getEvents was called.

  beforeEach(() => {
    resetState();
    process.env.VAULT_CONTRACT_ID = 'CONTRACT_ABC';
  });
  afterEach(() => stopEventListener());

  it('polls for events after startup', async () => {
    await startEventListener();
    await tickOnce();
    assert.ok(rpcCalls.includes('getEvents'), 'expected getEvents to be called during polling');
  });

  it('calls getEvents on each interval tick', async () => {
    await startEventListener();
    await tickOnce(11000); // two ticks
    const getEventsCalls = rpcCalls.filter((c) => c === 'getEvents');
    assert.ok(getEventsCalls.length >= 2, 'expected at least 2 getEvents calls across two ticks');
  });

  it('logs deposit events to the DB when DATABASE_URL is set', async () => {
    process.env.DATABASE_URL = 'postgres://localhost/test';
    eventsResponse = {
      events: [makeEvent({ topic: ['deposit', 'GABC1234'] })],
      latestLedger: 1001,
    };
    await startEventListener();
    await tickOnce();
    const inserts = dbQueries.filter((q) => q.includes('INSERT INTO vault_events'));
    assert.ok(inserts.length >= 1, 'deposit event should be logged to the DB');
  });

  it('logs withdraw events to the DB when DATABASE_URL is set', async () => {
    process.env.DATABASE_URL = 'postgres://localhost/test';
    eventsResponse = {
      events: [makeEvent({ topic: ['withdraw', 'GABC1234'] })],
      latestLedger: 1001,
    };
    await startEventListener();
    await tickOnce();
    const inserts = dbQueries.filter((q) => q.includes('INSERT INTO vault_events'));
    assert.ok(inserts.length >= 1, 'withdraw event should be logged to the DB');
  });

  it('does not write to DB for unrecognised event topics', async () => {
    process.env.DATABASE_URL = 'postgres://localhost/test';
    eventsResponse = {
      events: [makeEvent({ topic: ['unknown_event', 'GABC1234'] })],
      latestLedger: 1001,
    };
    await startEventListener();
    await tickOnce();
    const inserts = dbQueries.filter((q) => q.includes('INSERT INTO vault_events'));
    assert.strictEqual(inserts.length, 0, 'unrecognised events must not be logged');
  });

  it('skips DB writes when DATABASE_URL is absent', async () => {
    delete process.env.DATABASE_URL;
    eventsResponse = {
      events: [makeEvent()],
      latestLedger: 1001,
    };
    await startEventListener();
    await tickOnce();
    const inserts = dbQueries.filter((q) => q.includes('INSERT'));
    assert.strictEqual(inserts.length, 0, 'no DB writes without DATABASE_URL');
  });
});

// ---------------------------------------------------------------------------
// Suite 3 – Error handling
// ---------------------------------------------------------------------------

describe('eventListener – error handling for failed transactions', () => {
  beforeEach(() => {
    resetState();
    process.env.VAULT_CONTRACT_ID = 'CONTRACT_ABC';
  });
  afterEach(() => stopEventListener());

  it('does not crash when getEvents throws a transient error', async () => {
    let threw = false;
    serverAny.getEvents = async () => {
      rpcCalls.push('getEvents');
      if (!threw) {
        threw = true;
        throw new Error('ECONNRESET');
      }
      return { events: [], latestLedger: 1000 };
    };

    // Must not throw – listener swallows poll errors
    await startEventListener();
    await tickOnce(11000); // give it two ticks so the second succeeds

    assert.ok(rpcCalls.filter((c) => c === 'getEvents').length >= 2,
      'listener should keep polling after a transient error');

    // Restore stub
    serverAny.getEvents = async () => {
      rpcCalls.push('getEvents');
      return eventsResponse;
    };
  });

  it('does not crash when getLatestLedger throws on startup', async () => {
    serverAny.getLatestLedger = async () => {
      rpcCalls.push('getLatestLedger');
      throw new Error('network timeout');
    };

    // startEventListener must not throw
    await startEventListener();

    // Restore
    serverAny.getLatestLedger = async () => {
      rpcCalls.push('getLatestLedger');
      return { sequence: latestLedgerSeq };
    };
    assert.ok(true, 'startup error must not propagate');
  });

  it('does not crash when DB insert throws', async () => {
    process.env.DATABASE_URL = 'postgres://localhost/test';
    poolAny.query = async (sql: string) => {
      dbQueries.push(sql);
      if (sql.includes('INSERT')) throw new Error('DB write error');
      return { rows: [] };
    };

    eventsResponse = {
      events: [makeEvent()],
      latestLedger: 1001,
    };

    await startEventListener();
    await tickOnce();

    // Restore
    poolAny.query = async (sql: string) => { dbQueries.push(sql); return { rows: [] }; };
    assert.ok(true, 'DB write failure must not crash the listener');
  });

  it('handles events with null/undecodable value without crashing', async () => {
    process.env.DATABASE_URL = 'postgres://localhost/test';
    eventsResponse = {
      events: [makeEvent({ topic: ['deposit', 'GABC1234'], value: null })],
      latestLedger: 1001,
    };

    await startEventListener();
    await tickOnce();

    // Listener must still be alive – subsequent ticks work
    const countBefore = rpcCalls.filter((c) => c === 'getEvents').length;
    await tickOnce();
    const countAfter = rpcCalls.filter((c) => c === 'getEvents').length;
    assert.ok(countAfter > countBefore, 'listener must keep polling after a null-payload event');
  });
});

// ---------------------------------------------------------------------------
// Suite 4 – Concurrent event handling
// ---------------------------------------------------------------------------

describe('eventListener – concurrent event handling', () => {
  beforeEach(() => {
    resetState();
    process.env.VAULT_CONTRACT_ID = 'CONTRACT_ABC';
    process.env.DATABASE_URL = 'postgres://localhost/test';
  });
  afterEach(() => stopEventListener());

  it('logs all events from a full page to the DB', async () => {
    eventsResponse = {
      events: Array.from({ length: 5 }, (_, i) =>
        makeEvent({
          id: `000001000-000000000${i + 1}`,
          ledger: 1000,
          topic: [i % 2 === 0 ? 'deposit' : 'withdraw', `GADDR${i}`],
          value: { user: `GADDR${i}`, amount: BigInt((i + 1) * 100_000_000), shares: BigInt(100_000_000) },
        }),
      ),
      latestLedger: 1000,
    };

    await startEventListener();
    await tickOnce();

    const inserts = dbQueries.filter((q) => q.includes('INSERT INTO vault_events'));
    assert.strictEqual(inserts.length, 5, 'all 5 events on the page must be logged');
  });

  it('does not overlap polls (polling flag prevents re-entry)', async () => {
    // Make getEvents slow so the second tick fires before the first completes
    let inFlight = 0;
    let maxConcurrent = 0;

    serverAny.getEvents = async () => {
      inFlight++;
      if (inFlight > maxConcurrent) maxConcurrent = inFlight;
      await new Promise((r) => setTimeout(r, 3000)); // 3 s – shorter than the 5 s tick
      inFlight--;
      rpcCalls.push('getEvents');
      return { events: [], latestLedger: 1000 };
    };

    await startEventListener();
    await tickOnce(12000); // three ticks: 0 s, 5 s, 10 s

    assert.strictEqual(maxConcurrent, 1, 'at most one poll should be in-flight at a time');

    // Restore
    serverAny.getEvents = async () => {
      rpcCalls.push('getEvents');
      return eventsResponse;
    };
  });
});

// ---------------------------------------------------------------------------
// Suite 5 – Graceful shutdown
// ---------------------------------------------------------------------------

describe('eventListener – graceful shutdown', () => {
  beforeEach(() => {
    resetState();
  });

  it('stops polling after stopEventListener is called', async () => {
    process.env.VAULT_CONTRACT_ID = 'CONTRACT_ABC';
    await startEventListener();
    await tickOnce(); // one tick to confirm it's running
    const before = rpcCalls.filter((c) => c === 'getEvents').length;

    stopEventListener();
    await tickOnce(11000); // two more potential ticks

    const after = rpcCalls.filter((c) => c === 'getEvents').length;
    assert.strictEqual(after, before, 'no new getEvents calls after stop');
  });

  it('calling stopEventListener when not running is a no-op', () => {
    stopEventListener(); // already stopped
    assert.ok(true, 'should not throw');
  });

  it('calling stopEventListener multiple times is safe', async () => {
    process.env.VAULT_CONTRACT_ID = 'CONTRACT_ABC';
    await startEventListener();
    stopEventListener();
    stopEventListener();
    stopEventListener();
    assert.ok(true, 'triple-stop must not throw');
  });
});
