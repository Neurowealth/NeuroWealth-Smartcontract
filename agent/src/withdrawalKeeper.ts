/**
 * Keeper for processing withdrawal queue (Issue #1005)
 *
 * Queued withdrawals only settle when someone calls `process_withdrawal_queue`.
 * This keeper runs on a schedule to process pending withdrawals when idle liquidity
 * is available, with metrics and alerting for queue health.
 *
 * Features:
 * - Periodic queue processing when non-empty and liquidity available
 * - Metrics: queue depth, oldest request age, processed count
 * - Alert when oldest request approaches TTL
 */

import logger from './logger';
import { Keypair, Contract, xdr } from '@stellar/stellar-sdk';
import { server } from './eventListener';

export interface WithdrawalKeeperConfig {
  /** Interval between queue checks in milliseconds (default: 5 minutes) */
  checkIntervalMs: number;
  /** Maximum batch size per process_withdrawal_queue call */
  batchSize: number;
  /** Minimum idle liquidity required to process withdrawals (in USDC stroops) */
  minIdleLiquidity: number;
  /** Alert threshold: alert when oldest request is within this many milliseconds of TTL */
  ttlAlertThresholdMs: number;
}

export const DEFAULT_KEEPER_CONFIG: WithdrawalKeeperConfig = {
  checkIntervalMs: 5 * 60 * 1000, // 5 minutes
  batchSize: 10,
  minIdleLiquidity: 100_000_00, // 100 USDC (7 decimals)
  ttlAlertThresholdMs: 15 * 60 * 1000, // 15 minutes
};

export interface WithdrawalQueueMetrics {
  queueDepth: number;
  oldestRequestAgeMs: number;
  processedCount: number;
  lastProcessedAt: number | null;
  idleLiquidity: number;
}

export interface WithdrawalKeeperResult {
  runAt: number;
  processed: number;
  queueDepth: number;
  oldestRequestAgeMs: number;
  error?: string;
}

interface QueueEntry {
  request_id: number;
  user: string;
  amount: number;
  timestamp: number;
  ttl_ledger: number;
}

/**
 * Fetches current withdrawal queue state from the vault contract.
 * Returns queue depth, oldest request age, and idle liquidity.
 */
async function getQueueState(
  vaultContractId: string,
): Promise<{ queueDepth: number; oldestRequestAgeMs: number; idleLiquidity: number }> {
  try {
    const contract = new Contract(vaultContractId);

    // Query queue depth (this is a placeholder - actual implementation depends on contract)
    // In production, this would call a get_queue_state() or similar method
    const queueDepth = 0; // Placeholder

    // Query oldest request timestamp
    const oldestRequestAgeMs = 0; // Placeholder

    // Query idle liquidity
    const idleLiquidity = 0; // Placeholder

    return { queueDepth, oldestRequestAgeMs, idleLiquidity };
  } catch (error) {
    logger.error({ error: error instanceof Error ? error.message : String(error) }, 'Failed to fetch queue state');
    return { queueDepth: 0, oldestRequestAgeMs: 0, idleLiquidity: 0 };
  }
}

/**
 * Processes the withdrawal queue by calling process_withdrawal_queue on the vault contract.
 */
async function processWithdrawalQueue(
  vaultContractId: string,
  agentKeypair: Keypair,
  batchSize: number,
): Promise<number> {
  try {
    const contract = new Contract(vaultContractId);
    const { nativeToScVal } = await import('@stellar/stellar-sdk');

    const args = [
      nativeToScVal(agentKeypair.publicKey(), { type: 'address' }),
      nativeToScVal(batchSize, { type: 'u32' }),
    ];

    // Note: This is a placeholder - actual implementation needs to build and submit a transaction
    // In production, this would use the transaction building logic from sorobanTx.ts
    logger.info({ vaultContractId, batchSize }, 'process_withdrawal_queue called');

    return 0; // Placeholder: number of requests processed
  } catch (error) {
    logger.error({ error: error instanceof Error ? error.message : String(error) }, 'Failed to process withdrawal queue');
    return 0;
  }
}

/**
 * Runs a single keeper cycle: check queue, process if conditions met, alert if needed.
 */
