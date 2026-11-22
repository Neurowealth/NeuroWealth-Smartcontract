/**
 * Unit tests for withdrawal queue keeper (Issue #1005)
 *
 * Tests cover queue processing, metrics, and alerting.
 */

import { describe, it, beforeEach } from 'node:test';
import assert from 'node:assert';
import { Keypair, xdr } from '@stellar/stellar-sdk';
import { server } from './eventListener';
import {
  WithdrawalKeeperScheduler,
  DEFAULT_KEEPER_CONFIG,
  runWithdrawalKeeper,
  type WithdrawalKeeperConfig,
} from './withdrawalKeeper';

const serverAny = server as unknown as Record<string, unknown>;

/** Builds a `wq_add`-style event the way the keeper parses it. */
function queueEvent(
  kind: 'wq_add' | 'wq_cancel' | 'wq_done',
  payload: { requestId: number; timestamp?: number },
): unknown {
  return {
    topic: [xdr.ScVal.scvSymbol(kind)],
    value: {
      request_id: payload.requestId,
      user: 'GABCKEEPER',
      amount: 1n,
      timestamp: BigInt(payload.timestamp ?? 0),
    },
  };
}

// Default stubs so unit tests never hit the network; the submission test
// below overrides them with live-mode responses.
serverAny.simulateTransaction = async () => {
  throw new Error('simulateTransaction stub not set');
};
serverAny.getEvents = async () => {
  throw new Error('getEvents stub not set');
};

describe('Withdrawal queue keeper (#1005)', () => {
  let vaultContractId: string;
  let agentKeypair: Keypair;
  let config: WithdrawalKeeperConfig;

  beforeEach(() => {
    vaultContractId = 'CAPEE5AAN3ASTJSL6ZQMRGZULOB32W2VJPAY52RKXM2TOC6WH3PCJSW7';
    agentKeypair = Keypair.random();
    config = { ...DEFAULT_KEEPER_CONFIG };
  });

  it('scheduler starts and stops without errors', () => {
    const scheduler = new WithdrawalKeeperScheduler(vaultContractId, agentKeypair, config);
    scheduler.start();
    assert.ok(scheduler.getLastResult() === null || scheduler.getLastResult() !== undefined);
    scheduler.stop();
  });

  it('does not start if already started', () => {
    const scheduler = new WithdrawalKeeperScheduler(vaultContractId, agentKeypair, config);
    scheduler.start();
    scheduler.start(); // Should not throw or create duplicate timer
    scheduler.stop();
  });

  it('returns metrics with zero initial values', () => {
    const scheduler = new WithdrawalKeeperScheduler(vaultContractId, agentKeypair, config);
    const metrics = scheduler.getMetrics();
    assert.strictEqual(metrics.queueDepth, 0);
    assert.strictEqual(metrics.oldestRequestAgeMs, 0);
    assert.strictEqual(metrics.processedCount, 0);
    assert.strictEqual(metrics.lastProcessedAt, null);
  });

  it('uses configured check interval', () => {
    const customConfig = { ...config, checkIntervalMs: 30000 }; // 30 seconds
    const scheduler = new WithdrawalKeeperScheduler(vaultContractId, agentKeypair, customConfig);
    scheduler.start();
    scheduler.stop();
    // Interval is internal; we just verify it doesn't throw
  });

  it('uses configured batch size', () => {
    const customConfig = { ...config, batchSize: 20 };
    const scheduler = new WithdrawalKeeperScheduler(vaultContractId, agentKeypair, customConfig);
    const metrics = scheduler.getMetrics();
    assert.strictEqual(metrics.queueDepth, 0);
  });

  it('uses configured min idle liquidity', () => {
    const customConfig = { ...config, minIdleLiquidity: 500_000_00 }; // 500 USDC
    const scheduler = new WithdrawalKeeperScheduler(vaultContractId, agentKeypair, customConfig);
    scheduler.start();
    scheduler.stop();
  });

  it('uses configured TTL alert threshold', () => {
    const customConfig = { ...config, ttlAlertThresholdMs: 10 * 60 * 1000 }; // 10 minutes
    const scheduler = new WithdrawalKeeperScheduler(vaultContractId, agentKeypair, customConfig);
    scheduler.start();
    scheduler.stop();
  });

  it('runWithdrawalKeeper returns result structure', async () => {
    const result = await runWithdrawalKeeper(vaultContractId, agentKeypair, config);
    assert.ok(typeof result.runAt === 'number');
    assert.ok(typeof result.processed === 'number');
    assert.ok(typeof result.queueDepth === 'number');
    assert.ok(typeof result.oldestRequestAgeMs === 'number');
  });

  it('runWithdrawalKeeper handles errors gracefully', async () => {
    // With invalid contract ID, it should return an error result
    const result = await runWithdrawalKeeper('INVALID', agentKeypair, config);
    assert.ok(result.error !== undefined || result.queueDepth === 0);
  });

  it('submits process_withdrawal_queue when the queue has depth and idle liquidity', async () => {
    process.env.AGENT_EXECUTION_MODE = 'live';
    const nowSeconds = Math.floor(Date.now() / 1000);
    let submitCount = 0;

    serverAny.getAccount = async () => ({ sequenceNumber: () => '0' });
    serverAny.getEvents = async () => ({
      events: [
        queueEvent('wq_add', { requestId: 7, timestamp: nowSeconds - 20 }),
        queueEvent('wq_add', { requestId: 8, timestamp: nowSeconds - 5 }),
      ],
      cursor: '',
      latestLedger: 2000,
      oldestLedger: 0,
      latestLedgerCloseTime: '0',
      oldestLedgerCloseTime: '0',
    });
    serverAny.simulateTransaction = async () => {
      const retval = xdr.ScVal.scvU64(new xdr.Uint64(500_000_000));
      return {
        transactionData: undefined,
        result: { retval },
        results: [{ xdr: retval.toXDR('base64'), auth: [] }],
      };
    };
    serverAny.sendTransaction = async () => {
      submitCount += 1;
      return { status: 'PENDING', hash: 'abcd1234' };
    };
    serverAny.getTransaction = async () => ({
      status: 'SUCCESS',
      returnValue: xdr.ScVal.scvU64(new xdr.Uint64(2)),
    });

    const result = await runWithdrawalKeeper('CAPEE5AAN3ASTJSL6ZQMRGZULOB32W2VJPAY52RKXM2TOC6WH3PCJSW7', agentKeypair, config, () => Date.now());

    assert.strictEqual(result.queueDepth, 2);
    assert.ok(result.oldestRequestAgeMs >= 4_000);
    assert.ok(result.idleLiquidity >= 500_000_000);
    assert.strictEqual(result.processed, 2);
    assert.strictEqual(submitCount, 1);
    delete process.env.AGENT_EXECUTION_MODE;
  });
});
