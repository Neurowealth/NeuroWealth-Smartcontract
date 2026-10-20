/**
 * Unit tests for vaultRouter.ts (Issue #691, #1001)
 *
 * vaultRouter calls requireEnv('VAULT_CONTRACT_ID')/('SOROBAN_RPC_URL') at
 * module load time, so both are set first.
 *
 * #1001: getPortfolio / handleDeposit / handleWithdraw now submit real
 * @neurowealth/vault-client invocations rather than fabricating a result,
 * so exercising them here means mocking VaultClient.prototype's methods
 * (every `new VaultClient(...)` vaultRouter makes shares the same
 * prototype) rather than hitting testnet or returning a canned simulated
 * response.
 */

process.env.ENCRYPTION_KEY = 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT = 'test-phone-salt';
process.env.VAULT_CONTRACT_ID = 'CTEST_VAULT_CONTRACT_ID';
process.env.SOROBAN_RPC_URL = 'https://soroban-testnet.stellar.org';

import { test, describe, mock, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { VaultClient, VaultError, VaultErrorCode } from '@neurowealth/vault-client';
import { createCustodialWallet } from '../walletService';
import {
  getPortfolio,
  handleDeposit,
  handleWithdraw,
} from '../vaultRouter';

let counter = 0;
function ph(): string {
  return `vault-test-${Date.now()}-${++counter}`;
}

/** Create a wallet and return the phone hash (required before vault calls). */
async function setupWallet(): Promise<string> {
  const hash = ph();
  await createCustodialWallet(hash);
  return hash;
}

const FAKE_HASH = 'a'.repeat(64);

afterEach(() => {
  mock.restoreAll();
});

// ---------------------------------------------------------------------------
// getPortfolio
// ---------------------------------------------------------------------------

describe('getPortfolio', () => {
  beforeEach(() => {
    mock.method(VaultClient.prototype, 'get_balance', () => Promise.resolve(1_000_0000000n));
    mock.method(VaultClient.prototype, 'get_user_strategy', () => Promise.resolve('growth'));
    mock.method(VaultClient.prototype, 'get_exchange_rate', () => Promise.resolve(10_500_0000n));
    mock.method(VaultClient.prototype, 'get_total_assets', () => Promise.resolve(2_000_0000000n));
    mock.method(VaultClient.prototype, 'get_total_shares', () => Promise.resolve(1_800_0000000n));
  });

  test('returns portfolio with required numeric fields', async () => {
    const hash = await setupWallet();
    const p = await getPortfolio(hash);
    assert.ok(typeof p.balance === 'number');
    assert.ok(typeof p.usdEquivalent === 'number');
    assert.ok(typeof p.apy === 'number');
    assert.ok(typeof p.dailyEarnings === 'number');
    assert.ok(typeof p.strategy === 'string' && p.strategy.length > 0);
  });

  test('scales raw stroops by 10^7 and maps the strategy name', async () => {
    const hash = await setupWallet();
    const p = await getPortfolio(hash);
    assert.equal(p.balance, 1_000);
    assert.equal(p.strategy, 'Growth');
  });

  test('returns positive balance and APY when the contract reports growth', async () => {
    const hash = await setupWallet();
    const p = await getPortfolio(hash);
    assert.ok(p.balance > 0);
    assert.ok(p.apy > 0);
  });

  test('falls back to zeroed fields when the RPC call fails', async () => {
    mock.method(VaultClient.prototype, 'get_balance', () => Promise.reject(new Error('RPC unavailable')));
    const hash = await setupWallet();
    const p = await getPortfolio(hash);
    assert.deepEqual(p, {
      balance: 0,
      usdEquivalent: 0,
      strategy: 'Balanced',
      apy: 0,
      dailyEarnings: 0,
    });
  });

  test('throws when wallet does not exist', async () => {
    await assert.rejects(
      () => getPortfolio('no-wallet-hash'),
      /wallet not found/i,
    );
  });
});

// ---------------------------------------------------------------------------
// handleDeposit (#1001)
// ---------------------------------------------------------------------------

describe('handleDeposit', () => {
  test('returns success=true with the real tx hash for a valid deposit', async () => {
    const depositMock = mock.method(VaultClient.prototype, 'deposit', () =>
      Promise.resolve({ hash: FAKE_HASH }),
    );
    const hash = await setupWallet();
    const result = await handleDeposit(hash, 100);

    assert.equal(result.success, true);
    assert.equal(result.txHash, FAKE_HASH);
    assert.equal(depositMock.mock.callCount(), 1);
  });

  test('scales the USDC amount to stroops (10^7) when calling deposit', async () => {
    const depositMock = mock.method(VaultClient.prototype, 'deposit', () =>
      Promise.resolve({ hash: FAKE_HASH }),
    );
    const hash = await setupWallet();
    await handleDeposit(hash, 100);

    const call = depositMock.mock.calls[0];
    assert.equal(call.arguments[2], 1_000_000_000n);
  });

  test('message includes the deposit amount', async () => {
    mock.method(VaultClient.prototype, 'deposit', () => Promise.resolve({ hash: FAKE_HASH }));
    const hash = await setupWallet();
    const result = await handleDeposit(hash, 250);
    assert.match(result.message, /250/);
  });

  test('message includes the strategy when provided', async () => {
    mock.method(VaultClient.prototype, 'deposit', () => Promise.resolve({ hash: FAKE_HASH }));
    const hash = await setupWallet();
    const result = await handleDeposit(hash, 100, 'growth');
    assert.match(result.message, /growth/i);
  });

  test('uses Balanced strategy when none is provided', async () => {
    mock.method(VaultClient.prototype, 'deposit', () => Promise.resolve({ hash: FAKE_HASH }));
    const hash = await setupWallet();
    const result = await handleDeposit(hash, 100);
    assert.match(result.message, /balanced/i);
  });

  test('returns success=false when wallet does not exist', async () => {
    const result = await handleDeposit('no-wallet', 100);
    assert.equal(result.success, false);
    assert.equal(result.txHash, '');
    assert.match(result.message, /wallet/i);
  });

  // The core of #1001: no success reply without on-chain confirmation, and
  // the error message must not leak the raw contract error.
  test('returns success=false and a friendly message when the contract rejects the deposit', async () => {
    mock.method(VaultClient.prototype, 'deposit', () =>
      Promise.reject(new VaultError(VaultErrorCode.ExceedsTvlCap, 'Vault contract error 41')),
    );
    const hash = await setupWallet();
    const result = await handleDeposit(hash, 100);

    assert.equal(result.success, false);
    assert.equal(result.txHash, '');
    assert.match(result.message, /cap/i);
    assert.doesNotMatch(result.message, /41|VaultError|contract error/i);
  });

  test('returns success=false when the transaction never confirms (no hash)', async () => {
    mock.method(VaultClient.prototype, 'deposit', () => Promise.resolve({ hash: undefined }));
    const hash = await setupWallet();
    const result = await handleDeposit(hash, 100);

    assert.equal(result.success, false);
    assert.equal(result.txHash, '');
  });
});

// ---------------------------------------------------------------------------
// handleWithdraw (#1001)
// ---------------------------------------------------------------------------

describe('handleWithdraw', () => {
  test('returns success=true with the real tx hash for a partial withdraw', async () => {
    const withdrawMock = mock.method(VaultClient.prototype, 'withdraw', () =>
      Promise.resolve({ hash: FAKE_HASH }),
    );
    const hash = await setupWallet();
    const result = await handleWithdraw(hash, 50, false);

    assert.equal(result.success, true);
    assert.equal(result.txHash, FAKE_HASH);
    assert.equal(withdrawMock.mock.callCount(), 1);
  });

  test('scales the USDC amount to stroops (10^7) when calling withdraw', async () => {
    const withdrawMock = mock.method(VaultClient.prototype, 'withdraw', () =>
      Promise.resolve({ hash: FAKE_HASH }),
    );
    const hash = await setupWallet();
    await handleWithdraw(hash, 75, false);

    const call = withdrawMock.mock.calls[0];
    assert.equal(call.arguments[2], 750_000_000n);
  });

  test('message includes the withdraw amount for partial withdraw', async () => {
    mock.method(VaultClient.prototype, 'withdraw', () => Promise.resolve({ hash: FAKE_HASH }));
    const hash = await setupWallet();
    const result = await handleWithdraw(hash, 75, false);
    assert.match(result.message, /75/);
  });

  test('calls withdraw_all (not withdraw) for a withdraw-all, with the real tx hash', async () => {
    const withdrawAllMock = mock.method(VaultClient.prototype, 'withdraw_all', () =>
      Promise.resolve({ hash: FAKE_HASH, result: 500_0000000n }),
    );
    const withdrawMock = mock.method(VaultClient.prototype, 'withdraw', () =>
      Promise.resolve({ hash: FAKE_HASH }),
    );
    const hash = await setupWallet();
    const result = await handleWithdraw(hash, undefined, true);

    assert.equal(result.success, true);
    assert.equal(result.txHash, FAKE_HASH);
    assert.match(result.message, /all funds/i);
    assert.equal(withdrawAllMock.mock.callCount(), 1);
    assert.equal(withdrawMock.mock.callCount(), 0);
  });

  test('returns success=false when wallet does not exist', async () => {
    const result = await handleWithdraw('no-wallet', 50, false);
    assert.equal(result.success, false);
    assert.equal(result.txHash, '');
    assert.match(result.message, /wallet/i);
  });

  test('returns success=false and a friendly message when the contract rejects the withdrawal', async () => {
    mock.method(VaultClient.prototype, 'withdraw', () =>
      Promise.reject(new VaultError(VaultErrorCode.InsufficientShares, 'Vault contract error 8')),
    );
    const hash = await setupWallet();
    const result = await handleWithdraw(hash, 50, false);

    assert.equal(result.success, false);
    assert.equal(result.txHash, '');
    assert.match(result.message, /enough/i);
    assert.doesNotMatch(result.message, /^8$|VaultError|contract error/i);
  });

  test('returns success=false when the transaction never confirms (no hash)', async () => {
    mock.method(VaultClient.prototype, 'withdraw', () => Promise.resolve({ hash: undefined }));
    const hash = await setupWallet();
    const result = await handleWithdraw(hash, 50, false);

    assert.equal(result.success, false);
    assert.equal(result.txHash, '');
  });
});