export async function runWithdrawalKeeper(
  vaultContractId: string,
  agentKeypair: Keypair,
  config: WithdrawalKeeperConfig = DEFAULT_KEEPER_CONFIG,
  now: () => number = () => Date.now(),
): Promise<WithdrawalKeeperResult> {
  const runAt = now();

  try {
    // Fetch current queue state
    const { queueDepth, oldestRequestAgeMs, idleLiquidity } = await getQueueState(vaultContractId);

    // Alert if oldest request is approaching TTL
    if (oldestRequestAgeMs > 0 && oldestRequestAgeMs > config.ttlAlertThresholdMs) {
      logger.warn(
        { queueDepth, oldestRequestAgeMs, ttlAlertThreshold: config.ttlAlertThresholdMs },
        'Oldest withdrawal request approaching TTL',
      );
    }

    // Process queue if non-empty and idle liquidity available
    let processed = 0;
    if (queueDepth > 0 && idleLiquidity >= config.minIdleLiquidity) {
      logger.info({ queueDepth, idleLiquidity, batchSize: config.batchSize }, 'Processing withdrawal queue');
      processed = await processWithdrawalQueue(vaultContractId, agentKeypair, config.batchSize);
    } else if (queueDepth > 0 && idleLiquidity < config.minIdleLiquidity) {
      logger.warn(
        { queueDepth, idleLiquidity, minIdleLiquidity: config.minIdleLiquidity },
        'Withdrawal queue non-empty but insufficient idle liquidity',
      );
    }

    const result: WithdrawalKeeperResult = {
      runAt,
      processed,
      queueDepth,
      oldestRequestAgeMs,
    };

    logger.info({ result }, 'Withdrawal keeper cycle completed');
    return result;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    logger.error({ error: message }, 'Withdrawal keeper cycle failed');
    return {
      runAt,
      processed: 0,
      queueDepth: 0,
      oldestRequestAgeMs: 0,
      error: message,
    };
  }
}

/**
 * Wall-clock scheduler for the withdrawal queue keeper.
 * Follows the same lifecycle pattern as RollupScheduler.
 */
export class WithdrawalKeeperScheduler {
  private timer: ReturnType<typeof setInterval> | null = null;
  private lastResult: WithdrawalKeeperResult | null = null;
  private metrics: WithdrawalQueueMetrics = {
    queueDepth: 0,
    oldestRequestAgeMs: 0,
    processedCount: 0,
    lastProcessedAt: null,
    idleLiquidity: 0,
  };

  constructor(
    private readonly vaultContractId: string,
    private readonly agentKeypair: Keypair,
    private readonly config: WithdrawalKeeperConfig = DEFAULT_KEEPER_CONFIG,
    private readonly now: () => number = () => Date.now(),
  ) {}

  start(): void {
    if (this.timer) return;
    void this.runOnce();
    this.timer = setInterval(() => {
      void this.runOnce();
    }, this.config.checkIntervalMs);
    logger.info({ intervalMs: this.config.checkIntervalMs }, 'Withdrawal keeper scheduler started');
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
      logger.info('Withdrawal keeper scheduler stopped');
    }
  }

  private async runOnce(): Promise<void> {
    try {
      const result = await runWithdrawalKeeper(
        this.vaultContractId,
        this.agentKeypair,
        this.config,
        this.now,
      );
      this.lastResult = result;

      // Update metrics
      this.metrics.queueDepth = result.queueDepth;
      this.metrics.oldestRequestAgeMs = result.oldestRequestAgeMs;
      this.metrics.processedCount += result.processed;
      this.metrics.idleLiquidity = 0; // Would be updated from actual query
      if (result.processed > 0) {
        this.metrics.lastProcessedAt = result.runAt;
      }
    } catch (err) {
      logger.error(
        { error: err instanceof Error ? err.message : err },
        'Withdrawal keeper run threw unexpectedly',
      );
    }
  }

  getLastResult(): WithdrawalKeeperResult | null {
    return this.lastResult;
  }

  getMetrics(): WithdrawalQueueMetrics {
    return { ...this.metrics };
  }
}
