/**
 * Durability and multi-replica behaviour of OTP / session state (#943).
 *
 * Replicas and restarts are simulated by loading fresh copies of the service
 * modules (own module-level state, as in a separate process) on top of ONE
 * shared store - which is what Postgres is in production.
 */
process.env.PHONE_HASH_SALT = 'test-phone-salt-0123456789';

import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { InMemoryStateStore, PostgresStateStore, setStateStore } from '../stateStore';

type OtpModule = typeof import('../otpService');
type StateModule = typeof import('../stateManager');

/** Loads private copies of the service modules, as a new process would. */
function bootReplica(store: InMemoryStateStore): { otp: OtpModule; state: StateModule } {
  for (const name of ['../otpService', '../stateManager', '../stateStore']) {
    delete require.cache[require.resolve(name)];
  }
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  const storeModule = require('../stateStore') as typeof import('../stateStore');
  storeModule.setStateStore(store);
  return {
    otp: require('../otpService') as OtpModule, // eslint-disable-line @typescript-eslint/no-var-requires
    state: require('../stateManager') as StateModule, // eslint-disable-line @typescript-eslint/no-var-requires
  };
}

let counter = 0;
const phone = (): string => `durable-${++counter}`;

describe('state survives a restart', () => {
  test('a pending OTP is still verifiable after the process restarts', async () => {
    const store = new InMemoryStateStore();
    const hash = phone();

    const before = bootReplica(store);
    const code = await before.otp.generateOTP(hash);

    const after = bootReplica(store); // fresh process, empty module state
    assert.equal((await after.otp.verifyOTP(hash, code)).success, true);
  });

  test('conversation state and rate-limit counters survive a restart', async () => {
    const store = new InMemoryStateStore();
    const hash = phone();

    const before = bootReplica(store);
    await before.state.updateState(hash, before.state.UserState.VERIFIED);
    for (let i = 0; i < 4; i += 1) await before.state.checkRateLimit(hash);

    const after = bootReplica(store);
    assert.equal((await after.state.getSession(hash)).state, after.state.UserState.VERIFIED);
    assert.equal((await after.state.getSession(hash)).messageCount, 4);
  });

  test('persists only a hash of the OTP, never the code', async () => {
    const store = new InMemoryStateStore();
    const hash = phone();
    const { otp } = bootReplica(store);

    const code = await otp.generateOTP(hash);
    const stored = JSON.stringify([...store.otps.values()]);
    assert.ok(!stored.includes(code), 'plaintext OTP must not be stored');
  });
});

describe('multi-replica safety', () => {
  test('replicas share one OTP: issued on one, verified on another', async () => {
    const store = new InMemoryStateStore();
    const hash = phone();
    const a = bootReplica(store);
    const code = await a.otp.generateOTP(hash);
    const b = bootReplica(store);

    assert.equal((await b.otp.verifyOTP(hash, code)).success, true);
    assert.equal((await a.otp.verifyOTP(hash, code)).success, false, 'code is single-use across replicas');
  });

  test('parallel guesses across replicas cannot exceed the attempt limit', async () => {
    const store = new InMemoryStateStore();
    const hash = phone();
    const a = bootReplica(store);
    const code = await a.otp.generateOTP(hash);
    const b = bootReplica(store);

    // 10 simultaneous wrong guesses split between two replicas.
    const wrong = code === '000000' ? '111111' : '000000';
    const results = await Promise.all(
      Array.from({ length: 10 }, (_, i) => (i % 2 === 0 ? a : b).otp.verifyOTP(hash, wrong))
    );
    assert.equal(results.filter((r) => /^Invalid OTP/.test(r.message)).length, 3, 'only MAX_ATTEMPTS guesses are evaluated');
    assert.equal(results.filter((r) => /too many/i.test(r.message)).length > 0, true);

    // The legitimate code is rejected once the challenge is exhausted.
    assert.equal((await a.otp.verifyOTP(hash, code)).success, false);
  });

  test('a correct code submitted concurrently on two replicas is redeemed exactly once', async () => {
    const store = new InMemoryStateStore();
    const hash = phone();
    const a = bootReplica(store);
    const code = await a.otp.generateOTP(hash);
    const b = bootReplica(store);

    const results = await Promise.all([a.otp.verifyOTP(hash, code), b.otp.verifyOTP(hash, code)]);
    assert.equal(results.filter((r) => r.success).length, 1);
  });

  test('rate limiting is enforced across replicas, not per process', async () => {
    const store = new InMemoryStateStore();
    const hash = phone();
    const a = bootReplica(store);
    const b = bootReplica(store);

    const allowed = await Promise.all(
      Array.from({ length: 14 }, (_, i) => (i % 2 === 0 ? a : b).state.checkRateLimit(hash))
    );
    assert.equal(allowed.filter(Boolean).length, 10);
  });
});

