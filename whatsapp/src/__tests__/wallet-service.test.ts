/**
 * Unit tests for walletService.ts (Issue #691)
 *
 * walletService imports @stellar/stellar-sdk (Keypair) and cryptoUtils.
 * cryptoUtils requires ENCRYPTION_KEY and PHONE_HASH_SALT at load time, so
 * we set those env vars before any import.
 */

// Set required env vars before any module load
process.env.ENCRYPTION_KEY = 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT = 'test-phone-salt';
process.env.VAULT_CONTRACT_ID = 'CTEST';

import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import {
  createCustodialWallet,
  getWallet,
  getDecryptedSecretKey,
} from '../walletService';

let counter = 0;
function ph(): string {
  return `wallet-test-hash-${++counter}`;
}

// ---------------------------------------------------------------------------
// createCustodialWallet
// ---------------------------------------------------------------------------

describe('createCustodialWallet', () => {
  test('returns a wallet with a valid Stellar public key', () => {
    const wallet = createCustodialWallet(ph());
    // Stellar public keys start with 'G' and are 56 chars
    assert.match(wallet.publicKey, /^G[A-Z2-7]{55}$/);
  });

  test('returns encrypted secret (never plaintext)', () => {
    const wallet = createCustodialWallet(ph());
    assert.ok(typeof wallet.encryptedSecret.encryptedData === 'string');
    assert.ok(wallet.encryptedSecret.encryptedData.length > 0);
    assert.ok(typeof wallet.encryptedSecret.iv === 'string');
    assert.ok(typeof wallet.encryptedSecret.tag === 'string');
    // Plaintext Stellar secret keys start with 'S' — must NOT appear
    assert.doesNotMatch(wallet.encryptedSecret.encryptedData, /^S[A-Z2-7]{55}$/);
  });

  test('each call generates a different keypair', () => {
    const a = createCustodialWallet(ph());
    const b = createCustodialWallet(ph());
    assert.notEqual(a.publicKey, b.publicKey);
  });

  test('overwrites the wallet when called again for the same phone hash', () => {
    const hash = ph();
    const first = createCustodialWallet(hash);
    const second = createCustodialWallet(hash);
    assert.notEqual(first.publicKey, second.publicKey);
    // getWallet now returns the second one
    assert.equal(getWallet(hash)!.publicKey, second.publicKey);
  });

  test('sets createdAt to a recent timestamp', () => {
    const before = Date.now();
    const wallet = createCustodialWallet(ph());
    const after = Date.now();
    assert.ok(wallet.createdAt >= before);
    assert.ok(wallet.createdAt <= after);
  });
});

// ---------------------------------------------------------------------------
// getWallet
// ---------------------------------------------------------------------------

describe('getWallet', () => {
  test('returns the wallet after creation', () => {
    const hash = ph();
    const created = createCustodialWallet(hash);
    const fetched = getWallet(hash);
    assert.ok(fetched !== undefined);
    assert.equal(fetched!.publicKey, created.publicKey);
  });

  test('returns undefined for an unknown phone hash', () => {
    assert.equal(getWallet('no-such-hash-xyz'), undefined);
  });
});

// ---------------------------------------------------------------------------
// getDecryptedSecretKey
// ---------------------------------------------------------------------------

describe('getDecryptedSecretKey', () => {
  test('decrypted secret key starts with S (valid Stellar secret format)', () => {
    const hash = ph();
    createCustodialWallet(hash);
    const secret = getDecryptedSecretKey(hash);
    assert.ok(secret !== null);
    assert.match(secret!, /^S[A-Z2-7]{55}$/);
  });

  test('returns null for unknown phone hash', () => {
    assert.equal(getDecryptedSecretKey('unknown-hash-abc'), null);
  });

  test('decrypted secret matches the original keypair', () => {
    const hash = ph();
    const wallet = createCustodialWallet(hash);
    const secret = getDecryptedSecretKey(hash);
    assert.ok(secret !== null);
    // Re-derive public key from the decrypted secret and compare
    const { Keypair } = require('@stellar/stellar-sdk') as typeof import('@stellar/stellar-sdk');
    const rederived = Keypair.fromSecret(secret!);
    assert.equal(rederived.publicKey(), wallet.publicKey);
  });
});
