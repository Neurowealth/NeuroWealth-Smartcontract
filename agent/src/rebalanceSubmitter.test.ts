import { describe, it } from 'node:test';
import assert from 'node:assert';
import { Account, Keypair, StrKey } from '@stellar/stellar-sdk';
import {
  TESTNET_PASSPHRASE,
  rebalanceConfigFromEnv,
  submitRebalance,
} from './rebalanceSubmitter';

// Configuration and submission for the rebalance path (#687). The values that
// decide how much an on-chain trade may lose must come from the operator, so the
// validation is tested as strictly as the submission plumbing.

const SECRET = Keypair.random().secret();

function validEnv(overrides: Record<string, string | undefined> = {}) {
  return {
    AGENT_SECRET_KEY: SECRET,
    REBALANCE_EXPECTED_APY_BPS: '650',
    REBALANCE_MIN_OUT: '1000000000',
    ...overrides,
  } as NodeJS.ProcessEnv;
}

describe('rebalanceConfigFromEnv (#687)', () => {
  it('accepts a complete configuration and defaults to testnet', () => {
    const result = rebalanceConfigFromEnv(validEnv());
    assert.strictEqual(result.ok, true);
    if (!result.ok) return;
    assert.strictEqual(result.config.agentSecretKey, SECRET);
    assert.strictEqual(result.config.expectedApyBps, 650);
    assert.strictEqual(result.config.minOut, '1000000000');
    assert.strictEqual(result.config.networkPassphrase, TESTNET_PASSPHRASE);
  });

  it('honours an explicit network passphrase', () => {
    const result = rebalanceConfigFromEnv(
      validEnv({ STELLAR_NETWORK_PASSPHRASE: 'Public Global Stellar Network ; September 2015' }),
    );
    assert.strictEqual(result.ok, true);
    if (!result.ok) return;
    assert.match(result.config.networkPassphrase, /Public Global/);
  });

  it('refuses to run without an agent key', () => {
    const result = rebalanceConfigFromEnv(validEnv({ AGENT_SECRET_KEY: undefined }));
    assert.strictEqual(result.ok, false);
    if (result.ok) return;
    assert.match(result.reason, /AGENT_SECRET_KEY is not set/);
  });

  it('rejects a key that is not a Stellar secret', () => {
    const result = rebalanceConfigFromEnv(validEnv({ AGENT_SECRET_KEY: 'not-a-key' }));
    assert.strictEqual(result.ok, false);
    if (result.ok) return;
    assert.match(result.reason, /not a Stellar secret key/);
  });

  it('refuses to guess the expected APY', () => {
    for (const value of [undefined, '', 'abc', '12.5']) {
      const result = rebalanceConfigFromEnv(validEnv({ REBALANCE_EXPECTED_APY_BPS: value }));
      assert.strictEqual(result.ok, false, 'expected ' + String(value) + ' to be rejected');
    }
  });

  it('rejects an expected APY outside the basis-point range', () => {
    for (const value of ['-1', '10001']) {
      const result = rebalanceConfigFromEnv(validEnv({ REBALANCE_EXPECTED_APY_BPS: value }));
      assert.strictEqual(result.ok, false);
      if (result.ok) return;
      assert.match(result.reason, /between 0 and 10000/);
    }
  });

  it('refuses to guess the slippage floor', () => {
    for (const value of [undefined, '', '-5', '1.5']) {
      const result = rebalanceConfigFromEnv(validEnv({ REBALANCE_MIN_OUT: value }));
      assert.strictEqual(result.ok, false, 'expected min_out ' + String(value) + ' to be rejected');
    }
  });
});

interface StubOptions {
  sent?: { status: string; hash: string };
  statuses?: string[];
  accountError?: Error;
}

function stubServer(options: StubOptions = {}) {
  const calls = {
    prepared: 0,
    sent: 0,
    polled: [] as string[],
    slept: 0,
    signedWith: [] as string[],
  };
  const statuses = [...(options.statuses ?? ['SUCCESS'])];
  const server = {
    // A real Account: TransactionBuilder needs accountId() and sequenceNumber(),
    // and stubbing those by hand is what it would rather not see.
    getAccount: async (publicKey: string) => {
      if (options.accountError) throw options.accountError;
      return new Account(publicKey, '0');
    },
    prepareTransaction: async (tx: any) => {
      calls.prepared += 1;
      return tx;
    },
    sendTransaction: async () => {
      calls.sent += 1;
      return options.sent ?? { status: 'PENDING', hash: 'hash-1' };
    },
    getTransaction: async (hash: string) => {
      calls.polled.push(hash);
      return { status: statuses.shift() ?? 'NOT_FOUND' };
    },
  };
  return { server: server as any, calls };
}

