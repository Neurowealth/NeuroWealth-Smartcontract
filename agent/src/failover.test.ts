import { describe, it, beforeEach } from 'node:test';
import assert from 'node:assert';
import { FailoverManager, FailoverLease } from './failover';
import {
  StateBackupManager,
  StateStore,
  FileStateStore,
  MultiStateStore,
  AGENT_STATE_VERSION,
} from './stateBackup';
import { promises as fs } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

/** Deterministic in-memory store shared by all instances in a scenario. */
class MemoryStore<T> implements StateStore<T> {
  public entries = new Map<string, T>();
  public failSaves = false;

  async save(key: string, value: T): Promise<void> {
    if (this.failSaves) throw new Error('store unavailable');
    this.entries.set(key, JSON.parse(JSON.stringify(value)));
  }

  async load(key: string): Promise<T | null> {
    return (this.entries.get(key) as T) ?? null;
  }
}

interface Fixture {
  clock: { t: number };
  store: MemoryStore<FailoverLease>;
}

function makeFixture(): Fixture {
  return { clock: { t: 1_700_000_000_000 }, store: new MemoryStore<FailoverLease>() };
}

function makeManager(
  fixture: Fixture,
  agentId: string,
  hooks: { onPromote?: () => Promise<void> | void; onDemote?: () => Promise<void> | void } = {},
): FailoverManager {
  return new FailoverManager(
    { agentId, heartbeatIntervalMs: 15_000, heartbeatTtlMs: 60_000 },
    { store: fixture.store, now: () => fixture.clock.t, ...hooks },
  );
}

