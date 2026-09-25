/**
 * Unit tests for vaultRouter.ts (Issue #691)
 *
 * vaultRouter calls requireEnv('VAULT_CONTRACT_ID') at module load time, so
 * we set that env var first. getPortfolio / handleDeposit / handleWithdraw
 * currently use simulated responses, so no RPC stub is needed.
 */

process.env.ENCRYPTION_KEY = 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT = 'test-phone-salt';
process.env.VAULT_CONTRACT_ID = 'CTEST_VAULT_CONTRACT_ID';

import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { createCustodialWallet } from '../walletService';
import {
  getPortfolio,
  handleDeposit,
  handleWithdraw,
} from '../vaultRouter';

let counter = 0;
function ph(): string {
  return `vault-test-${++counter}`;
}

/** Create a wallet and return the phone hash (required before vault calls). */
function setupWallet(): string {
  const hash = ph();
  createCustodialWallet(hash);
  return hash;
}

// ---------------------------------------------------------------------------
// getPortfolio
// ---------------------------------------------------------------------------

describe('getPortfolio', () => {
  test('returns portfolio with required numeric fields', async () => {
    const hash = setupWallet();
    const p = await getPortfolio(hash);
    assert.ok(typeof p.balance === 'number');
    assert.ok(typeof p.usdEquivalent === 'number');
    assert.ok(typeof p.apy === 'number');
    assert.ok(typeof p.dailyEarnings === 'number');
    assert.ok(typeof p.strategy === 'string' && p.strategy.length > 0);
  });

  test('returns positive balance and APY', async () => {
    const hash = setupWallet();
    const p = await getPortfolio(hash);
    assert.ok(p.balance > 0);
    assert.ok(p.apy > 0);
  });

  test('throws when wallet does not exist', async () => {
    await assert.rejects(
      () => getPortfolio('no-wallet-hash'),
      /wallet not found/i,
    );
  });
});

// ---------------------------------------------------------------------------
// handleDeposit
// ---------------------------------------------------------------------------

describe('handleDeposit', () => {
  test('returns success=true with a txHash for a valid deposit', async () => {
    const hash = setupWallet();
    const result = await handleDeposit(hash, 100);
    assert.equal(result.success, true);
    assert.ok(result.txHash.length > 0);
    assert.ok(result.message.length > 0);
  });

  test('message includes the deposit amount', async () => {
    const hash = setupWallet();
    const result = await handleDeposit(hash, 250);
    assert.match(result.message, /250/);
  });

  test('message includes the strategy when provided', async () => {
    const hash = setupWallet();
    const result = await handleDeposit(hash, 100, 'growth');
    assert.match(result.message, /growth/i);
  });

  test('uses Balanced strategy when none is provided', async () => {
    const hash = setupWallet();
    const result = await handleDeposit(hash, 100);
    assert.match(result.message, /balanced/i);
  });

  test('returns success=false when wallet does not exist', async () => {
    const result = await handleDeposit('no-wallet', 100);
    assert.equal(result.success, false);
    assert.match(result.message, /wallet/i);
  });

  test('txHash is a hex string prefixed with 0x', async () => {
    const hash = setupWallet();
    const result = await handleDeposit(hash, 50);
    assert.match(result.txHash, /^0x[0-9a-f]+/i);
  });
});

// ---------------------------------------------------------------------------
// handleWithdraw
// ---------------------------------------------------------------------------

describe('handleWithdraw', () => {
  test('returns success=true for a partial withdraw', async () => {
    const hash = setupWallet();
    const result = await handleWithdraw(hash, 50, false);
    assert.equal(result.success, true);
    assert.ok(result.txHash.length > 0);
  });

  test('message includes the withdraw amount for partial withdraw', async () => {
    const hash = setupWallet();
    const result = await handleWithdraw(hash, 75, false);
    assert.match(result.message, /75/);
  });

  test('message says "all funds" for a withdraw-all', async () => {
    const hash = setupWallet();
    const result = await handleWithdraw(hash, undefined, true);
    assert.equal(result.success, true);
    assert.match(result.message, /all funds/i);
  });

  test('returns success=false when wallet does not exist', async () => {
    const result = await handleWithdraw('no-wallet', 50, false);
    assert.equal(result.success, false);
    assert.match(result.message, /wallet/i);
  });

  test('txHash is a hex string prefixed with 0x', async () => {
    const hash = setupWallet();
    const result = await handleWithdraw(hash, 30, false);
    assert.match(result.txHash, /^0x[0-9a-f]+/i);
  });
});