function target() {
  const config = rebalanceConfigFromEnv(validEnv());
  assert.strictEqual(config.ok, true);
  if (!config.ok) throw new Error('unreachable');
    // A generated strkey rather than a copied constant: the Contract constructor
  // validates its input, so an invented id fails the build step instead of the
  // assertion that is under test.
  return {
    contractId: StrKey.encodeContract(Buffer.alloc(32)),
    protocol: 'blend',
    config: config.config,
  };
}

describe('submitRebalance (#687)', () => {
  it('builds, signs, sends and confirms the rebalance', async () => {
    const { server, calls } = stubServer({ statuses: ['SUCCESS'] });
    const outcome = await submitRebalance(target(), {
      server,
      sign: (_prepared, keypair) => calls.signedWith.push(keypair.publicKey()),
      sleep: async () => { calls.slept += 1; },
    });

    assert.deepStrictEqual(outcome, { submitted: true, hash: 'hash-1', status: 'SUCCESS' });
    assert.strictEqual(calls.prepared, 1);
    assert.strictEqual(calls.sent, 1);
    assert.deepStrictEqual(calls.polled, ['hash-1']);
    assert.deepStrictEqual(calls.signedWith, [Keypair.fromSecret(SECRET).publicKey()]);
    assert.strictEqual(calls.slept, 0, 'a first-poll success should not sleep');
  });

  it('polls past NOT_FOUND until the transaction confirms', async () => {
    const { server, calls } = stubServer({ statuses: ['NOT_FOUND', 'NOT_FOUND', 'SUCCESS'] });
    const outcome = await submitRebalance(target(), { server, sleep: async () => { calls.slept += 1; } });

    assert.strictEqual(outcome.submitted, true);
    assert.strictEqual(calls.slept, 2, 'it waits between polls');
  });

  it('reports an on-chain failure instead of claiming success', async () => {
    const { server } = stubServer({ statuses: ['FAILED'] });
    const outcome = await submitRebalance(target(), { server, sleep: async () => {} });

    assert.deepStrictEqual(outcome, {
      submitted: false,
      reason: 'the rebalance transaction failed on-chain',
    });
  });

  it('reports a submission the network rejected', async () => {
    const { server } = stubServer({ sent: { status: 'ERROR', hash: 'hash-err' } });
    const outcome = await submitRebalance(target(), { server, sleep: async () => {} });
    assert.strictEqual(outcome.submitted, false);
    if (outcome.submitted) return;
    assert.match(outcome.reason, /rejected/);
  });

  it('gives up after the poll budget rather than waiting forever', async () => {
    const { server, calls } = stubServer({ statuses: Array(5).fill('NOT_FOUND') });
    const outcome = await submitRebalance(target(), {
      server,
      maxPolls: 3,
      sleep: async () => { calls.slept += 1; },
    });

    assert.strictEqual(calls.polled.length, 3);
    assert.strictEqual(outcome.submitted, false);
    if (outcome.submitted) return;
    assert.match(outcome.reason, /did not confirm in time/);
  });

  it('returns a failure instead of throwing when the RPC call fails', async () => {
    const { server } = stubServer({ accountError: new Error('rpc unreachable') });
    const outcome = await submitRebalance(target(), { server, sleep: async () => {} });
    assert.deepStrictEqual(outcome, { submitted: false, reason: 'rpc unreachable' });
  });

  it('returns a failure for a key it cannot load', async () => {
    const { server } = stubServer();
    const broken = target();
    broken.config = { ...broken.config, agentSecretKey: 'SINVALID' };
    const outcome = await submitRebalance(broken, { server, sleep: async () => {} });

    assert.strictEqual(outcome.submitted, false);
    if (outcome.submitted) return;
    assert.match(outcome.reason, /could not be loaded/);
  });
});
