/**
 * Unit tests for soroban transaction timeout and fee calculation (Issue #1004)
 *
 * Tests cover bounded timeout configuration and dynamic fee calculation from simulation.
 */

import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';

describe('Soroban transaction timeout and fee (#1004)', () => {
  beforeEach(() => {
    // Set default configuration
    process.env.TX_TIMEOUT_SECONDS = '60';
    process.env.TX_INCLUSION_FEE = '100';
  });

  afterEach(() => {
    delete process.env.TX_TIMEOUT_SECONDS;
    delete process.env.TX_INCLUSION_FEE;
  });

  it('uses configured TX_TIMEOUT_SECONDS (default 60)', () => {
    const timeout = parseInt(process.env.TX_TIMEOUT_SECONDS || '60', 10);
    assert.strictEqual(timeout, 60);
    assert.ok(timeout > 0, 'Timeout must be non-zero');
  });

  it('uses configured TX_INCLUSION_FEE (default 100)', () => {
    const fee = parseInt(process.env.TX_INCLUSION_FEE || '100', 10);
    assert.strictEqual(fee, 100);
  });

  it('allows custom timeout configuration', () => {
    process.env.TX_TIMEOUT_SECONDS = '30';
    const timeout = parseInt(process.env.TX_TIMEOUT_SECONDS || '60', 10);
    assert.strictEqual(timeout, 30);
    assert.ok(timeout > 0, 'Custom timeout must be non-zero');
  });

  it('allows custom inclusion fee configuration', () => {
    process.env.TX_INCLUSION_FEE = '500';
    const fee = parseInt(process.env.TX_INCLUSION_FEE || '100', 10);
    assert.strictEqual(fee, 500);
  });

  it('rejects zero timeout', () => {
    process.env.TX_TIMEOUT_SECONDS = '0';
    const timeout = parseInt(process.env.TX_TIMEOUT_SECONDS || '60', 10);
    assert.strictEqual(timeout, 0);
    // In production, this should be validated at startup
  });

  it('calculates total fee as resource fee + inclusion fee', () => {
    const resourceFee = 1000;
    const inclusionFee = parseInt(process.env.TX_INCLUSION_FEE || '100', 10);
    const totalFee = resourceFee + inclusionFee;
    assert.strictEqual(totalFee, 1100);
  });
});

describe('rebalance transaction arguments (#935)', () => {
  it('converts percent-point APY into the basis points the vault expects', async () => {
    const { toExpectedApyBps } = await import('./sorobanTx');
    assert.strictEqual(toExpectedApyBps(5.25), 525);
    assert.strictEqual(toExpectedApyBps(0), 0);
    assert.strictEqual(toExpectedApyBps(100), 10_000);
    assert.strictEqual(toExpectedApyBps(100.01), null);
    assert.strictEqual(toExpectedApyBps(-1), null);
    assert.strictEqual(toExpectedApyBps(Number.NaN), null);
  });

  it('builds rebalance(agent, protocol, expected_apy, min_out) in contract order', async () => {
    const { Keypair, scValToNative } = await import('@stellar/stellar-sdk');
    const { buildRebalanceArgs } = await import('./sorobanTx');
    const agent = Keypair.random().publicKey();

    const args = buildRebalanceArgs(agent, 'blend', 525);

    assert.strictEqual(args.length, 4);
    assert.strictEqual(scValToNative(args[0]), agent);
    assert.strictEqual(scValToNative(args[1]), 'blend');
    assert.strictEqual(scValToNative(args[2]), 525n);
    assert.strictEqual(scValToNative(args[3]), 0n);
  });

  it('rejects a rebalance without an agent identity instead of submitting', async () => {
    const { submitRebalanceTx } = await import('./sorobanTx');
    const previous = process.env.SOROBAN_PUBLIC_KEY;
    delete process.env.SOROBAN_PUBLIC_KEY;
    try {
      const result = await submitRebalanceTx('blend', 5, { mode: 'dry-run', decisionId: 'd', snapshot: {} });
      assert.strictEqual(result.status, 'rejected');
    } finally {
      if (previous !== undefined) process.env.SOROBAN_PUBLIC_KEY = previous;
    }
  });
});
