/**
 * Boot-time rejection of insecure configuration (#945).
 */
import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { assertEnv, validateEnv } from '../envGuard';

const GOOD: NodeJS.ProcessEnv = {
  NODE_ENV: 'production',
  ENCRYPTION_KEY: 'test-encryption-key-at-least-32-chars-long!',
  PHONE_HASH_SALT: 'test-phone-salt-0123456789',
  DATABASE_URL: 'postgresql://neurowealth_app@db.internal:5432/neurowealth',
  TWILIO_AUTH_TOKEN: 'test-twilio-auth-token-0123456789',
  SOROBAN_RPC_URL: 'https://soroban-testnet.stellar.org',
  VAULT_CONTRACT_ID: 'CTEST_VAULT',
};

function with_(overrides: Record<string, string | undefined>): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { ...GOOD };
  for (const [k, v] of Object.entries(overrides)) {
    if (v === undefined) delete env[k];
    else env[k] = v;
  }
  return env;
}

describe('validateEnv', () => {
  test('accepts a complete, strong configuration', () => {
    assert.deepEqual(validateEnv(GOOD), []);
    assert.doesNotThrow(() => assertEnv(GOOD));
  });

  for (const name of ['ENCRYPTION_KEY', 'PHONE_HASH_SALT', 'DATABASE_URL', 'TWILIO_AUTH_TOKEN', 'SOROBAN_RPC_URL', 'VAULT_CONTRACT_ID']) {
    test(`rejects a missing ${name} instead of falling back to a default`, () => {
      const problems = validateEnv(with_({ [name]: undefined }));
      assert.ok(problems.some((p) => p.startsWith(name)), problems.join('\n'));
      assert.throws(() => assertEnv(with_({ [name]: undefined })), new RegExp(name));
    });
  }

  test('rejects short and placeholder secrets', () => {
    assert.ok(validateEnv(with_({ ENCRYPTION_KEY: 'short' })).some((p) => /ENCRYPTION_KEY.*32/.test(p)));
    assert.ok(validateEnv(with_({ ENCRYPTION_KEY: 'x'.repeat(40) })).some((p) => /placeholder|low-entropy/.test(p)));
    assert.ok(validateEnv(with_({ PHONE_HASH_SALT: 'changeme' })).some((p) => /PHONE_HASH_SALT/.test(p)));
    assert.ok(validateEnv(with_({ PHONE_HASH_SALT: 'a'.repeat(20) })).some((p) => /low-entropy/.test(p)));
  });

  test('rejects the well-known local database credentials', () => {
    const problems = validateEnv(with_({ DATABASE_URL: 'postgresql://postgres:postgres@localhost:5432/neurowealth' }));
    assert.ok(problems.some((p) => /default postgres:postgres/.test(p)));
  });

  test('refuses to skip Twilio signature validation in production', () => {
    const prod = validateEnv(with_({ TWILIO_SKIP_SIGNATURE_VALIDATION: 'true' }));
    assert.ok(prod.some((p) => /TWILIO_SKIP_SIGNATURE_VALIDATION/.test(p)));

    // Local development may opt out explicitly, and then needs no token.
    const dev = validateEnv(with_({ NODE_ENV: 'development', TWILIO_SKIP_SIGNATURE_VALIDATION: 'true', TWILIO_AUTH_TOKEN: undefined }));
    assert.deepEqual(dev, []);
  });

  test('validates the key ring settings', () => {
    assert.ok(validateEnv(with_({ ENCRYPTION_KEY_VERSION: 'bad version!' })).some((p) => /ENCRYPTION_KEY_VERSION/.test(p)));
    assert.ok(validateEnv(with_({ ENCRYPTION_KEY_RETIRED: 'garbage' })).some((p) => /ENCRYPTION_KEY_RETIRED/.test(p)));
  });

  test('rejects non-numeric tuning values', () => {
    assert.ok(validateEnv(with_({ WHATSAPP_OTP_TTL_MS: 'soon' })).some((p) => /WHATSAPP_OTP_TTL_MS/.test(p)));
    assert.ok(validateEnv(with_({ WHATSAPP_OTP_MAX_ATTEMPTS: '0' })).some((p) => /WHATSAPP_OTP_MAX_ATTEMPTS/.test(p)));
  });

  test('reports every problem at once and never echoes secret values', () => {
    const env = with_({ ENCRYPTION_KEY: 'hunter2', PHONE_HASH_SALT: undefined, DATABASE_URL: undefined });
    const problems = validateEnv(env);
    assert.ok(problems.length >= 3);
    assert.ok(!problems.join('\n').includes('hunter2'));
    assert.throws(() => assertEnv(env), /Refusing to start/);
  });
});
