import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { Writable } from 'node:stream';
import { createLogger } from './logger';

describe('structured logger redaction (#864)', () => {
  it('redacts configured secrets and authorization data, shortens addresses, and hashes user content', () => {
    const output: string[] = [];
    const stream = new Writable({
      write(chunk, _encoding, callback) {
        output.push(String(chunk));
        callback();
      },
    });
    const canary = 'canary-agent-secret-987654';
    process.env.AGENT_LOG_CANARY_SECRET = canary;

    createLogger(stream).info({
      authorization: `Bearer ${canary}`,
      stellarAddress: `G${'A'.repeat(55)}`,
      userMessage: 'withdraw all my funds from vault',
      error: new Error(`provider failed with key=${canary}`),
    }, `provider response included ${canary}`);

    const serialized = output.join('');
    assert.ok(!serialized.includes(canary));
    assert.ok(serialized.includes('[REDACTED]'));
    assert.ok(serialized.includes('GAAAAA...AAAA'));
    assert.ok(serialized.includes('sha256:'));
    const record = JSON.parse(serialized) as { error: { type: string; message: string } };
    assert.strictEqual(record.error.type, 'Error');
    assert.match(record.error.message, /provider failed/);

    delete process.env.AGENT_LOG_CANARY_SECRET;
  });

  it('redacts URL credentials embedded in RPC errors while retaining diagnostic context', () => {
    const output: string[] = [];
    const stream = new Writable({
      write(chunk, _encoding, callback) {
        output.push(String(chunk));
        callback();
      },
    });
    createLogger(stream).error({
      error: new Error('request failed for https://user:db-password-canary@rpc.local/v1'),
      code: 'ECONNRESET',
    }, 'RPC request failed');

    const serialized = output.join('');
    assert.ok(!serialized.includes('db-password-canary'));
    const record = JSON.parse(serialized) as { error: { message: string }; code: string };
    assert.match(record.error.message, /request failed/);
    assert.strictEqual(record.code, 'ECONNRESET');
  });
});