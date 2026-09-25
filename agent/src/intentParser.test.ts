/**
 * Unit tests for intentParser.ts (Issue #689)
 *
 * Strategy: intercept the CJS module cache to inject a stub for
 * openAiKeyManager before intentParser.ts loads it, avoiding any real
 * OpenAI SDK import (which uses ESM-only internals incompatible with
 * ts-node's CJS mode).
 */

import { describe, it } from 'node:test';
import assert from 'node:assert';
import path from 'node:path';

// ---------------------------------------------------------------------------
// Stub the openAiKeyManager module BEFORE intentParser is required.
// We inject a synthetic entry into the CJS require cache so that when
// intentParser does `require('./openAiKeyManager')` it gets our stub.
// ---------------------------------------------------------------------------

let stubbedExecute: (op: (client: unknown) => Promise<unknown>) => Promise<string>;

function makeStubModule() {
  return {
    openAiKeyManager: {
      executeWithRotation: (_op: (client: unknown) => Promise<unknown>) =>
        stubbedExecute(_op),
    },
    OpenAIKeyManager: class {},
  };
}

// Resolve the on-disk path intentParser would require
const managerPath = path.resolve(__dirname, 'openAiKeyManager');

// Inject the stub under every extension ts-node might use as a cache key
for (const ext of ['.ts', '.js', '']) {
  const key = managerPath + ext;
  (require as any).cache[key] = {
    id: key,
    filename: key,
    loaded: true,
    exports: makeStubModule(),
    parent: null,
    children: [],
    paths: [],
  };
}

// Now safe to require intentParser – it will pick up our stub above
// eslint-disable-next-line @typescript-eslint/no-var-requires
const { parseIntent } = require('./intentParser') as {
  parseIntent: (msg: string) => Promise<ParsedIntent>;
};

// ---------------------------------------------------------------------------
// Types & helpers
// ---------------------------------------------------------------------------

interface ParsedIntent {
  action: string;
  amount?: number | 'all' | null;
  strategy?: string;
}

function stubSuccess(json: object): void {
  stubbedExecute = async (_op) => JSON.stringify(json);
}

function stubFailure(error: Error): void {
  stubbedExecute = async (_op) => { throw error; };
}

// ---------------------------------------------------------------------------
// Suite 1 – Common intents
// ---------------------------------------------------------------------------

describe('intentParser – common intents', () => {
  it('parses a deposit intent with a numeric amount', async () => {
    stubSuccess({ action: 'deposit', amount: 500 });
    const result = await parseIntent('deposit 500 USDC');
    assert.strictEqual(result.action, 'deposit');
    assert.strictEqual(result.amount, 500);
  });

  it('parses a withdraw intent with a numeric amount', async () => {
    stubSuccess({ action: 'withdraw', amount: 250 });
    const result = await parseIntent('withdraw 250 USDC');
    assert.strictEqual(result.action, 'withdraw');
    assert.strictEqual(result.amount, 250);
  });

  it('parses a withdraw-all intent with amount "all"', async () => {
    stubSuccess({ action: 'withdraw', amount: 'all' });
    const result = await parseIntent('withdraw everything');
    assert.strictEqual(result.action, 'withdraw');
    assert.strictEqual(result.amount, 'all');
  });

  it('parses a balance check intent', async () => {
    stubSuccess({ action: 'balance' });
    const result = await parseIntent('what is my balance?');
    assert.strictEqual(result.action, 'balance');
    assert.strictEqual(result.amount, undefined);
  });

  it('parses an earnings check intent', async () => {
    stubSuccess({ action: 'earnings' });
    const result = await parseIntent('how much have I earned?');
    assert.strictEqual(result.action, 'earnings');
  });

  it('parses a get_apy intent', async () => {
    stubSuccess({ action: 'get_apy' });
    const result = await parseIntent('what is the current APY?');
    assert.strictEqual(result.action, 'get_apy');
  });

  it('parses a switch_strategy intent with a strategy field', async () => {
    stubSuccess({ action: 'switch_strategy', strategy: 'conservative' });
    const result = await parseIntent('switch me to conservative mode');
    assert.strictEqual(result.action, 'switch_strategy');
    assert.strictEqual(result.strategy, 'conservative');
  });

  it('parses a deposit intent that includes a strategy', async () => {
    stubSuccess({ action: 'deposit', amount: 1000, strategy: 'growth' });
    const result = await parseIntent('deposit 1000 with growth strategy');
    assert.strictEqual(result.action, 'deposit');
    assert.strictEqual(result.amount, 1000);
    assert.strictEqual(result.strategy, 'growth');
  });

  it('parses large amounts correctly', async () => {
    stubSuccess({ action: 'deposit', amount: 1_000_000 });
    const result = await parseIntent('deposit one million USDC');
    assert.strictEqual(result.amount, 1_000_000);
  });

  it('parses fractional amounts correctly', async () => {
    stubSuccess({ action: 'withdraw', amount: 0.5 });
    const result = await parseIntent('withdraw half a USDC');
    assert.strictEqual(result.amount, 0.5);
  });
});

// ---------------------------------------------------------------------------
// Suite 2 – Ambiguous and malformed input
// ---------------------------------------------------------------------------

