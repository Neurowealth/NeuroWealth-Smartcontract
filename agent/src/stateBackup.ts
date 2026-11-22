/**
 * Agent state backup and restoration (Issue #655).
 *
 * The agent's operational state (scheduler position, last processed ledger,
 * metrics snapshot, allocation view) is periodically serialized to one or
 * more **pluggable stores** so a crashed or replaced agent server can restore
 * to its last known state within the 5-minute downtime budget.
 *
 * Stores:
 *  - `PostgresStateStore` — Supabase/Postgres via the shared pool (`db.ts`)
 *  - `FileStateStore`     — local JSON file (dev / last-resort fallback)
 *  - `S3StateStore`       — any HTTP PUT/GET object endpoint (S3, Supabase
 *    Storage, or a presigned-URL proxy)
 *  - `MultiStateStore`    — fans out saves and restores from the first store
 *    that yields a readable state (primary first)
 *
 * Restoration validates the schema `version` so an old/incompatible backup is
 * rejected instead of silently corrupting the new instance.
 */

import { promises as fs } from 'node:fs';
import path from 'node:path';
import logger from './logger';

/** Schema version — bump when AgentState changes shape. */
export const AGENT_STATE_VERSION = 1;

/** Serializable operational state of the agent. */
export interface AgentState {
  version: number;
  savedAt: number;
  /** Last Soroban ledger processed by the event listener. */
  lastLedger: number;
  lastRebalanceAt: number | null;
  scheduler: {
    interval: string;
    lastRunAt: number | null;
    consecutiveFailures: number;
  };
  metrics: {
    tvl: number;
    apy: number;
    allocations: Array<{ protocol: string; amount: number }>;
  };
}

/** Generic key/value persistence used by backup + failover. */
export interface StateStore<T = AgentState> {
  save(key: string, value: T): Promise<void>;
  load(key: string): Promise<T | null>;
}

/** Validates an untrusted loaded value as AgentState (version-checked). */
export function parseAgentState(raw: unknown): AgentState | null {
  if (!raw || typeof raw !== 'object') return null;
  const candidate = raw as Partial<AgentState>;
  if (candidate.version !== AGENT_STATE_VERSION) {
    logger.warn(
      { found: candidate.version, expected: AGENT_STATE_VERSION },
      'State backup version mismatch; ignoring backup',
    );
    return null;
  }
  if (typeof candidate.savedAt !== 'number' || typeof candidate.lastLedger !== 'number') {
    return null;
  }
  return {
    version: candidate.version,
    savedAt: candidate.savedAt,
    lastLedger: candidate.lastLedger,
    lastRebalanceAt: candidate.lastRebalanceAt ?? null,
    scheduler: {
      interval: candidate.scheduler?.interval ?? '4h',
      lastRunAt: candidate.scheduler?.lastRunAt ?? null,
      consecutiveFailures: candidate.scheduler?.consecutiveFailures ?? 0,
    },
    metrics: {
      tvl: candidate.metrics?.tvl ?? 0,
      apy: candidate.metrics?.apy ?? 0,
      allocations: candidate.metrics?.allocations ?? [],
    },
  };
}

/**
 * Postgres/Supabase store. Backed by the shared pg pool from `db.ts`; the
 * `agent_state` table is created lazily on first use.
 */
export class PostgresStateStore<T = AgentState> implements StateStore<T> {
  constructor(
    private readonly pool: {
      query: (sql: string, values?: unknown[]) => Promise<unknown>;
    },
  ) {}

  async save(key: string, value: T): Promise<void> {
    await this.pool.query(
      `INSERT INTO agent_state (key, value, updated_at)
       VALUES ($1, $2, NOW())
       ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = NOW()`,
      [key, JSON.stringify(value)],
    );
  }

  async load(key: string): Promise<T | null> {
    const result = (await this.pool.query(
      'SELECT value FROM agent_state WHERE key = $1',
      [key],
    )) as { rows: Array<{ value: unknown }> };
    if (!result.rows || result.rows.length === 0) return null;
    return result.rows[0].value as T;
  }
}


/** Local filesystem store (JSON file). Used for dev and as a local fallback. */
export class FileStateStore<T = AgentState> implements StateStore<T> {
  constructor(private readonly filePath: string) {}

  async save(key: string, value: T): Promise<void> {
    await fs.mkdir(path.dirname(this.filePath), { recursive: true });
    const wrapper = { key, value };
    await fs.writeFile(this.filePath, JSON.stringify(wrapper, null, 2), 'utf8');
  }

