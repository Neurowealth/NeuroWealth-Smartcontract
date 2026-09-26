/**
 * PostgreSQL-backed BridgeStore implementation (#809)
 *
 * Uses the schema.sql tables for durable storage of bridge transfers.
 * Replaces InMemoryBridgeStore for production deployments.
 *
 * The workflow columns added for #848 (`stage`, `attempt_count`,
 * `last_error`, `next_attempt_at`, `last_reconciled_at`) are what a restarted
 * process reads back to resume a transfer; `getNonTerminal` feeds the startup
 * reconciliation pass from #850.
 */

import pino from "pino";
import { Pool, PoolClient } from "pg";
import { BridgeStore, TERMINAL_STATUSES } from "./bridge-store";
import { sanitizeErrorMessage } from "./redaction";
import { BridgeTransferPatch, StoredBridgeTransfer } from "./types";

export interface PostgresBridgeStoreConfig {
  connectionString: string;
  maxConnections?: number;
  /** Pool idle timeout in milliseconds. */
  idleTimeoutMs?: number;
}

/** Columns holding a timestamp, mapped from the millisecond values we use. */
const TIMESTAMP_PATCH_COLUMNS: ReadonlyArray<keyof BridgeTransferPatch> = [
  "lastRetryTime",
  "nextAttemptAt",
  "lastReconciledAt",
];

/** Columns that map one to one onto a SQL column of the same meaning. */
const SCALAR_PATCH_COLUMNS: ReadonlyArray<{
  field: keyof BridgeTransferPatch;
  column: string;
}> = [
  { field: "status", column: "status" },
  { field: "direction", column: "direction" },
  { field: "sourceChain", column: "source_chain" },
  { field: "destinationChain", column: "destination_chain" },
  { field: "user", column: "user_address" },
  { field: "amount", column: "amount" },
  { field: "bridgeFee", column: "bridge_fee" },
  { field: "netAmount", column: "net_amount" },
  { field: "sourceChainTxHash", column: "source_chain_tx_hash" },
  { field: "bridgeTxHash", column: "bridge_tx_hash" },
  { field: "destinationTxHash", column: "destination_tx_hash" },
  { field: "estimatedArrivalTime", column: "estimated_arrival_time" },
  { field: "retriesRemaining", column: "retries_remaining" },
  { field: "stage", column: "stage" },
  { field: "attemptCount", column: "attempt_count" },
  { field: "lastError", column: "last_error" },
  { field: "errorMessage", column: "error_message" },
  { field: "idempotencyKey", column: "idempotency_key" },
  { field: "currentConfirmationDepth", column: "current_confirmation_depth" },
  { field: "requiredConfirmationDepth", column: "required_confirmation_depth" },
];

export class PostgresBridgeStore implements BridgeStore {
  private logger = pino();
  private pool: Pool;

  constructor(config: PostgresBridgeStoreConfig) {
    this.pool = new Pool({
      connectionString: config.connectionString,
      max: config.maxConnections ?? 10,
      idleTimeoutMillis: config.idleTimeoutMs ?? 30000,
    });

    this.pool.on("error", (err) => {
      this.logger.error({ err }, "Unexpected PostgreSQL pool error");
    });
  }

