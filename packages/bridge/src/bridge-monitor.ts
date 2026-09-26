/**
 * Monitors pending bridge transfers and handles retries/timeouts
 */

import pino from "pino";
import { BridgeManager } from "./bridge-manager";
import { BridgeStore } from "./bridge-store";
import { StoredBridgeTransfer } from "./types";

const BRIDGE_TIMEOUT_MS = 30 * 60 * 1000; // 30 minutes
const POLL_INTERVAL_MS = 60 * 1000; // 1 minute
const RETRY_BACKOFF_MS = 5 * 60 * 1000; // 5 minutes

export class BridgeMonitor {
  private logger = pino();
  private monitoringInterval: NodeJS.Timeout | null = null;

  constructor(
    private bridgeManager: BridgeManager,
    private store: BridgeStore,
  ) {}

  async start(): Promise<void> {
    this.logger.info("Starting bridge monitor");

    // #848 - Rebuild the in-memory view from durable state first: after a
    // restart the manager only knows about transfers it can read back from
    // the store, otherwise the first polling pass would see nothing to do.
    await this.bridgeManager.loadDurableState();

    // Poll every minute
    this.monitoringInterval = setInterval(() => {
      this.checkPendingTransfers().catch((error) => {
        this.logger.error({ error }, "Bridge monitor error");
      });
    }, POLL_INTERVAL_MS);

    // Initial check
    await this.checkPendingTransfers();
  }

  async stop(): Promise<void> {
    if (this.monitoringInterval) {
      clearInterval(this.monitoringInterval);
      this.monitoringInterval = null;
    }
    this.logger.info("Bridge monitor stopped");
  }

  private async checkPendingTransfers(): Promise<void> {
    try {
      const pending = await this.store.getPending();

      for (const transfer of pending) {
        await this.processTransfer(transfer);
      }
    } catch (error) {
      this.logger.error({ error }, "Failed to check pending transfers");
    }
  }

  private async processTransfer(transfer: StoredBridgeTransfer): Promise<void> {
    const now = Date.now();
    const elapsedTime = now - transfer.createdAt;

    // Check for timeout
    if (elapsedTime > BRIDGE_TIMEOUT_MS) {
      this.logger.warn(
        { transferId: transfer.id, elapsedTime },
        "Bridge transfer timeout",
      );
      await this.store.update(transfer.id, {
        status: "failed",
        errorMessage: "Bridge transfer timeout after 30 minutes",
      });
      return;
    }

    // Poll status if in confirming state
    if (transfer.status === "confirming" && transfer.bridgeTxHash) {
      try {
        const status = await this.bridgeManager.pollTransferStatus(transfer.id);
        await this.store.update(transfer.id, { status });

        this.logger.info(
          { transferId: transfer.id, status },
          "Transfer status updated",
        );
      } catch (error) {
        this.logger.error(
          { error, transferId: transfer.id },
          "Failed to poll status",
        );
      }
    }

    // Retry if pending and ready for retry
    if (
      transfer.status === "failed" &&
      transfer.retriesRemaining > 0 &&
      (!transfer.lastRetryTime ||
        now - transfer.lastRetryTime > RETRY_BACKOFF_MS)
    ) {
      this.logger.info(
        {
          transferId: transfer.id,
          retriesRemaining: transfer.retriesRemaining - 1,
        },
        "Retrying failed bridge transfer",
      );

      try {
        await this.bridgeManager.retryTransfer(transfer.id);
        await this.store.update(transfer.id, {
          status: "pending",
          lastRetryTime: now,
          retriesRemaining: transfer.retriesRemaining - 1,
        });
      } catch (error) {
        this.logger.error({ error, transferId: transfer.id }, "Retry failed");
      }
    }
  }

  /**
   * Get bridge health status
   */
  async getHealthStatus(): Promise<{
    isHealthy: boolean;
    pendingTransfers: number;
    failedTransfers: number;
    averageConfirmationTime: number;
  }> {
    const pending = await this.store.getPending();
    const stats = this.bridgeManager.getStatistics();

    // Calculate average confirmation time
    let totalTime = 0;
    let confirmedCount = 0;

    for (const transfer of Array.from(
      (this.store as unknown as { transfers?: Map<string, StoredBridgeTransfer> })
        .transfers?.values() ?? [],
    ) as StoredBridgeTransfer[]) {
      if (transfer.status === "confirmed" && transfer.estimatedArrivalTime) {
        totalTime += transfer.estimatedArrivalTime - transfer.createdAt;
        confirmedCount++;
      }
    }

    const averageTime = confirmedCount > 0 ? totalTime / confirmedCount : 0;
    const isHealthy = stats.failedTransfers < stats.totalTransfers * 0.1; // <10% failure rate

    return {
      isHealthy,
      pendingTransfers: pending.length,
      failedTransfers: stats.failedTransfers,
      averageConfirmationTime: averageTime,
    };
  }
}