describe('Failover manager (Issue #655)', () => {
  let fixture: Fixture;

  beforeEach(() => {
    fixture = makeFixture();
  });

  it('refuses to write heartbeats while not primary', async () => {
    const standby = makeManager(fixture, 'agent-b');
    await assert.rejects(() => standby.beat(), /not primary/);
    assert.strictEqual(standby.isPrimary(), false);
  });

  it('standby stays on standby while the primary heartbeat is fresh', async () => {
    const primary = makeManager(fixture, 'agent-a');
    const standby = makeManager(fixture, 'agent-b');

    // Primary acquires the lease (empty store -> promoted) and beats.
    assert.strictEqual(await primary.checkAndFailover(), 'promoted');
    const action = await standby.checkAndFailover();
    assert.strictEqual(action, 'standby');
    assert.strictEqual(standby.isPrimary(), false);
  });

  it('promotes a standby when the lease record is missing', async () => {
    let promoted = false;
    const mgr = new FailoverManager(
      { agentId: 'agent-b' },
      {
        store: fixture.store,
        now: () => fixture.clock.t,
        onPromote: () => {
          promoted = true;
        },
      },
    );
    assert.strictEqual(await mgr.checkAndFailover(), 'promoted');
    assert.strictEqual(promoted, true);
    assert.strictEqual(mgr.isPrimary(), true);
  });

  it('promotes a standby once the primary heartbeat goes stale', async () => {
    const primary = makeManager(fixture, 'agent-a');
    const standby = makeManager(fixture, 'agent-b');

    await primary.checkAndFailover(); // primary takes lease at t
    fixture.clock.t += 30_000;
    assert.strictEqual(await standby.checkAndFailover(), 'standby'); // still fresh

    fixture.clock.t += 31_000; // 61s > 60s TTL
    assert.strictEqual(await standby.checkAndFailover(), 'promoted');
    assert.strictEqual(standby.isPrimary(), true);

    // The new primary's lease overwrote the dead one.
    const lease = fixture.store.entries.get('agent:lease')!;
    assert.strictEqual(lease.primaryId, 'agent-b');
  });

  it('re-affirms the lease when an already-primary instance checks in', async () => {
    const primary = makeManager(fixture, 'agent-a');
    await primary.checkAndFailover();
    const seqAfterAcquire = fixture.store.entries.get('agent:lease')!.seq;

    fixture.clock.t += 20_000;
    assert.strictEqual(await primary.checkAndFailover(), 'primary');
    const lease = fixture.store.entries.get('agent:lease')!;
    assert.strictEqual(lease.primaryId, 'agent-a');
    assert.strictEqual(lease.seq, seqAfterAcquire + 1);
    assert.strictEqual(primary.isPrimary(), true);
  });

  it('stepDown releases the lease and blocks further beats', async () => {
    let demoted = false;
    const primary = makeManager(fixture, 'agent-a', {
      onDemote: () => {
        demoted = true;
      },
    });
    await primary.checkAndFailover();
    assert.strictEqual(primary.isPrimary(), true);

    await primary.stepDown();
    assert.strictEqual(demoted, true);
    assert.strictEqual(primary.isPrimary(), false);
    await assert.rejects(() => primary.beat(), /not primary/);
  });

  it('failover restores the latest agent state backup on promotion', async () => {
    const backupFile = join(tmpdir(), `nw-dr-test-${process.pid}-${Date.now()}.json`);
    const fileStore = new FileStateStore(backupFile);
    const backups = new StateBackupManager(fileStore, 'agent:state', () => fixture.clock.t);
    await backups.backup({
      lastLedger: 4242,
      lastRebalanceAt: null,
      scheduler: { interval: '4h', lastRunAt: null, consecutiveFailures: 0 },
      metrics: { tvl: 1_000_000, apy: 0.07, allocations: [] },
    });

    let restoredLedger: number | null = null;
    const standby = new FailoverManager(
      { agentId: 'agent-b' },
      {
        store: fixture.store,
        now: () => fixture.clock.t,
        onPromote: async () => {
          const state = await backups.restore();
          restoredLedger = state ? state.lastLedger : null;
        },
      },
    );
    await standby.checkAndFailover(); // promoted (empty lease store)
    assert.strictEqual(restoredLedger, 4242);
    await fs.rm(backupFile, { force: true });
  });

  it('MultiStateStore fans out saves and falls back across stores on load', async () => {
    const broken = new MemoryStore<FailoverLease>();
    broken.failSaves = true;
    const healthy = new MemoryStore<FailoverLease>();
    const multi = new MultiStateStore<FailoverLease>([
      { name: 'broken', store: broken },
      { name: 'healthy', store: healthy },
    ]);

    const lease: FailoverLease = { primaryId: 'agent-a', at: 1, seq: 1 };
    await multi.save('agent:lease', lease); // must not throw despite broken store
    assert.strictEqual(broken.entries.size, 0);
    assert.strictEqual((await multi.load('agent:lease'))?.primaryId, 'agent-a');

    // Load falls back past a throwing store too.
    const throwing = new MemoryStore<FailoverLease>();
    throwing.failSaves = true;
    // Simulate load failure by pointing load at a missing prototype method.
    const failing = {
      save: async () => {
        throw new Error('down');
      },
      load: async () => {
        throw new Error('down');
      },
    } as StateStore<FailoverLease>;
    void failing;
    const multi2 = new MultiStateStore<FailoverLease>([
      { name: 'down', store: failing },
      { name: 'healthy2', store: healthy },
    ]);
    assert.strictEqual((await multi2.load('agent:lease'))?.seq, 1);
  });

  it('validated backups carry the current schema version', async () => {
    const store = new MemoryStore();
    const mgr = new StateBackupManager(
      store as unknown as StateStore,
      'agent:state',
      () => fixture.clock.t,
    );
    const saved = await mgr.backup({
      lastLedger: 7,
      lastRebalanceAt: null,
      scheduler: { interval: 'hourly', lastRunAt: null, consecutiveFailures: 0 },
      metrics: { tvl: 0, apy: 0, allocations: [] },
    });
    assert.strictEqual(saved.version, AGENT_STATE_VERSION);
    assert.strictEqual((await mgr.restore())?.lastLedger, 7);
  });
});