  async save(transfer: StoredBridgeTransfer): Promise<void> {
    const client = await this.pool.connect();
    try {
      await client.query(
        `INSERT INTO bridge_transfers (
          id, status, direction, source_chain, destination_chain,
          user_address,
          amount, bridge_fee, net_amount,
          source_chain_tx_hash, bridge_tx_hash, destination_tx_hash,
          created_at, updated_at, estimated_arrival_time,
          retries_remaining, last_retry_time,
          stage, attempt_count, last_error, next_attempt_at, last_reconciled_at,
          error_message, idempotency_key
        ) VALUES (
          $1, $2, $3, $4, $5,
          $6,
          $7, $8, $9,
          $10, $11, $12,
          $13, $14, $15,
          $16, $17,
          $18, $19, $20, $21, $22,
          $23, $24
        )`,
        [
          transfer.id,
          transfer.status,
          transfer.direction,
          transfer.sourceChain,
          transfer.destinationChain,
          transfer.user,
          transfer.amount.toString(),
          transfer.bridgeFee.toString(),
          transfer.netAmount.toString(),
          transfer.sourceChainTxHash ?? null,
          transfer.bridgeTxHash ?? null,
          transfer.destinationTxHash ?? null,
          new Date(transfer.createdAt),
          new Date(transfer.updatedAt),
          transfer.estimatedArrivalTime
            ? new Date(transfer.estimatedArrivalTime)
            : null,
          transfer.retriesRemaining ?? 3,
          transfer.lastRetryTime ? new Date(transfer.lastRetryTime) : null,
          transfer.stage,
          transfer.attemptCount,
          transfer.lastError ? sanitizeErrorMessage(transfer.lastError) : null,
          transfer.nextAttemptAt ? new Date(transfer.nextAttemptAt) : null,
          transfer.lastReconciledAt
            ? new Date(transfer.lastReconciledAt)
            : null,
          transfer.errorMessage
            ? sanitizeErrorMessage(transfer.errorMessage)
            : null,
          transfer.idempotencyKey ?? null,
        ],
      );
      this.logger.debug({ transferId: transfer.id }, "Transfer saved to PostgreSQL");
    } finally {
      client.release();
    }
  }

  async get(transferId: string): Promise<StoredBridgeTransfer | null> {
    const client = await this.pool.connect();
    try {
      const result = await client.query(
        "SELECT * FROM bridge_transfers WHERE id = $1",
        [transferId],
      );

      if (result.rows.length === 0) return null;

      return this.mapRowToTransfer(result.rows[0]);
    } finally {
      client.release();
    }
  }

  async getAll(): Promise<StoredBridgeTransfer[]> {
    const client = await this.pool.connect();
    try {
      const result = await client.query(
        "SELECT * FROM bridge_transfers ORDER BY created_at ASC",
      );
      return result.rows.map((row) => this.mapRowToTransfer(row));
    } finally {
      client.release();
    }
  }

  async getPending(): Promise<StoredBridgeTransfer[]> {
    const client = await this.pool.connect();
    try {
      const result = await client.query(
        "SELECT * FROM bridge_transfers WHERE status IN ('pending', 'confirming') ORDER BY created_at ASC",
      );

      return result.rows.map((row) => this.mapRowToTransfer(row));
    } finally {
      client.release();
    }
  }

  async getNonTerminal(): Promise<StoredBridgeTransfer[]> {
    const client = await this.pool.connect();
    try {
      // Parameterised IN list keeps the terminal set in one place.
      const placeholders = TERMINAL_STATUSES.map((_, i) => `$${i + 1}`).join(", ");
      const result = await client.query(
        `SELECT * FROM bridge_transfers
         WHERE status NOT IN (${placeholders})
         ORDER BY created_at ASC`,
        [...TERMINAL_STATUSES],
      );

      return result.rows.map((row) => this.mapRowToTransfer(row));
    } finally {
      client.release();
    }
  }

  async getByUser(userAddress: string): Promise<StoredBridgeTransfer[]> {
    const client = await this.pool.connect();
    try {
      const result = await client.query(
        "SELECT * FROM bridge_transfers WHERE LOWER(user_address) = LOWER($1) ORDER BY created_at DESC",
        [userAddress],
      );

      return result.rows.map((row) => this.mapRowToTransfer(row));
    } finally {
      client.release();
    }
  }

  /**
   * #848 - Applies the whole patch in a single transaction: a partial write
   * would leave a durable record in a state the process never passed through,
   * so either every column lands or none does.
   */
  async update(
    transferId: string,
    updates: BridgeTransferPatch,
  ): Promise<void> {
    const client = await this.pool.connect();
    try {
      const { setClauses, values } = this.buildUpdate(updates, transferId);

      await client.query("BEGIN");
      await client.query(
        `UPDATE bridge_transfers SET ${setClauses.join(", ")} WHERE id = $${values.length}`,
        values,
      );
      await client.query("COMMIT");

      this.logger.debug({ transferId }, "Transfer updated in PostgreSQL");
    } catch (error) {
      await this.rollbackQuietly(client);
      this.logger.error({ transferId }, "Failed to update transfer");
      throw error;
    } finally {
      client.release();
    }
  }

