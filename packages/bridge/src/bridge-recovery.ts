/**
 * Startup reconciliation for durable bridge transfers (#850).
 *
 * A bridge process restart can land between any two workflow stages. This
 * pass walks every non-terminal durable record, asks both chains what actually
 * happened, and decides deterministically whether to resume the transfer,
 * mark it complete, or leave it retryable behind a bounded backoff.
 *
 * Invariants:
 * - A transfer the destination already confirmed is never submitted again.
 * - A transfer whose bridge message is in flight is polled, never resubmitted.
 * - Terminal records stay queryable and are never retried.
 * - Unknown external states stay retryable with an exponentially bounded,
 *   capped backoff instead of a hot loop.
 */

import pino from "pino";
import { BridgeManager, backoffDelayMs } from "./bridge-manager";
import { BridgeStore, isTerminalStatus } from "./bridge-store";
import {
  BridgeStatus,
  BridgeTransferPatch,
  StoredBridgeTransfer,
  TransferStage,
} from "./types";
import { redactTransfer } from "./redaction";

/** What reconciliation decided to do with one record. */
export type ReconciliationAction =
  /** Nothing to do: the record is already terminal. */
  | "skipped_terminal"
  /** Re-attempted an external step that had not been performed yet. */
  | "resumed"
  /** The destination chain confirmed the transfer during this pass. */
  | "confirmed"
  /** Marked failed: the external side reported failure or retries ran out. */
  | "failed"
  /** Left retryable; polled or observed, external state still unknown. */
  | "awaiting_external"
  /** Backoff window has not elapsed yet - untouched. */
  | "backoff";

/** Structured outcome for a single record (#850 acceptance). */
export interface ReconciliationResult {
  transferId: string;
  stage: TransferStage;
  statusBefore: BridgeStatus;
  statusAfter: BridgeStatus;
  action: ReconciliationAction;
  /** Machine-readable reason, e.g. `destination_executed`, `backoff_active`. */
  reason: string;
  /** Coarse error class (`timeout`, `network`, `rate_limited`, ...). */
  errorClass?: string;
  /** Timestamp at which a deferred record may be retried. */
  retryAt?: number;
}

/** Aggregate counts by terminal state and error class. */
export interface ReconciliationSummary {
  scanned: number;
  byAction: Record<ReconciliationAction, number>;
  byStatus: Record<string, number>;
  byErrorClass: Record<string, number>;
  results: ReconciliationResult[];
}

export interface TransferReconcilerOptions {
  /** Records inspected per pass. Older records are processed first. */
  maxRecords?: number;
  /** When false, the pass is read-only (used by dry-run tooling). */
  apply?: boolean;
}

const DEFAULT_MAX_RECORDS = 200;

function emptyByAction(): Record<ReconciliationAction, number> {
  return {
    skipped_terminal: 0,
    resumed: 0,
    confirmed: 0,
    failed: 0,
    awaiting_external: 0,
    backoff: 0,
  };
}

export class TransferReconciler {
  private logger = pino();

  constructor(
    private manager: BridgeManager,
    private store: BridgeStore,
    private options: TransferReconcilerOptions = {},
  ) {}

  /**
   * Scans durable non-terminal records and reconciles each one.
   */
  async reconcileOnStartup(): Promise<ReconciliationSummary> {
    const maxRecords = this.options.maxRecords ?? DEFAULT_MAX_RECORDS;
    const records = await this.store.getNonTerminal();
    const ordered = [...records].sort((a, b) => a.createdAt - b.createdAt);

    const summary: ReconciliationSummary = {
      scanned: 0,
      byAction: emptyByAction(),
      byStatus: {},
      byErrorClass: {},
      results: [],
    };

    for (const record of ordered) {
      if (summary.results.length >= maxRecords) {
        this.logger.warn(
          { remaining: ordered.length - summary.results.length },
          "Reconciliation batch limit reached; remaining records deferred to the next pass",
        );
        break;
      }

      const result = await this.reconcileOne(record);
      summary.scanned += 1;
      summary.results.push(result);
      summary.byAction[result.action] += 1;
      summary.byStatus[result.statusAfter] =
        (summary.byStatus[result.statusAfter] ?? 0) + 1;
      if (result.errorClass) {
        summary.byErrorClass[result.errorClass] =
          (summary.byErrorClass[result.errorClass] ?? 0) + 1;
      }
    }

    this.logger.info(
      {
        scanned: summary.scanned,
        byAction: summary.byAction,
        byStatus: summary.byStatus,
        byErrorClass: summary.byErrorClass,
      },
      "Bridge reconciliation pass complete",
    );

    return summary;
  }