describe('PostgresStateStore', () => {
  /** Runs the store against a scripted fake `pg` query function. */
  async function withDb<T>(
    responses: Array<{ rows: unknown[] }>,
    run: (store: PostgresStateStore, calls: Array<{ sql: string; params: unknown[] }>) => Promise<T>
  ): Promise<T> {
    const calls: Array<{ sql: string; params: unknown[] }> = [];
    const dbPath = require.resolve('../db');
    const original = require.cache[dbPath];
    require.cache[dbPath] = {
      id: dbPath,
      filename: dbPath,
      loaded: true,
      exports: {
        query: async (sql: string, params: unknown[] = []) => {
          calls.push({ sql, params });
          return responses.shift() ?? { rows: [] };
        },
      },
      parent: null,
      children: [],
      paths: [],
    } as unknown as NodeModule;
    delete require.cache[require.resolve('../stateStore')];
    try {
      const { PostgresStateStore: Fresh } = require('../stateStore') as typeof import('../stateStore');
      return await run(new Fresh(), calls);
    } finally {
      if (original) require.cache[dbPath] = original;
      else delete require.cache[dbPath];
      delete require.cache[require.resolve('../stateStore')];
    }
  }

  test('counts an OTP attempt in one parameterised UPDATE and returns the stored hash', async () => {
    await withDb([{ rows: [{ code_hash: 'abc', attempts: 2 }] }], async (store, calls) => {
      const result = await store.attemptOtp('h', 1000, 3);
      assert.deepEqual(result, { kind: 'attempt', codeHash: 'abc', attempts: 2 });
      assert.equal(calls.length, 1);
      assert.match(calls[0].sql, /UPDATE whatsapp_otps\s+SET attempts = attempts \+ 1/);
      assert.deepEqual(calls[0].params, ['h', 1000, 3]);
    });
  });

  test('classifies missing, expired and exhausted challenges and deletes dead ones', async () => {
    await withDb([{ rows: [] }, { rows: [] }], async (store) => {
      assert.deepEqual(await store.attemptOtp('h', 1000, 3), { kind: 'none' });
    });
    await withDb([{ rows: [] }, { rows: [{ expires_at_ms: '500', attempts: 1 }] }, { rows: [] }], async (store, calls) => {
      assert.deepEqual(await store.attemptOtp('h', 1000, 3), { kind: 'expired' });
      assert.match(calls[2].sql, /DELETE FROM whatsapp_otps/);
    });
    await withDb([{ rows: [] }, { rows: [{ expires_at_ms: '5000', attempts: 3 }] }, { rows: [] }], async (store) => {
      assert.deepEqual(await store.attemptOtp('h', 1000, 3), { kind: 'locked' });
    });
  });

  test('consumes an OTP with DELETE ... RETURNING so only one caller wins', async () => {
    await withDb([{ rows: [{ phone_hash: 'h' }] }, { rows: [] }], async (store, calls) => {
      assert.equal(await store.consumeOtp('h', 'abc'), true);
      assert.equal(await store.consumeOtp('h', 'abc'), false);
      assert.match(calls[0].sql, /DELETE FROM whatsapp_otps[\s\S]*RETURNING/);
    });
  });

  test('maps BIGINT columns (returned as strings by pg) to numbers', async () => {
    const row = { state: 'VERIFIED', last_message_ms: '1700000000000', message_count: 3, rate_limit_window_start_ms: '1699999999000' };
    await withDb([{ rows: [row] }], async (store) => {
      assert.deepEqual(await store.touchSession('h', 1700000000000, 900000), {
        state: 'VERIFIED',
        lastMessageTime: 1700000000000,
        messageCount: 3,
        rateLimitWindowStart: 1699999999000,
      });
    });
  });

  test('never selects or writes secrets: session/OTP statements touch only their own tables', async () => {
    await withDb([{ rows: [{ message_count: 2 }] }], async (store, calls) => {
      assert.equal(await store.bumpMessageCount('h', 1, 60000), 2);
      assert.match(calls[0].sql, /whatsapp_sessions/);
      assert.doesNotMatch(calls[0].sql, /whatsapp_wallets/);
    });
  });
});

// Leave the default store in a known state for any test sharing this process.
setStateStore(new InMemoryStateStore());
