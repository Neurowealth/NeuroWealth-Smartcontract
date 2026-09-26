import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  redactSensitiveData,
  sanitizeErrorMessage,
  getSanitizedErrorInfo,
} from './errorSanitizer';

describe('errorSanitizer', () => {
  const originalEnv = process.env.NODE_ENV;

  afterEach(() => {
    process.env.NODE_ENV = originalEnv;
  });

  describe('redactSensitiveData', () => {
    it('redacts valid Stellar secret keys (starts with S, 56 characters base32)', () => {
      // 56 char Stellar secret key
      const secretKey = 'SB2Z2XNJWOUJMYZKLX2R5R32NKV7F6O2V7J4OQ5OOU4N7X3U5D2H67AB';
      const input = `Failed to sign transaction with key ${secretKey} on testnet`;
      const sanitized = redactSensitiveData(input);

      expect(sanitized).not.toContain(secretKey);
      expect(sanitized).toContain('[REDACTED_SECRET_KEY]');
    });

    it('redacts Authorization bearer tokens and credentials', () => {
      const input = 'Authorization failed: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.xyz and apikey=secret_token_12345';
      const sanitized = redactSensitiveData(input);

      expect(sanitized).not.toContain('eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9');
      expect(sanitized).not.toContain('secret_token_12345');
      expect(sanitized).toContain('[REDACTED_CREDENTIAL]');
    });

    it('redacts sensitive query string parameters', () => {
      const input = 'Fetch error from https://api.stellar.org/rpc?token=abc12345678&secret=xyz98765432';
      const sanitized = redactSensitiveData(input);

      expect(sanitized).not.toContain('abc12345678');
      expect(sanitized).not.toContain('xyz98765432');
      expect(sanitized).toContain('[REDACTED_PARAM]');
    });

    it('handles empty or non-string inputs safely', () => {
      expect(redactSensitiveData('')).toBe('');
      // @ts-expect-error test non-string
      expect(redactSensitiveData(null)).toBe('');
      // @ts-expect-error test non-string
      expect(redactSensitiveData(undefined)).toBe('');
    });
  });

  describe('sanitizeErrorMessage', () => {
    it('sanitizes standard Error objects and redacts secrets', () => {
      const secret = 'SC5O2V7J4OQ5OOU4N7X3U5D2H67AB2Z2XNJWOUJMYZKLX2R5R32NKV7F6O';
      const err = new Error(`Connection timeout using secret=${secret}`);
      const result = sanitizeErrorMessage(err);

      expect(result).not.toContain(secret);
      expect(result).toContain('[REDACTED_CREDENTIAL]');
    });

    it('suppresses internal paths and stack dumps in production', () => {
      process.env.NODE_ENV = 'production';
      const internalErr = new Error('ChunkLoadError: /home/finite/app/node_modules/next/dist/client.js failed');
      const result = sanitizeErrorMessage(internalErr);

      expect(result).toBe('An unexpected system error occurred. Please try again.');
    });

    it('preserves user-facing message in non-production', () => {
      process.env.NODE_ENV = 'development';
      const userErr = new Error('Network request timed out. Please check your internet connection.');
      const result = sanitizeErrorMessage(userErr);

      expect(result).toBe('Network request timed out. Please check your internet connection.');
    });

    it('handles plain string errors', () => {
      expect(sanitizeErrorMessage('Custom error message')).toBe('Custom error message');
    });

    it('handles undefined or null error gracefully', () => {
      expect(sanitizeErrorMessage(null)).toBe('An unexpected error occurred.');
      expect(sanitizeErrorMessage(undefined)).toBe('An unexpected error occurred.');
    });
  });

  describe('getSanitizedErrorInfo', () => {
    it('extracts digest when present on the error object', () => {
      const errorWithDigest = Object.assign(new Error('Render crashed'), { digest: '12345-abcde' });
      const info = getSanitizedErrorInfo(errorWithDigest, 'portfolio');

      expect(info.digest).toBe('12345-abcde');
      expect(info.title).toBe('Unable to load portfolio data');
    });

    it('suppresses stack trace in production', () => {
      process.env.NODE_ENV = 'production';
      const err = new Error('Some error');
      const info = getSanitizedErrorInfo(err, 'transactions');

      expect(info.sanitizedStack).toBeUndefined();
      expect(info.title).toBe('Unable to load transactions');
    });

    it('provides sanitized stack in development without exposing secrets', () => {
      process.env.NODE_ENV = 'development';
      const secret = 'SD2H67AB2Z2XNJWOUJMYZKLX2R5R32NKV7F6O2V7J4OQ5OOU4N7X3U5D2H';
      const err = new Error(`Error with password=${secret}`);
      const info = getSanitizedErrorInfo(err, 'strategies');

      expect(info.title).toBe('Unable to load strategy chart');
      if (err.stack) {
        expect(info.sanitizedStack).toBeDefined();
        expect(info.sanitizedStack).not.toContain(secret);
      }
    });
  });
});
