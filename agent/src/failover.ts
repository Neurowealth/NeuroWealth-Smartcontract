/**
 * Agent failover and heartbeat monitoring (Issue #655).
 *
 * A **primary** agent instance writes heartbeats to the shared state store.
 * One or more **hot standby** instances poll the same store; when no fresh
 * heartbeat is observed within `heartbeatTtlMs`, a standby promotes itself,
 * restores the last state backup, and resumes operations.
 *
 * Promotion uses a lease record with a deterministic tie-break (lowest
 * agentId wins) so two standbys racing on a dead primary converge on a single
 * primary instead of splitting brain. Maximum acceptable downtime is 5
 * minutes; with the default 15s heartbeat interval and 60s TTL, detection
 * plus promotion completes in well under 90 seconds.
 */

import logger from './logger';
import { StateStore } from './stateBackup';

/** What a standby should do after a failover check. */
export type FailoverAction = 'promoted' | 'primary' | 'standby';

/** Lease/heartbeat record persisted to the shared store. */
export interface FailoverLease {
  /** Instance currently holding (or last to hold) the primary lease. */
  primaryId: string;
  /** Last heartbeat timestamp (ms epoch). */
  at: number;
  /** Monotonic heartbeat counter (informational). */
  seq: number;
}

export interface FailoverConfig {
  agentId: string;
  /** How often the primary writes a heartbeat. Default 15s. */
  heartbeatIntervalMs?: number;
  /** Heartbeats older than this are considered dead. Default 60s. */
  heartbeatTtlMs?: number;
  /** Shared-store key for the lease record. */
  leaseKey?: string;
}

export interface FailoverDeps {
  store: StateStore<FailoverLease>;
  /** Called locally when this instance becomes primary. */
  onPromote?: () => Promise<void> | void;
  /** Called locally when this instance steps down. */
  onDemote?: () => Promise<void> | void;
  now?: () => number;
}

export class FailoverManager {
  private leaseKey: string;
  private heartbeatIntervalMs: number;
  private heartbeatTtlMs: number;
  private primary = false;
  private seq = 0;
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private pollTimer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private readonly config: FailoverConfig,
    private readonly deps: FailoverDeps,
  ) {
    this.leaseKey = config.leaseKey ?? 'agent:lease';
    this.heartbeatIntervalMs = config.heartbeatIntervalMs ?? 15_000;
    this.heartbeatTtlMs = config.heartbeatTtlMs ?? 60_000;
  }

  /** Whether this instance currently believes it is primary. */
  isPrimary(): boolean {
    return this.primary;
  }

  /** Primary-side heartbeat write. Throws if the store write fails. */
  async beat(healthy = true): Promise<FailoverLease> {
    if (!this.primary) {
      throw new Error('beat() called while not primary');
    }
    const lease: FailoverLease = {
      primaryId: this.config.agentId,
      at: this.now(),
      seq: ++this.seq,
    };
    await this.deps.store.save(this.leaseKey, lease);
    logger.debug({ seq: lease.seq, healthy }, 'Heartbeat written');
    return lease;
  }

  /** Voluntarily releases the primary lease. */
  async stepDown(): Promise<void> {
    if (!this.primary) return;
    this.primary = false;
    logger.info({ agentId: this.config.agentId }, 'Stepping down from primary');
    if (this.deps.onDemote) {
      await this.deps.onDemote();
    }
  }

  /**
   * Attempts to become primary at startup: takes the lease if it is free
   * (no record, or a stale heartbeat), otherwise stays standby.
   */
  async bootstrap(): Promise<FailoverAction> {
    const lease = await this.deps.store.load(this.leaseKey);
    const now = this.now();
    if (!lease || now - lease.at > this.heartbeatTtlMs) {
      await this.acquireLease(lease);
      return 'promoted';
    }
    this.primary = false;
    logger.info(
      { primaryId: lease.primaryId, ageMs: now - lease.at },
      'Healthy primary detected; running as hot standby',
    );
    return 'standby';
  }

  /**
   * Standby-side failover check: promotes this instance when the lease is
   * missing or stale, re-affirms when already primary, and honors a
   * higher-priority (lower agentId) claimant after a write race.
   */
  async checkAndFailover(): Promise<FailoverAction> {
    const lease = await this.deps.store.load(this.leaseKey);
    const now = this.now();

    if (this.primary) {
      // Re-read to detect a manual demotion or a higher-priority claimant.
      if (lease && lease.primaryId !== this.config.agentId) {
        const winner = [lease.primaryId, this.config.agentId].sort()[0];
        if (winner === lease.primaryId) {
          await this.stepDown();
          return 'standby';
        }
      }
      await this.beat();
      return 'primary';
    }

    const stale = !lease || now - lease.at > this.heartbeatTtlMs;
    if (stale) {
      await this.acquireLease(lease);
      return 'promoted';
    }

    // Fresh heartbeat from another instance — remain standby.
    return 'standby';
  }

  /** Starts the primary heartbeat loop. */
  startHeartbeat(): void {
    if (this.heartbeatTimer) return;
    this.heartbeatTimer = setInterval(() => {
      void this.beat().catch((err) =>
        logger.error(
          { error: err instanceof Error ? err.message : err },
          'Heartbeat write failed',
        ),
      );
    }, this.heartbeatIntervalMs);
  }

  /** Starts the standby failover polling loop. */
  startStandbyPolling(pollMs?: number): void {
    if (this.pollTimer) return;
    const interval = pollMs ?? Math.floor(this.heartbeatIntervalMs / 2);
    this.pollTimer = setInterval(() => {
      void this.checkAndFailover().catch((err) =>
        logger.error(
          { error: err instanceof Error ? err.message : err },
          'Failover check failed',
        ),
      );
    }, interval);
  }

  stop(): void {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
    if (this.pollTimer) {
      clearInterval(this.pollTimer);
      this.pollTimer = null;
    }
  }

  /**
   * Takes the lease. On a race between two standbys both observing staleness,
   * both write; the deterministic tie-break (lowest agentId owns the lease)
   * is enforced by whichever instance next re-reads the record.
   */
  private async acquireLease(previous: FailoverLease | null): Promise<void> {
    const competing = previous?.primaryId;
    if (
      competing &&
      previous &&
      competing !== this.config.agentId &&
      this.now() - previous.at <= this.heartbeatTtlMs &&
      competing < this.config.agentId
    ) {
      // A fresh, higher-priority (lower agentId) instance holds the lease.
      this.primary = false;
      return;
    }

    this.primary = true;
    this.seq = 0;
    await this.beat();
    logger.info(
      { agentId: this.config.agentId, previousPrimary: competing ?? null },
      'Acquired primary lease',
    );
    if (this.deps.onPromote) {
      await this.deps.onPromote();
    }
  }

  private now(): number {
    return this.deps.now ? this.deps.now() : Date.now();
  }
}

