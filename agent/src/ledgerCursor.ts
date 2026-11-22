/**
 * Persisted event cursor for the event listener (Issue #747).
 *
 * The listener's position is saved after every processed event so a restarted
 * agent resumes where it stopped instead of jumping to the latest ledger and
 * silently missing deposits/withdrawals in between.
 *
 * The position is the Soroban RPC paging token (an event id or a page
 * `cursor`), which is exact even when a page ends in the middle of a ledger.
 * `startLedger` is only used on the very first run.
 */

import { FileStateStore, PostgresStateStore, StateStore } from './stateBackup';
import logger from './logger';

export const LEDGER_CURSOR_KEY = 'agent:ledger_cursor';

export interface LedgerCursorValue {
  /** RPC paging token to resume from (exclusive). */
  pagingToken: string;
  /** Ledger of the last processed position, for logs / diagnostics. */
  ledger: number;
  updatedAt: number;
}

export type EventPosition = { cursor: string } | { startLedger: number };

type Queryable = { query: (sql: string, values?: unknown[]) => Promise<unknown> };

/** Postgres store that creates the `agent_state` table on first use. */
class EnsuredPostgresStore implements StateStore<LedgerCursorValue> {
  private ready: Promise<unknown> | null = null;
  private readonly inner: PostgresStateStore<LedgerCursorValue>;

  constructor(private readonly pool: Queryable) {
    this.inner = new PostgresStateStore<LedgerCursorValue>(pool);
  }

  private ensureTable(): Promise<unknown> {
    if (!this.ready) {
      this.ready = this.pool
        .query(
          `CREATE TABLE IF NOT EXISTS agent_state (
             key TEXT PRIMARY KEY,
             value JSONB NOT NULL,
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
           )`,
        )
        .catch((err) => {
          this.ready = null;
          throw err;
        });
    }
    return this.ready;
  }

  async save(key: string, value: LedgerCursorValue): Promise<void> {
    await this.ensureTable();
    await this.inner.save(key, value);
  }

  async load(key: string): Promise<LedgerCursorValue | null> {
    await this.ensureTable();
    return this.inner.load(key);
  }
}

/** Postgres when DATABASE_URL is set, otherwise a local JSON file. */
export function createLedgerCursorStore(pool: Queryable): StateStore<LedgerCursorValue> {
  if (process.env.DATABASE_URL) {
    return new EnsuredPostgresStore(pool);
  }
  const file = process.env.LEDGER_CURSOR_FILE || '.agent-state/ledger-cursor.json';
  logger.warn({ file }, 'DATABASE_URL not set; persisting ledger cursor to a local file');
  return new FileStateStore<LedgerCursorValue>(file);
}

export class LedgerCursor {
  private lastSavedToken: string | null = null;

  constructor(
    private readonly store: StateStore<LedgerCursorValue>,
    private readonly now: () => number = () => Date.now(),
  ) {}

  /** Returns the stored position, or null when none (or unreadable). */
  async load(): Promise<LedgerCursorValue | null> {
    try {
      const value = await this.store.load(LEDGER_CURSOR_KEY);
      if (value && typeof value.pagingToken === 'string' && value.pagingToken.length > 0) {
        this.lastSavedToken = value.pagingToken;
        return value;
      }
    } catch (error) {
      logger.error({ error: error instanceof Error ? error.message : error }, 'Failed to load ledger cursor');
      throw error;
    }
    return null;
  }

  /** Persists the position if it changed. Errors are logged, not thrown. */
  async save(pagingToken: string, ledger: number): Promise<void> {
    if (!pagingToken || pagingToken === this.lastSavedToken) return;
    try {
      await this.store.save(LEDGER_CURSOR_KEY, { pagingToken, ledger, updatedAt: this.now() });
      this.lastSavedToken = pagingToken;
    } catch (error) {
      logger.error({ error: error instanceof Error ? error.message : error, pagingToken }, 'Failed to persist ledger cursor');
    }
  }
}

/** Resume from the saved paging token, or start at `latestLedger` on first run. */
export function initialPosition(stored: LedgerCursorValue | null, latestLedger: number): EventPosition {
  return stored ? { cursor: stored.pagingToken } : { startLedger: latestLedger };
}

/**
 * True when the RPC rejected the request because the saved position is no
 * longer inside its retention window (or otherwise unusable).
 */
export function isStalePositionError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return /cursor|startLedger|ledger range|out of range|retention/i.test(message);
}