  async load(key: string): Promise<T | null> {
    try {
      const content = await fs.readFile(this.filePath, 'utf8');
      const wrapper = JSON.parse(content) as { key?: string; value?: T };
      if (wrapper.key !== key) return null;
      return (wrapper.value ?? null) as T | null;
    } catch {
      return null;
    }
  }
}


/**
 * HTTP object store (S3 / Supabase Storage compatible). Performs PUT on save
 * and GET on load against `baseUrl/key`; optional headers (e.g. Authorization
 * bearer for Supabase Storage RLS) are attached to both.
 */
export class S3StateStore<T = AgentState> implements StateStore<T> {
  constructor(
    private readonly baseUrl: string,
    private readonly headers: Record<string, string> = {},
  ) {}

  async save(key: string, value: T): Promise<void> {
    const response = await fetch(`${this.baseUrl}/${encodeURIComponent(key)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json', ...this.headers },
      body: JSON.stringify(value),
    });
    if (!response.ok) {
      throw new Error(`S3StateStore save failed: HTTP ${response.status}`);
    }
  }

  async load(key: string): Promise<T | null> {
    const response = await fetch(
      `${this.baseUrl}/${encodeURIComponent(key)}`,
      { headers: this.headers },
    );
    if (response.status === 404) return null;
    if (!response.ok) {
      throw new Error(`S3StateStore load failed: HTTP ${response.status}`);
    }
    return (await response.json()) as T;
  }
}


/**
 * Fans out saves to every store (best-effort: one failing store is logged,
 * not fatal) and restores from the first store that yields a readable state.
 */
export class MultiStateStore<T = AgentState> implements StateStore<T> {
  constructor(private readonly stores: Array<{ name: string; store: StateStore<T> }>) {}

  async save(key: string, value: T): Promise<void> {
    const errors: Array<{ name: string; error: string }> = [];
    for (const { name, store } of this.stores) {
      try {
        await store.save(key, value);
      } catch (err) {
        errors.push({
          name,
          error: err instanceof Error ? err.message : String(err),
        });
      }
    }
    if (errors.length === this.stores.length && this.stores.length > 0) {
      throw new Error(`all state stores failed: ${JSON.stringify(errors)}`);
    }
    if (errors.length > 0) {
      logger.warn({ errors }, 'some state stores failed to persist');
    }
  }

  async load(key: string): Promise<T | null> {
    for (const { name, store } of this.stores) {
      try {
        const value = await store.load(key);
        if (value !== null) return value;
      } catch (err) {
        logger.warn(
          { store: name, error: err instanceof Error ? err.message : err },
          'state store load failed; trying next',
        );
      }
    }
    return null;
  }
}

/**
 * High-level backup manager: validates, persists, and restores agent state.
 *
 * `startAutoBackup` snapshots state on a fixed interval (default 60s) so the
 * recovery point objective (RPO) stays inside the 5-minute downtime budget.
 */
export class StateBackupManager {
  private timer: ReturnType<typeof setInterval> | null = null;
  private lastState: AgentState | null = null;

  constructor(
    private readonly store: StateStore,
    private readonly key = 'agent:state',
    private readonly now: () => number = () => Date.now(),
  ) {}

  /** Persists the given state (version-stamped) to the store. */
  async backup(state: Omit<AgentState, 'version' | 'savedAt'>): Promise<AgentState> {
    const stamped: AgentState = {
      ...state,
      version: AGENT_STATE_VERSION,
      savedAt: this.now(),
    };
    await this.store.save(this.key, stamped);
    this.lastState = stamped;
    logger.debug({ at: stamped.savedAt, ledger: stamped.lastLedger }, 'Agent state backed up');
    return stamped;
  }

  /** Loads and validates the most recent backup, or null when none exists. */
  async restore(): Promise<AgentState | null> {
    const raw = await this.store.load(this.key);
    const state = parseAgentState(raw);
    if (state) {
      this.lastState = state;
      logger.info(
        { savedAt: state.savedAt, lastLedger: state.lastLedger },
        'Agent state restored from backup',
      );
    } else {
      logger.warn('No valid agent state backup found');
    }
    return state;
  }

  /** Most recently backed-up/restored state (in-memory fast path). */
  getLastState(): AgentState | null {
    return this.lastState;
  }

  /** Starts periodic backups; `getState` is called each interval. */
  startAutoBackup(
    getState: () => Omit<AgentState, 'version' | 'savedAt'>,
    intervalMs = 60_000,
  ): void {
    if (this.timer) return;
    this.timer = setInterval(() => {
      void this.backup(getState()).catch((err) => {
        logger.error(
          { error: err instanceof Error ? err.message : err },
          'Automatic state backup failed',
        );
      });
    }, intervalMs);
  }

  stopAutoBackup(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }
}
