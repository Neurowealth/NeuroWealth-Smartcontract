/**
 * Persistent storage for bridge transfers (database abstraction)
 */

import pino from "pino";
import {
  StoredBridgeTransfer,
  BridgeTransfer,
  BridgeStatus,
  BridgeTransferPatch,
} from "./types";
import { sanitizeErrorMessage } from "./redaction";

/**
 * Statuses a transfer can never leave. Terminal records stay queryable but
 * must never be retried or re-submitted (#850).
 */
export const TERMINAL_STATUSES: readonly BridgeStatus[] = ["confirmed", "cancelled"];

export function isTerminalStatus(status: BridgeStatus): boolean {
  return TERMINAL_STATUSES.includes(status);
}

export interface BridgeStore {
  save(transfer: StoredBridgeTransfer): Promise<void>;
  get(transferId: string): Promise<StoredBridgeTransfer | null>;
  /** Every durable record, oldest first. Used to rebuild state after a restart. */
  getAll(): Promise<StoredBridgeTransfer[]>;
  getPending(): Promise<StoredBridgeTransfer[]>;
  /**
   * Every durable record that is not in a terminal state, i.e. the input of the
   * startup reconciliation pass (#850).
   */
  getNonTerminal(): Promise<StoredBridgeTransfer[]>;
  getByUser(userAddress: string): Promise<StoredBridgeTransfer[]>;
  /**
   * Applies a patch atomically: either every field lands or the stored record
   * is left exactly as it was. A partially written record would make a restart
   * resume from a state that never existed (#848).
   */
  update(transferId: string, updates: BridgeTransferPatch): Promise<void>;
  delete(transferId: string): Promise<void>;
}

/**
 * Copies a record so the store never aliases the caller's mutable object. The
 * manager keeps mutating its in-memory transfer while the store owns the
 * durable snapshot; sharing a reference would let a later mutation leak into
 * "persisted" state without a write.
 */
function snapshot(transfer: StoredBridgeTransfer): StoredBridgeTransfer {
  return { ...transfer };
}

/**
 * In-memory store for development/testing
 * Replace with PostgreSQL/Supabase in production
 */
export class InMemoryBridgeStore implements BridgeStore {
  private logger = pino();
  private transfers: Map<string, StoredBridgeTransfer> = new Map();

  async save(transfer: StoredBridgeTransfer): Promise<void> {
    this.transfers.set(transfer.id, snapshot(transfer));
    this.logger.debug({ transferId: transfer.id }, "Transfer saved to store");
  }

  async get(transferId: string): Promise<StoredBridgeTransfer | null> {
    const stored = this.transfers.get(transferId);
    return stored ? snapshot(stored) : null;
  }

  async getAll(): Promise<StoredBridgeTransfer[]> {
    return Array.from(this.transfers.values())
      .sort((a, b) => a.createdAt - b.createdAt)
      .map(snapshot);
  }

  async getPending(): Promise<StoredBridgeTransfer[]> {
    return Array.from(this.transfers.values())
      .filter((t) => t.status === "pending" || t.status === "confirming")
      .map(snapshot);
  }

  async getNonTerminal(): Promise<StoredBridgeTransfer[]> {
    return Array.from(this.transfers.values())
      .filter((t) => !isTerminalStatus(t.status))
      .map(snapshot);
  }

  async getByUser(userAddress: string): Promise<StoredBridgeTransfer[]> {
    return Array.from(this.transfers.values())
      .filter((t) => t.user.toLowerCase() === userAddress.toLowerCase())
      .map(snapshot);
  }

  async update(
    transferId: string,
    updates: BridgeTransferPatch,
  ): Promise<void> {
    const transfer = this.transfers.get(transferId);
    if (!transfer) {
      throw new Error(`Transfer not found: ${transferId}`);
    }

    // Sanitise free-form text before it becomes durable state (#848).
    const patch: BridgeTransferPatch = { ...updates };
    if (updates.lastError !== undefined) {
      patch.lastError = sanitizeErrorMessage(updates.lastError);
    }
    if (updates.errorMessage !== undefined) {
      patch.errorMessage = sanitizeErrorMessage(updates.errorMessage);
    }

    // Single atomic swap: a throw while building the patch cannot leave a
    // half-written record behind.
    const updated: StoredBridgeTransfer = {
      ...transfer,
      ...patch,
      updatedAt: Date.now(),
    };
    this.transfers.set(transferId, updated);
    this.logger.debug({ transferId }, "Transfer updated in store");
  }

  async delete(transferId: string): Promise<void> {
    this.transfers.delete(transferId);
    this.logger.debug({ transferId }, "Transfer deleted from store");
  }
}

/**
 * SQL-based store template (Supabase/PostgreSQL)
 * Implement this for production
 */
export class SqlBridgeStore implements BridgeStore {
  private logger = pino();

  constructor(private dbClient: any) {} // Replace with actual DB client type

  async save(transfer: StoredBridgeTransfer): Promise<void> {
    const { data, error } = await this.dbClient
      .from("bridge_transfers")
      .insert([transfer]);

    if (error) {
      this.logger.error({ error }, "Failed to save transfer");
      throw error;
    }

    this.logger.debug(
      { transferId: transfer.id },
      "Transfer saved to database",
    );
  }

  async get(transferId: string): Promise<StoredBridgeTransfer | null> {
    const { data, error } = await this.dbClient
      .from("bridge_transfers")
      .select("*")
      .eq("id", transferId)
      .single();

    if (error && error.code !== "PGRST116") {
      throw error;
    }

    return data || null;
  }

  async getAll(): Promise<StoredBridgeTransfer[]> {
    const { data, error } = await this.dbClient
      .from("bridge_transfers")
      .select("*")
      .order("created_at", { ascending: true });

    if (error) {
      throw error;
    }

    return data || [];
  }

  async getPending(): Promise<StoredBridgeTransfer[]> {
    const { data, error } = await this.dbClient
      .from("bridge_transfers")
      .select("*")
      .in("status", ["pending", "confirming"]);

    if (error) {
      throw error;
    }

    return data || [];
  }

  async getNonTerminal(): Promise<StoredBridgeTransfer[]> {
    const { data, error } = await this.dbClient
      .from("bridge_transfers")
      .select("*")
      .not("status", "in", `(${TERMINAL_STATUSES.join(",")})`);

    if (error) {
      throw error;
    }

    return data || [];
  }

  async getByUser(userAddress: string): Promise<StoredBridgeTransfer[]> {
    const { data, error } = await this.dbClient
      .from("bridge_transfers")
      .select("*")
      .ilike("user", userAddress);

    if (error) {
      throw error;
    }

    return data || [];
  }

  async update(
    transferId: string,
    updates: BridgeTransferPatch,
  ): Promise<void> {
    const { error } = await this.dbClient
      .from("bridge_transfers")
      .update({ ...updates, updatedAt: Date.now() })
      .eq("id", transferId);

    if (error) {
      this.logger.error({ error }, "Failed to update transfer");
      throw error;
    }

    this.logger.debug({ transferId }, "Transfer updated in database");
  }

  async delete(transferId: string): Promise<void> {
    const { error } = await this.dbClient
      .from("bridge_transfers")
      .delete()
      .eq("id", transferId);

    if (error) {
      this.logger.error({ error }, "Failed to delete transfer");
      throw error;
    }

    this.logger.debug({ transferId }, "Transfer deleted from database");
  }
}