  /**
   * Reconciles a single durable record.
   */
  async reconcileOne(
    record: StoredBridgeTransfer,
  ): Promise<ReconciliationResult> {
    const statusBefore = record.status;

    // Terminal records are queryable but never retried.
    if (isTerminalStatus(statusBefore)) {
      return this.result(record, "skipped_terminal", statusBefore, "terminal_status");
    }

    // The manager's in-memory operations (poll, resume) need the durable
    // record; after a restart the mirror starts empty.
    this.manager.attachDurableRecord(record);

    // A retry scheduled by an earlier pass is not due yet.
    if (record.nextAttemptAt !== undefined && record.nextAttemptAt > Date.now()) {
      return this.result(record, "backoff", statusBefore, "backoff_active", {
        retryAt: record.nextAttemptAt,
      });
    }

    // Both chains are queried before any decision is taken.
    const external = await this.manager.probeExternalState(record);

    // The destination already executed: adopt it, never resubmit.
    if (
      external.destinationChain === "found" &&
      external.axelarStatus === "executed" &&
      this.hasRequiredDepth(record, external.confirmationDepth)
    ) {
      await this.apply(record, {
        status: "confirmed",
        stage: "confirmed",
        destinationTxHash: external.destinationTxHash,
        lastError: undefined,
        nextAttemptAt: undefined,
        lastReconciledAt: Date.now(),
      });
      return this.result(record, "confirmed", statusBefore, "destination_executed");
    }

    // The bridge message is in flight: poll, do not resubmit.
    if (record.stage === "submitted" || record.bridgeTxHash) {
      const statusAfter = await this.poll(record);
      if (statusAfter === "confirmed") {
        return this.result(record, "confirmed", statusBefore, "destination_executed");
      }
      if (statusAfter === "failed") {
        return this.result(record, "failed", statusBefore, "bridge_reported_failure", {
          errorClass: "bridge_failure",
        });
      }
      // Polling may have recorded a fresh error and backoff deadline; read the
      // durable copy back so the summary reflects what a restart would see.
      const fresh = (await this.store.get(record.id)) ?? record;
      return this.result(record, "awaiting_external", statusAfter, "in_flight", {
        retryAt: fresh.nextAttemptAt ?? this.backoffFor(fresh),
        errorClass: fresh.lastError
          ? this.manager.classifyError(fresh.lastError)
          : undefined,
      });
    }

    // Nothing was submitted to the bridge yet: resume the workflow, but only
    // once the source-chain transaction is known to exist. Resubmitting on top
    // of an unknown source state is exactly what must not happen (#850).
    if (!record.sourceChainTxHash) {
      await this.apply(record, { lastReconciledAt: Date.now() });
      return this.result(record, "awaiting_external", statusBefore, "awaiting_source_hash", {
        retryAt: this.backoffFor(record),
      });
    }

    if (external.sourceChain === "missing") {
      await this.apply(record, {
        status: "failed",
        lastError: "Source-chain transaction not found",
        nextAttemptAt: undefined,
        lastReconciledAt: Date.now(),
      });
      return this.result(record, "failed", statusBefore, "source_transaction_missing", {
        errorClass: "not_found",
      });
    }

    if (external.sourceChain !== "found") {
      await this.apply(record, {
        attemptCount: record.attemptCount + 1,
        lastError: "Source-chain state unknown",
        nextAttemptAt: this.backoffFor(record),
        lastReconciledAt: Date.now(),
      });
      return this.result(record, "awaiting_external", statusBefore, "source_chain_unknown", {
        errorClass: "unknown",
        retryAt: this.backoffFor(record),
      });
    }

    if (record.retriesRemaining <= 0) {
      await this.apply(record, {
        status: "failed",
        lastError: "No retries remaining after restart",
        lastReconciledAt: Date.now(),
      });
      return this.result(record, "failed", statusBefore, "retries_exhausted", {
        errorClass: "retries_exhausted",
      });
    }

    try {
      // Resumes the workflow. The manager refuses to submit a record that has
      // already been submitted, so a duplicate is impossible even if the pass
      // is retried.
      await this.manager.executeAxelarTransfer(record.id, record.sourceChainTxHash);
      return this.result(record, "resumed", "confirming", "resubmitted_from_observed");
    } catch (error) {
      const errorClass = this.manager.classifyError(error);
      const fresh = (await this.store.get(record.id)) ?? record;
      this.logger.warn(
        { transferId: record.id, errorClass },
        "Reconciliation could not resume transfer submission",
      );
      return this.result(record, "awaiting_external", record.status, "resume_failed", {
        errorClass,
        retryAt: fresh.nextAttemptAt,
      });
    }
  }

  private hasRequiredDepth(
    record: StoredBridgeTransfer,
    depth: number | undefined,
  ): boolean {
    if (depth === undefined) {
      return false;
    }
    const required =
      record.requiredConfirmationDepth ??
      this.manager.getRequiredConfirmationDepth(record.destinationChain);
    return depth >= required;
  }

  private async poll(record: StoredBridgeTransfer): Promise<BridgeStatus> {
    return this.manager.pollTransferStatus(record.id);
  }

  private backoffFor(record: StoredBridgeTransfer): number | undefined {
    return Date.now() + backoffDelayMs(record.attemptCount);
  }

  private async apply(
    record: StoredBridgeTransfer,
    patch: BridgeTransferPatch,
  ): Promise<void> {
    if (this.options.apply === false) {
      return;
    }
    await this.store.update(record.id, patch);
    this.logger.debug(
      { transfer: redactTransfer(record), patch: { ...patch, lastError: patch.lastError ? "[redacted]" : undefined } },
      "Reconciliation applied durable patch",
    );
  }

  private result(
    record: StoredBridgeTransfer,
    action: ReconciliationAction,
    statusAfter: BridgeStatus,
    reason: string,
    extra: { errorClass?: string; retryAt?: number } = {},
  ): ReconciliationResult {
    return {
      transferId: record.id,
      stage: record.stage,
      statusBefore: record.status,
      statusAfter,
      action,
      reason,
      errorClass: extra.errorClass,
      retryAt: extra.retryAt,
    };
  }
}
