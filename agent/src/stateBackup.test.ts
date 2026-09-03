import { describe, it, beforeEach, after } from 'node:test';
import assert from 'node:assert';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  AGENT_STATE_VERSION,
  AgentState,
  FileStateStore,
  MultiStateStore,
  PostgresStateStore,
  StateBackupManager,
  parseAgentState,
} from './stateBackup';

function validState(): Omit<AgentState, 'version' | 'savedAt'> {
  return {
    lastLedger: 42,
    lastRebalanceAt: 1_000,
    scheduler: {
      interval: '4h',
      lastRunAt: 900,
      consecutiveFailures: 0,
    },
    metrics: {
      tvl: 1_000_000,
      apy: 0.07,
      allocations: [{ protocol: 'blend', amount: 1_000_000 }],
    },
  };
}

describe('Agent state backup & restoration (#655)', () => {
  let tmpDir: string;
  let tmpFile: string;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'nw-state-'));
    tmpFile = path.join(tmpDir, 'state.json');
  });

  after(async () => {
    // tmp dirs are left in place; OS cleans /tmp
  });

  it('parseAgentState accepts a valid state and stamps nothing', () => {
    const parsed = parseAgentState({ ...validState(), version: AGENT_STATE_VERSION, savedAt: 5 });
    assert.ok(parsed);
    assert.strictEqual(parsed.lastLedger, 42);
    assert.strictEqual(parsed.version, AGENT_STATE_VERSION);
    assert.strictEqual(parsed.savedAt, 5);
  });

  it('parseAgentState rejects wrong schema versions and garbage', () => {
    assert.strictEqual(parseAgentState({ ...validState(), version: 99, savedAt: 1 }), null);
    assert.strictEqual(parseAgentState(null), null);
    assert.strictEqual(parseAgentState('nope'), null);
    assert.strictEqual(parseAgentState({ version: AGENT_STATE_VERSION }), null);
  });

  it('FileStateStore round-trips and returns null for missing keys/files', async () => {
    const store = new FileStateStore<Omit<AgentState, 'version' | 'savedAt'>>(tmpFile);
    await store.save('agent:state', validState());
    assert.deepStrictEqual(await store.load('agent:state'), validState());
    assert.strictEqual(await store.load('other:key'), null);
    assert.strictEqual(await new FileStateStore(path.join(tmpDir, 'missing.json')).load('agent:state'), null);
  });

  it('PostgresStateStore serializes JSON and handles empty results', async () => {
    const queries: Array<{ sql: string; values?: unknown[] }> = [];
    const pool = {
      query: async (sql: string, values?: unknown[]) => {
        queries.push({ sql, values });
        if (sql.startsWith('SELECT')) return { rows: [] };
        return { rows: [] };
      },
    };
    const store = new PostgresStateStore<Record<string, unknown>>(pool);
    await store.save('k', { a: 1 });
    assert.strictEqual(queries[0].values?.[0], 'k');
    assert.strictEqual(queries[0].values?.[1], '{"a":1}');
    assert.strictEqual(await store.load('k'), null);
  });

  it('MultiStateStore fans out saves and fails over on load', async () => {
    const good = new Map<string, unknown>();
    const failing = {
      save: async () => {
        throw new Error('boom');
      },
      load: async () => {
        throw new Error('boom');
      },
    };
    const primary = {
      save: async (key: string, value: unknown) => {
        good.set(key, value);
      },
      load: async (key: string) => (good.get(key) ?? null) as never,
    };

    // Failing store first: save logs but succeeds; load falls through.
    const multi = new MultiStateStore<Record<string, unknown>>([
      { name: 'failing', store: failing },
      { name: 'primary', store: primary },
    ]);
    await multi.save('k', { v: 7 });
    assert.deepStrictEqual(await multi.load('k'), { v: 7 });
    assert.deepStrictEqual(good.get('k'), { v: 7 });

    // All stores failing -> save throws.
    const allBad = new MultiStateStore<Record<string, unknown>>([
      { name: 'a', store: failing },
      { name: 'b', store: failing },
    ]);
    await assert.rejects(() => allBad.save('k', { v: 1 }), /all state stores failed/);
  });

  it('StateBackupManager stamps version/savedAt and restores valid backups', async () => {
    const store = new FileStateStore(tmpFile);
    const manager = new StateBackupManager(store, 'agent:state', () => 1_234);
    const backed = await manager.backup(validState());
    assert.strictEqual(backed.version, AGENT_STATE_VERSION);
    assert.strictEqual(backed.savedAt, 1_234);
    assert.strictEqual(manager.getLastState()?.lastLedger, 42);

    const restored = await manager.restore();
    assert.ok(restored);
    assert.strictEqual(restored.lastLedger, 42);
    assert.strictEqual(restored.metrics.tvl, 1_000_000);
  });

  it('StateBackupManager.restore rejects incompatible backups instead of corrupting state', async () => {
    const store = new FileStateStore(tmpFile);
    await store.save('agent:state', { ...validState(), version: 999, savedAt: 1 });
    const manager = new StateBackupManager(store);
    assert.strictEqual(await manager.restore(), null);
    assert.strictEqual(manager.getLastState(), null);
  });
});
