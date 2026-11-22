import { test } from 'node:test';
import assert from 'node:assert/strict';

// Set up env vars before importing the module to avoid "must be set" errors
process.env.ENCRYPTION_KEY = 'test-encryption-key-that-is-at-least-32-chars-long';
process.env.PHONE_HASH_SALT = 'test-salt';

import { hashPhoneNumber, encryptSecretKey, decryptSecretKey } from '../cryptoUtils';

test('hashPhoneNumber stability', () => {
  const hash1 = hashPhoneNumber('+1234567890');
  const hash2 = hashPhoneNumber('+1234567890');
  assert.equal(hash1, hash2);
  assert.notEqual(hash1, hashPhoneNumber('+0987654321'));
});

test('encrypt/decrypt round-trip', () => {
  const secret = 'my-secret-stellar-key';
  const encrypted = encryptSecretKey(secret);
  
  assert.ok(encrypted.encryptedData);
  assert.ok(encrypted.iv);
  assert.ok(encrypted.tag);
  
  const decrypted = decryptSecretKey(encrypted.encryptedData, encrypted.iv, encrypted.tag);
  assert.equal(decrypted, secret);
});

test('uniqueness of IV and encrypted data', () => {
  const secret = 'my-secret-stellar-key';
  const enc1 = encryptSecretKey(secret);
  const enc2 = encryptSecretKey(secret);
  
  assert.notEqual(enc1.iv, enc2.iv);
  assert.notEqual(enc1.encryptedData, enc2.encryptedData);
});

test('tamper detection throws an error', () => {
  const secret = 'my-secret-stellar-key';
  const encrypted = encryptSecretKey(secret);
  
  // Tamper with the encrypted data
  const tamperedData = encrypted.encryptedData.replace(/[0-9a-f]/, (m) => (m === '0' ? '1' : '0'));
  assert.throws(() => {
    decryptSecretKey(tamperedData, encrypted.iv, encrypted.tag);
  });
  
  // Tamper with IV
  const tamperedIv = encrypted.iv.replace(/[0-9a-f]/, (m) => (m === '0' ? '1' : '0'));
  assert.throws(() => {
    decryptSecretKey(encrypted.encryptedData, tamperedIv, encrypted.tag);
  });

  // Tamper with Tag
  const tamperedTag = encrypted.tag.replace(/[0-9a-f]/, (m) => (m === '0' ? '1' : '0'));
  assert.throws(() => {
    decryptSecretKey(encrypted.encryptedData, encrypted.iv, tamperedTag);
  });
});

test('fail-fast when ENCRYPTION_KEY is too short', () => {
  const originalKey = process.env.ENCRYPTION_KEY;
  process.env.ENCRYPTION_KEY = 'short';
  
  const modulePath = require.resolve('../cryptoUtils');
  delete require.cache[modulePath];
  
  assert.throws(() => {
    require('../cryptoUtils');
  }, /ENCRYPTION_KEY must be at least 32 characters/);
  
  process.env.ENCRYPTION_KEY = originalKey;
  delete require.cache[modulePath];
});
