import { describe, it } from 'node:test';
import assert from 'node:assert';
import { ipRateLimiter, userRateLimiter } from './rateLimiter';

describe('Rate Limiter (#713)', () => {
  it('exports ipRateLimiter and userRateLimiter middleware functions', () => {
    assert.strictEqual(typeof ipRateLimiter, 'function');
    assert.strictEqual(typeof userRateLimiter, 'function');
  });
});

describe('userRateLimiter identity (#1119)', () => {
  it('ignores spoofable x-user-id headers', async () => {
    const { getVerifiedUserId } = await import('./rateLimiter');
    assert.strictEqual(getVerifiedUserId({ locals: {} } as any), undefined);
    assert.strictEqual(getVerifiedUserId({ locals: { authenticatedUserId: ' u1 ' } } as any), 'u1');
  });
});
