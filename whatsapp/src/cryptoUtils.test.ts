import { describe, it } from 'node:test';
import assert from 'node:assert';
import { hashPhoneNumber, encryptSecretKey, decryptSecretKey } from './cryptoUtils';

// Crypto helpers (#691). The encryption here protects custodial Stellar secret
// keys, so the round trip and the tamper detection are the two things worth
// pinning.

describe('hashPhoneNumber (#691)', () => {
  it('is deterministic for the same number', () => {
    assert.strictEqual(hashPhoneNumber('whatsapp:+14155550123'), hashPhoneNumber('whatsapp:+14155550123'));
  });

  it('produces a 64-character hex digest and never the number itself', () => {
    const hash = hashPhoneNumber('whatsapp:+14155550123');
    assert.match(hash, /^[0-9a-f]{64}$/);
    assert.ok(!hash.includes('14155550123'), 'the digest must not carry the phone number');
  });

  it('separates different numbers', () => {
    assert.notStrictEqual(hashPhoneNumber('whatsapp:+14155550123'), hashPhoneNumber('whatsapp:+14155550124'));
  });
});

describe('encryptSecretKey / decryptSecretKey (#691)', () => {
  const SECRET = 'SAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA1';

  it('round-trips a secret key', () => {
    const sealed = encryptSecretKey(SECRET);
    assert.strictEqual(decryptSecretKey(sealed.encryptedData, sealed.iv, sealed.tag), SECRET);
  });

  it('never stores the plaintext key in the ciphertext', () => {
    const sealed = encryptSecretKey(SECRET);
    assert.ok(!sealed.encryptedData.includes(SECRET));
    assert.notStrictEqual(sealed.encryptedData, SECRET);
  });

  it('uses a fresh IV per call, so the same key encrypts differently each time', () => {
    const first = encryptSecretKey(SECRET);
    const second = encryptSecretKey(SECRET);
    assert.notStrictEqual(first.iv, second.iv);
    assert.notStrictEqual(first.encryptedData, second.encryptedData);
  });

  it('emits a 12-byte IV and a 16-byte GCM tag as hex', () => {
    const sealed = encryptSecretKey(SECRET);
    assert.strictEqual(sealed.iv.length, 24);
    assert.strictEqual(sealed.tag.length, 32);
  });

  it('refuses to decrypt a tampered ciphertext', () => {
    const sealed = encryptSecretKey(SECRET);
    const lastTwo = sealed.encryptedData.slice(-2);
    const tampered = sealed.encryptedData.slice(0, -2) + (lastTwo === '00' ? '11' : '00');
    assert.throws(() => decryptSecretKey(tampered, sealed.iv, sealed.tag));
  });

  it('refuses to decrypt with a wrong auth tag', () => {
    const sealed = encryptSecretKey(SECRET);
    assert.throws(() => decryptSecretKey(sealed.encryptedData, sealed.iv, '00'.repeat(16)));
  });
});