describe('intentParser – ambiguous and malformed input', () => {
  it('throws when deposit has no amount', async () => {
    stubSuccess({ action: 'deposit' });
    await assert.rejects(() => parseIntent('deposit some tokens'), /amount/i);
  });

  it('throws when withdraw has no amount', async () => {
    stubSuccess({ action: 'withdraw' });
    await assert.rejects(() => parseIntent('withdraw something'), /amount/i);
  });

  it('throws when amount is zero', async () => {
    stubSuccess({ action: 'deposit', amount: 0 });
    await assert.rejects(
      () => parseIntent('deposit nothing'),
      /amount must be greater than 0/i,
    );
  });

  it('throws when amount is negative', async () => {
    stubSuccess({ action: 'deposit', amount: -100 });
    await assert.rejects(
      () => parseIntent('deposit negative 100'),
      /amount must be greater than 0/i,
    );
  });

  it('throws when the response is not valid JSON', async () => {
    stubbedExecute = async (_op) => 'this is not json at all';
    await assert.rejects(() => parseIntent('gibberish'));
  });

  it('throws when the response is an empty string', async () => {
    stubbedExecute = async (_op) => '';
    await assert.rejects(() => parseIntent(''));
  });

  it('returns a valid result when extra unknown fields are present', async () => {
    stubSuccess({ action: 'balance', unknownField: 'ignored' });
    const result = await parseIntent('balance please');
    assert.strictEqual(result.action, 'balance');
  });

  it('handles null amount gracefully for actions that do not require it', async () => {
    stubSuccess({ action: 'balance', amount: null });
    const result = await parseIntent('balance');
    assert.strictEqual(result.action, 'balance');
  });
});

// ---------------------------------------------------------------------------
// Suite 3 – OpenAI API error handling
// ---------------------------------------------------------------------------

describe('intentParser – OpenAI API error handling', () => {
  it('propagates a generic API error', async () => {
    stubFailure(new Error('OpenAI service unavailable'));
    await assert.rejects(() => parseIntent('deposit 100'), /OpenAI service unavailable/);
  });

  it('propagates a 401 authentication error', async () => {
    stubFailure(Object.assign(new Error('Incorrect API key provided'), { status: 401 }));
    await assert.rejects(() => parseIntent('deposit 100'), /Incorrect API key/);
  });

  it('propagates a 429 quota-exceeded error', async () => {
    stubFailure(Object.assign(new Error('You exceeded your current quota'), { status: 429 }));
    await assert.rejects(() => parseIntent('check balance'), /quota/i);
  });

  it('propagates a 500 server error', async () => {
    stubFailure(Object.assign(new Error('Internal server error'), { status: 500 }));
    await assert.rejects(() => parseIntent('what is my APY?'), /Internal server error/);
  });

  it('propagates an all-keys-exhausted error from the key manager', async () => {
    stubFailure(new Error('All OpenAI API keys failed: Key sk-t...est1 failed: rate limit'));
    await assert.rejects(() => parseIntent('withdraw all'), /All OpenAI API keys failed/);
  });

  it('propagates the empty-response sentinel error', async () => {
    stubFailure(new Error('Failed to parse intent: Empty response from OpenAI'));
    await assert.rejects(() => parseIntent('deposit 100'), /Empty response/);
  });
});

// ---------------------------------------------------------------------------
// Suite 4 – Rate limiting behaviour
// ---------------------------------------------------------------------------

describe('intentParser – rate limiting behaviour', () => {
  it('throws a rate-limit error on 429', async () => {
    stubFailure(Object.assign(new Error('rate limit exceeded'), { status: 429 }));
    await assert.rejects(() => parseIntent('deposit 50'), /rate limit/i);
  });

  it('throws a quota error when quota message is embedded', async () => {
    stubFailure(new Error('You exceeded your current quota, please check your plan'));
    await assert.rejects(() => parseIntent('get my earnings'), /quota/i);
  });

  it('propagates rate-limit errors unchanged to the caller', async () => {
    const sentinel = new Error('sentinel-rate-limit-error');
    stubFailure(sentinel);
    try {
      await parseIntent('any message');
      assert.fail('expected to throw');
    } catch (err) {
      assert.strictEqual(err, sentinel);
    }
  });
});

// ---------------------------------------------------------------------------
// Suite 5 – Intent classification accuracy
// ---------------------------------------------------------------------------

describe('intentParser – intent classification accuracy', () => {
  const cases: Array<{ label: string; modelOutput: object; expected: Partial<ParsedIntent> }> = [
    {
      label: 'deposit with balanced strategy',
      modelOutput: { action: 'deposit', amount: 200, strategy: 'balanced' },
      expected: { action: 'deposit', amount: 200, strategy: 'balanced' },
    },
    {
      label: 'switch to growth strategy',
      modelOutput: { action: 'switch_strategy', strategy: 'growth' },
      expected: { action: 'switch_strategy', strategy: 'growth' },
    },
    {
      label: 'withdraw partial amount',
      modelOutput: { action: 'withdraw', amount: 75.5 },
      expected: { action: 'withdraw', amount: 75.5 },
    },
    {
      label: 'withdraw all',
      modelOutput: { action: 'withdraw', amount: 'all' },
      expected: { action: 'withdraw', amount: 'all' },
    },
    {
      label: 'balance query has no amount or strategy',
      modelOutput: { action: 'balance' },
      expected: { action: 'balance' },
    },
    {
      label: 'get_apy has no amount',
      modelOutput: { action: 'get_apy' },
      expected: { action: 'get_apy' },
    },
    {
      label: 'earnings has no amount',
      modelOutput: { action: 'earnings' },
      expected: { action: 'earnings' },
    },
  ];

  for (const { label, modelOutput, expected } of cases) {
    it(`correctly classifies: ${label}`, async () => {
      stubSuccess(modelOutput);
      const result = await parseIntent('any user message');
      for (const [key, value] of Object.entries(expected)) {
        assert.strictEqual(
          (result as any)[key],
          value,
          `field "${key}" should be ${JSON.stringify(value)}`,
        );
      }
    });
  }
});
