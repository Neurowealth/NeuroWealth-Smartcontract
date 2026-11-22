/**
 * Keeper for processing withdrawal queue (Issue #1005)
 *
 * Queued withdrawals only settle when someone calls `process_withdrawal_queue`.
 * This keeper runs on a schedule to process pending withdrawals when idle liquidity
 * is available, with metrics and alerting for queue health.
 *
 * Features:
 * - Periodic queue processing when non-empty and liquidity available
 * - Metrics: queue depth, oldest request age, processed count, idle liquidity
 * - Alert when oldest request approaches TTL
 */

import logger from './logger';
import { server } from './eventListener';
import { submitProcessWithdrawalQueueTx } from './sorobanTx';
import {
  Account,
  Contract,
  Keypair,
  Networks,
  TransactionBuilder,
  rpc,
  scValToNative,
  xdr,
} from '@stellar/stellar-sdk';

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
  idleLiquidity: number;
  error?: string;
}

const networkPassphrase = process.env.SOROBAN_NETWORK_PASSPHRASE || Networks.TESTNET;
const TX_INCLUSION_FEE = '100';
const TX_TIMEOUT_SECONDS = 60;
const QUEUE_EVENTS_PAGE_LIMIT = 500;
const MAX_QUEUE_EVENT_PAGES = 10;

/**
 * Reads the vault's idle liquidity via a read-only `get_idle_balance`
 * simulation. Returns 0 when the value is unavailable so the keeper never
 * gates on fabricated state.
 */
async function readIdleLiquidity(vaultContractId: string, sourcePublicKey: string | undefined): Promise<number> {
  if (!vaultContractId || !sourcePublicKey) return 0;
  try {
    const tx = new TransactionBuilder(new Account(sourcePublicKey, '0'), {
      fee: TX_INCLUSION_FEE,
      networkPassphrase,
    })
      .addOperation(new Contract(vaultContractId).call('get_idle_balance'))
      .setTimeout(TX_TIMEOUT_SECONDS)
      .build();
    const simulation = await server.simulateTransaction(tx);
    if (!rpc.Api.isSimulationSuccess(simulation) || !simulation.result?.retval) return 0;
    const value = Number(scValToNative(simulation.result.retval));
    return Number.isFinite(value) && value > 0 ? value : 0;
  } catch (error) {
    logger.debug({ error: error instanceof Error ? error.message : String(error) }, 'Failed to read vault idle liquidity');
    return 0;
  }
}

/** Converts an event topic/value ScVal to native, or returns the value as-is. */
function toNativeEvent(value: unknown): unknown {
  if (value instanceof xdr.ScVal) {
    try {
      return scValToNative(value);
    } catch {
      return null;
    }
  }
  return value;
}

/**
 * Reconstructs queue depth and the oldest pending request age from the
 * `wq_add` / `wq_cancel` / `wq_done` events (#757): every `wq_add` opens a
 * pending request and `wq_cancel` / `wq_done` close theirs.
 */
