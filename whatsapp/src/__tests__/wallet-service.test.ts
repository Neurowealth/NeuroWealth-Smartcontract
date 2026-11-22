/**
 * Unit tests for walletService.ts (Issue #691, #1000)
 *
 * walletService imports @stellar/stellar-sdk (Keypair) and cryptoUtils.
 * cryptoUtils requires ENCRYPTION_KEY and PHONE_HASH_SALT at load time, so
 * we set those env vars before any import.
 *
 * createCustodialWallet, getWallet and getDecryptedSecretKey are all async
 * (they hit the database) - every call below must be awaited.
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
  return `wallet-test-hash-${Date.now()}-${++counter}`;
}

// ---------------------------------------------------------------------------
// createCustodialWallet
// ---------------------------------------------------------------------------

describe('createCustodialWallet', () => {
  test('returns a wallet with a valid Stellar public key', async () => {
    const { wallet, isNew } = await createCustodialWallet(ph());
    // Stellar public keys start with 'G' and are 56 chars
    assert.match(wallet.publicKey, /^G[A-Z2-7]{55}$/);
    assert.equal(isNew, true);
  });

  test('returns encrypted secret (never plaintext)', async () => {
    const { wallet } = await createCustodialWallet(ph());
    assert.ok(typeof wallet.encryptedSecret.encryptedData === 'string');
    assert.ok(wallet.encryptedSecret.encryptedData.length > 0);
    assert.ok(typeof wallet.encryptedSecret.iv === 'string');
    assert.ok(typeof wallet.encryptedSecret.tag === 'string');
    // Plaintext Stellar secret keys start with 'S' — must NOT appear
    assert.doesNotMatch(wallet.encryptedSecret.encryptedData, /^S[A-Z2-7]{55}$/);
  });

  test('each call generates a different keypair for a different phone', async () => {
    const { wallet: a } = await createCustodialWallet(ph());
    const { wallet: b } = await createCustodialWallet(ph());
    assert.notEqual(a.publicKey, b.publicKey);
  });

  // #1000: a returning user (e.g. a session that timed out and re-verified)
  // must get back the SAME wallet, not have it silently replaced. The fresh
  // keypair createCustodialWallet generates internally on a second call is
  // discarded in favor of the one already stored.
  test('returns the same wallet, not a new one, when called again for the same phone hash', async () => {
    const hash = ph();
    const first = await createCustodialWallet(hash);
    const second = await createCustodialWallet(hash);

    assert.equal(first.isNew, true);
    assert.equal(second.isNew, false);
    assert.equal(second.wallet.publicKey, first.wallet.publicKey);
    assert.deepEqual(second.wallet.encryptedSecret, first.wallet.encryptedSecret);

    const stored = await getWallet(hash);
    assert.equal(stored!.publicKey, first.wallet.publicKey);
  });

  // The whole point of #1000: the secret key behind the public key the user
  // is shown must be the one actually persisted, every time, including on a
  // re-verification. If this ever regresses to generating-but-discarding a
  // fresh keypair while showing its public half, this decrypts to the wrong
  // keypair and the assertion below catches it.
  test('the decrypted secret always matches the publicKey that was returned', async () => {
    const hash = ph();
    await createCustodialWallet(hash);
    const { wallet } = await createCustodialWallet(hash); // second call, same hash

    const secret = await getDecryptedSecretKey(hash);
    assert.ok(secret !== null);
    const { Keypair } = require('@stellar/stellar-sdk') as typeof import('@stellar/stellar-sdk');
    assert.equal(Keypair.fromSecret(secret!).publicKey(), wallet.publicKey);
  });

  test('sets createdAt to a recent timestamp', async () => {
    const before = Date.now();
    const { wallet } = await createCustodialWallet(ph());
    const after = Date.now();
    assert.ok(wallet.createdAt >= before);
    assert.ok(wallet.createdAt <= after);
  });
});

// ---------------------------------------------------------------------------
// getWallet
// ---------------------------------------------------------------------------

describe('getWallet', () => {
  test('returns the wallet after creation', async () => {
    const hash = ph();
    const { wallet: created } = await createCustodialWallet(hash);
    const fetched = await getWallet(hash);
    assert.ok(fetched !== undefined);
    assert.equal(fetched!.publicKey, created.publicKey);
  });

  test('returns undefined for an unknown phone hash', async () => {
    assert.equal(await getWallet('no-such-hash-xyz'), undefined);
  });
});

// ---------------------------------------------------------------------------
// getDecryptedSecretKey
// ---------------------------------------------------------------------------

describe('getDecryptedSecretKey', () => {
  test('decrypted secret key starts with S (valid Stellar secret format)', async () => {
    const hash = ph();
    await createCustodialWallet(hash);
    const secret = await getDecryptedSecretKey(hash);
    assert.ok(secret !== null);
    assert.match(secret!, /^S[A-Z2-7]{55}$/);
  });

  test('returns null for unknown phone hash', async () => {
    assert.equal(await getDecryptedSecretKey('unknown-hash-abc'), null);
  });

  test('decrypted secret matches the original keypair', async () => {
    const hash = ph();
    const { wallet } = await createCustodialWallet(hash);
    const secret = await getDecryptedSecretKey(hash);
    assert.ok(secret !== null);
    // Re-derive public key from the decrypted secret and compare
    const { Keypair } = require('@stellar/stellar-sdk') as typeof import('@stellar/stellar-sdk');
    const rederived = Keypair.fromSecret(secret!);
    assert.equal(rederived.publicKey(), wallet.publicKey);
  });
});
