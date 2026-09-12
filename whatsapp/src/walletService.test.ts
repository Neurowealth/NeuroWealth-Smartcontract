import { describe, it } from 'node:test';
import assert from 'node:assert';
import { Keypair } from '@stellar/stellar-sdk';
import { createCustodialWallet, getWallet, getDecryptedSecretKey } from './walletService';

// Custodial wallet creation (#691). The key that matters is that the stored
// secret still decrypts to the pair that owns the stored public key.

let counter = 0;
const freshPhone = () => 'wallet-test-' + (counter += 1);

describe('createCustodialWallet (#691)', () => {
  it('creates a Stellar keypair shaped public key', () => {
    const wallet = createCustodialWallet(freshPhone());
    assert.match(wallet.publicKey, /^G[A-Z0-9]{55}$/);
    assert.strictEqual(wallet.publicKey.length, 56);
    assert.ok(wallet.createdAt > 0);
  });

  it('stores only the encrypted secret, never the plaintext', () => {
    const wallet = createCustodialWallet(freshPhone());
    assert.ok(wallet.encryptedSecret.encryptedData.length > 0);
    assert.ok(wallet.encryptedSecret.iv.length > 0);
    assert.ok(wallet.encryptedSecret.tag.length > 0);
    assert.strictEqual(Object.keys(wallet.encryptedSecret).sort().join(','), 'encryptedData,iv,tag');
  });

  it('generates a different wallet per phone', () => {
    const first = createCustodialWallet(freshPhone());
    const second = createCustodialWallet(freshPhone());
    assert.notStrictEqual(first.publicKey, second.publicKey);
  });
});

describe('getWallet / getDecryptedSecretKey (#691)', () => {
  it('returns undefined for a phone with no wallet', () => {
    assert.strictEqual(getWallet('never-created'), undefined);
    assert.strictEqual(getDecryptedSecretKey('never-created'), null);
  });

  it('decrypts to the secret that owns the stored public key', () => {
    const phone = freshPhone();
    const wallet = createCustodialWallet(phone);

    const secret = getDecryptedSecretKey(phone);
    assert.ok(secret, 'the secret should decrypt');
    assert.match(secret!, /^S[A-Z0-9]{55}$/);
    assert.strictEqual(Keypair.fromSecret(secret!).publicKey(), wallet.publicKey);
  });

  it('keeps wallets separate per phone', () => {
    const first = freshPhone();
    const second = freshPhone();
    const firstWallet = createCustodialWallet(first);
    createCustodialWallet(second);

    assert.strictEqual(getWallet(first)!.publicKey, firstWallet.publicKey);
    assert.notStrictEqual(getDecryptedSecretKey(first), getDecryptedSecretKey(second));
  });
});