async function readQueueStateFromEvents(vaultContractId: string): Promise<{ depth: number; oldestAgeMs: number }> {
  const pending = new Map<number, number>();
  try {
    const filters: rpc.Api.EventFilter[] = [{ type: 'contract', contractIds: [vaultContractId] }];
    let cursor: string | undefined;
    for (let page = 0; page < MAX_QUEUE_EVENT_PAGES; page += 1) {
      const request: rpc.Api.GetEventsRequest = cursor
        ? { filters, cursor, limit: QUEUE_EVENTS_PAGE_LIMIT }
        : { filters, startLedger: 0, limit: QUEUE_EVENTS_PAGE_LIMIT };
      const response = await server.getEvents(request);
      for (const event of response.events) {
        const kind = toNativeEvent(event.topic[0]);
        if (kind !== 'wq_add' && kind !== 'wq_cancel' && kind !== 'wq_done') continue;
        const payload = toNativeEvent(event.value);
        if (!payload || typeof payload !== 'object' || Array.isArray(payload)) continue;
        const record = payload as Record<string, unknown>;
        const requestId = Number(record.request_id ?? 0);
        if (!Number.isFinite(requestId)) continue;
        if (kind === 'wq_add') {
          const timestamp = Number(record.timestamp ?? 0);
          if (Number.isFinite(timestamp)) pending.set(requestId, timestamp);
        } else {
          pending.delete(requestId);
        }
      }
      cursor = response.cursor || undefined;
      if (!cursor) break;
    }
  } catch (error) {
    logger.debug({ error: error instanceof Error ? error.message : String(error) }, 'Failed to read withdrawal queue events');
  }
  const nowSeconds = Math.floor(Date.now() / 1000);
  let oldestAgeMs = 0;
  for (const timestamp of pending.values()) {
    const ageMs = Math.max(0, (nowSeconds - timestamp) * 1000);
    oldestAgeMs = Math.max(oldestAgeMs, ageMs);
  }
  return { depth: pending.size, oldestAgeMs };
}

/**
 * Fetches current withdrawal queue state from the vault contract: queue
 * depth, oldest request age, and idle liquidity.
 */
async function getQueueState(
  vaultContractId: string,
  agentKeypair: Keypair,
): Promise<{ queueDepth: number; oldestRequestAgeMs: number; idleLiquidity: number }> {
  try {
    const sourcePublicKey = agentKeypair?.publicKey() || process.env.SOROBAN_PUBLIC_KEY;
    const [idleLiquidity, queueState] = await Promise.all([
      readIdleLiquidity(vaultContractId, sourcePublicKey),
      readQueueStateFromEvents(vaultContractId),
    ]);
    return { queueDepth: queueState.depth, oldestRequestAgeMs: queueState.oldestAgeMs, idleLiquidity };
  } catch (error) {
    logger.error({ error: error instanceof Error ? error.message : String(error) }, 'Failed to fetch queue state');
    return { queueDepth: 0, oldestRequestAgeMs: 0, idleLiquidity: 0 };
  }
}

/**
 * Processes the withdrawal queue by calling `process_withdrawal_queue` on the
 * vault contract with the agent's signing key. Returns the number of
 * requests the contract reports as processed.
 */
async function processWithdrawalQueue(
  vaultContractId: string,
  agentKeypair: Keypair,
  batchSize: number,
): Promise<number> {
  try {
    if (process.env.AGENT_EXECUTION_MODE === 'dry-run') {
      logger.info({ vaultContractId, batchSize }, 'process_withdrawal_queue skipped in dry-run mode');
      return 0;
    }
    const result = await submitProcessWithdrawalQueueTx(vaultContractId, agentKeypair, batchSize);
    if (result.status !== 'submitted' && result.status !== 'reconciling') {
      logger.warn(
        { status: result.status, rejectionReasons: result.rejectionReasons },
        'process_withdrawal_queue not submitted',
      );
      return 0;
    }
    if (!result.txHash) return 0;
    const txResp = await server.getTransaction(result.txHash);
    const retval = (txResp as rpc.Api.GetSuccessfulTransactionResponse).returnValue;
    const processed = retval ? Number(scValToNative(retval) ?? 0) : 0;
    logger.info({ vaultContractId, batchSize, processed }, 'process_withdrawal_queue submitted');
    return processed;
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
    const { queueDepth, oldestRequestAgeMs, idleLiquidity } = await getQueueState(vaultContractId, agentKeypair);

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
      idleLiquidity,
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
      idleLiquidity: 0,
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
      this.metrics.idleLiquidity = result.idleLiquidity;
      if (result.processed > 0) {
        this.metrics.lastProcessedAt = result.runAt;
      }
    } catch (err) {
      logger.error(
        { error: err instanceof Error ? err.message : String(err) },
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
