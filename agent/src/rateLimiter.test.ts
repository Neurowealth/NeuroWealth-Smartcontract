import { describe, it } from 'node:test';
import assert from 'node:assert';
import { ipRateLimiter, userRateLimiter } from './rateLimiter';

describe('Rate Limiter (#713)', () => {
  it('exports ipRateLimiter and userRateLimiter middleware functions', () => {
    assert.strictEqual(typeof ipRateLimiter, 'function');
    assert.strictEqual(typeof userRateLimiter, 'function');
  });
});