  async delete(transferId: string): Promise<void> {
    const client = await this.pool.connect();
    try {
      await client.query("DELETE FROM bridge_transfers WHERE id = $1", [transferId]);
      this.logger.debug({ transferId }, "Transfer deleted from PostgreSQL");
    } finally {
      client.release();
    }
  }

  async close(): Promise<void> {
    await this.pool.end();
  }

  /**
   * Builds the SET clause for an update. The id is appended last so its
   * placeholder index is the highest one.
   */
  private buildUpdate(
    updates: BridgeTransferPatch,
    transferId: string,
  ): { setClauses: string[]; values: unknown[] } {
    const setClauses: string[] = [];
    const values: unknown[] = [];

    for (const { field, column } of SCALAR_PATCH_COLUMNS) {
      const value = updates[field];
      if (value === undefined) continue;
      setClauses.push(`${column} = $${values.length + 1}`);
      values.push(
        field === "lastError" || field === "errorMessage"
          ? sanitizeErrorMessage(value as string)
          : typeof value === "bigint"
            ? value.toString()
            : (value ?? null),
      );
    }

    for (const field of TIMESTAMP_PATCH_COLUMNS) {
      const value = updates[field];
      if (value === undefined) continue;
      setClauses.push(`${columnFor(field)} = $${values.length + 1}`);
      values.push(value === null ? null : new Date(value as number));
    }

    setClauses.push("updated_at = NOW()");
    values.push(transferId);

    return { setClauses, values };
  }

  private async rollbackQuietly(client: PoolClient): Promise<void> {
    try {
      await client.query("ROLLBACK");
    } catch {
      // The connection is already unusable; the pool will discard it.
    }
  }

  private mapRowToTransfer(row: Record<string, unknown>): StoredBridgeTransfer {
    return {
      id: row.id as string,
      status: row.status as StoredBridgeTransfer["status"],
      direction: row.direction as StoredBridgeTransfer["direction"],
      sourceChain: row.source_chain as StoredBridgeTransfer["sourceChain"],
      destinationChain:
        row.destination_chain as StoredBridgeTransfer["destinationChain"],
      user: row.user_address as string,
      amount: BigInt(row.amount as string),
      bridgeFee: BigInt(row.bridge_fee as string),
      netAmount: BigInt(row.net_amount as string),
      sourceChainTxHash: (row.source_chain_tx_hash as string) ?? undefined,
      bridgeTxHash: (row.bridge_tx_hash as string) ?? undefined,
      destinationTxHash: (row.destination_tx_hash as string) ?? undefined,
      createdAt: new Date(row.created_at as string).getTime(),
      updatedAt: new Date(row.updated_at as string).getTime(),
      estimatedArrivalTime: row.estimated_arrival_time
        ? new Date(row.estimated_arrival_time as string).getTime()
        : undefined,
      retriesRemaining: row.retries_remaining as number,
      lastRetryTime: row.last_retry_time
        ? new Date(row.last_retry_time as string).getTime()
        : undefined,
      stage: (row.stage as StoredBridgeTransfer["stage"]) ?? "observed",
      attemptCount: (row.attempt_count as number) ?? 0,
      lastError: (row.last_error as string) ?? undefined,
      nextAttemptAt: row.next_attempt_at
        ? new Date(row.next_attempt_at as string).getTime()
        : undefined,
      lastReconciledAt: row.last_reconciled_at
        ? new Date(row.last_reconciled_at as string).getTime()
        : undefined,
      errorMessage: (row.error_message as string) ?? undefined,
      idempotencyKey: (row.idempotency_key as string) ?? undefined,
      currentConfirmationDepth:
        (row.current_confirmation_depth as number) ?? undefined,
      requiredConfirmationDepth:
        (row.required_confirmation_depth as number) ?? undefined,
    };
  }
}

/** Maps a camelCase timestamp field onto its SQL column. */
function columnFor(field: keyof BridgeTransferPatch): string {
  switch (field) {
    case "lastRetryTime":
      return "last_retry_time";
    case "nextAttemptAt":
      return "next_attempt_at";
    case "lastReconciledAt":
      return "last_reconciled_at";
    default:
      throw new Error(`No timestamp column for field: ${String(field)}`);
  }
}
