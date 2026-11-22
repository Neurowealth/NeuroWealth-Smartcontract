import { describe, it } from 'node:test';
import assert from 'node:assert';
import { initialPosition, isStalePositionError, LedgerCursor, LedgerCursorValue, LEDGER_CURSOR_KEY } from './ledgerCursor';
import { StateStore } from './stateBackup';

class MemoryStore implements StateStore<LedgerCursorValue> {
  data = new Map<string, LedgerCursorValue>();
  saves = 0;
  async save(key: string, value: LedgerCursorValue) { this.saves++; this.data.set(key, value); }
  async load(key: string) { return this.data.get(key) ?? null; }
}

describe('Ledger cursor persistence (#747)', () => {
  it('starts from the latest ledger when nothing is stored', async () => {
    const cursor = new LedgerCursor(new MemoryStore());
    assert.deepStrictEqual(initialPosition(await cursor.load(), 5000), { startLedger: 5000 });
  });

  it('resumes from the saved paging token after a restart', async () => {
    const store = new MemoryStore();
    await new LedgerCursor(store, () => 1).save('0000004200-0000000003', 4200);

    // "restart": a fresh cursor over the same store
    const stored = await new LedgerCursor(store).load();
    assert.strictEqual(stored?.pagingToken, '0000004200-0000000003');
    assert.deepStrictEqual(initialPosition(stored, 9999), { cursor: '0000004200-0000000003' });
  });

  it('skips redundant writes for an unchanged token', async () => {
    const store = new MemoryStore();
    const cursor = new LedgerCursor(store);
    await cursor.save('a', 1);
    await cursor.save('a', 1);
    await cursor.save('b', 2);
    assert.strictEqual(store.saves, 2);
    assert.strictEqual(store.data.get(LEDGER_CURSOR_KEY)?.pagingToken, 'b');
  });

  it('does not throw when persisting fails', async () => {
    const failing: StateStore<LedgerCursorValue> = {
      save: async () => { throw new Error('db down'); },
      load: async () => null,
    };
    await new LedgerCursor(failing).save('x', 1);
  });

  it('propagates load failures so the listener does not silently skip ahead', async () => {
    const failing: StateStore<LedgerCursorValue> = {
      save: async () => {},
      load: async () => { throw new Error('db down'); },
    };
    await assert.rejects(new LedgerCursor(failing).load(), /db down/);
  });

  it('recognises stale-cursor RPC errors', () => {
    assert.ok(isStalePositionError(new Error('startLedger must be within the ledger range: 100 - 200')));
    assert.ok(isStalePositionError(new Error('invalid cursor')));
    assert.ok(!isStalePositionError(new Error('ECONNRESET')));
  });
});
