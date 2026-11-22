import { describe, it } from 'node:test';
import assert from 'node:assert';
import { getUserStrategy, normalizeStrategy, runRebalanceCycle } from './userStrategies';

function fakePool(tables: { users: Array<Record<string, unknown>>; rebalances: Array<Record<string, unknown>> }) {
  return {
    async query(sql: string, values?: unknown[]) {
      if (sql.includes('FROM rebalances')) return { rows: tables.rebalances.slice(0, 1) };
      if (sql.includes('WHERE stellar_address')) {
        return { rows: tables.users.filter((u) => u.stellar_address === values?.[0]) };
      }
      if (sql.includes('FROM users')) return { rows: tables.users };
      throw new Error(`unexpected query: ${sql}`);
    },
  };
}

const users = [
  { id: 'u1', stellar_address: 'GA1', strategy_preference: 'conservative' },
  { id: 'u2', stellar_address: 'GA2', strategy_preference: 'growth' },
  { id: 'u3', stellar_address: 'GA3', strategy_preference: 'growth' },
];

describe('Per-user strategy from Postgres (#749)', () => {
  it('evaluates each stored strategy against the current allocation', async () => {
    const calls: Array<[string, string, number]> = [];
    const pool = fakePool({ users, rebalances: [{ protocol: 'dex', apy_after: '7.25' }] });

    const result = await runRebalanceCycle(pool, async (strategy, protocol, apy) => {
      calls.push([strategy, protocol, apy]);
      return { shouldRebalance: strategy === 'growth', targetProtocol: 'blend' };
    });

    assert.strictEqual(result.usersEvaluated, 3);
    assert.deepStrictEqual(calls, [
      ['conservative', 'dex', 7.25],
      ['growth', 'dex', 7.25],
    ]);
    assert.strictEqual(result.decisions.get('growth')?.shouldRebalance, true);
    assert.strictEqual(result.decisions.get('conservative')?.shouldRebalance, false);
  });

  it('defaults to no position when no rebalance has happened yet', async () => {
    const calls: Array<[string, string, number]> = [];
    await runRebalanceCycle(fakePool({ users: users.slice(0, 1), rebalances: [] }), async (s, p, a) => {
      calls.push([s, p, a]);
      return { shouldRebalance: false };
    });
    assert.deepStrictEqual(calls, [['conservative', 'none', 0]]);
  });

  it('looks up a single depositor strategy by address', async () => {
    const pool = fakePool({ users, rebalances: [] });
    assert.strictEqual(await getUserStrategy(pool, 'GA2'), 'growth');
    assert.strictEqual(await getUserStrategy(pool, 'GUNKNOWN'), 'balanced');
  });

  it('falls back to balanced for unexpected values', () => {
    assert.strictEqual(normalizeStrategy('yolo'), 'balanced');
    assert.strictEqual(normalizeStrategy('conservative'), 'conservative');
  });
});
