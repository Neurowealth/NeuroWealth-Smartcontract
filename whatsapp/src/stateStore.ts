/**
 * Durable storage for OTP challenges and conversation sessions (#943).
 *
 * State used to live in per-process Maps, so a restart dropped every pending
 * OTP and session and each replica kept its own divergent copy. It now lives
 * in PostgreSQL (`whatsapp_otps`, `whatsapp_sessions`).
 *
 * Replica safety comes from making every operation a single atomic statement
 * (upsert / conditional UPDATE / DELETE ... RETURNING) instead of
 * read-modify-write in application memory:
 *
 *   - an OTP attempt is counted *before* the code is compared, so concurrent
 *     guesses on different replicas cannot exceed the attempt limit;
 *   - a verified OTP is consumed with DELETE ... RETURNING, so it can only be
 *     redeemed once even if two replicas verify it at the same moment;
 *   - rate-limit counters are incremented inside the database.
 *
 * Timestamps are epoch milliseconds supplied by the caller (BIGINT columns), so
 * every replica applies the same expiry rules and tests can control the clock.
 */
import { query } from './db';

export type StoredSessionState = 'UNVERIFIED' | 'AWAITING_OTP' | 'VERIFIED';

export interface StoredSession {
  state: StoredSessionState;
  lastMessageTime: number;
  messageCount: number;
  rateLimitWindowStart: number;
}

export type OtpAttempt =
  | { kind: 'none' }
  | { kind: 'expired' }
  | { kind: 'locked' }
  | { kind: 'attempt'; codeHash: string; attempts: number };

export interface StateStore {
  /** Replaces any existing challenge for the phone with a fresh one (attempts reset to 0). */
  putOtp(phoneHash: string, codeHash: string, expiresAtMs: number): Promise<void>;
  /**
   * Atomically counts one verification attempt against a live challenge and
   * returns the stored hash to compare. Expired and exhausted challenges are
   * deleted and reported as such.
   */
  attemptOtp(phoneHash: string, nowMs: number, maxAttempts: number): Promise<OtpAttempt>;
  /** Deletes the challenge iff it still holds `codeHash`. True for exactly one caller. */
  consumeOtp(phoneHash: string, codeHash: string): Promise<boolean>;

  /** Loads the session (creating it if absent) and resets it if inactive for longer than `timeoutMs`. */
  touchSession(phoneHash: string, nowMs: number, timeoutMs: number): Promise<StoredSession>;
  setSessionState(phoneHash: string, state: StoredSessionState, nowMs: number): Promise<void>;
  /** Atomically counts one message in the fixed window and returns the new count. */
  bumpMessageCount(phoneHash: string, nowMs: number, windowMs: number): Promise<number>;

  /** Removes sessions idle past `sessionTimeoutMs` and OTPs past their expiry. */
  purgeExpired(nowMs: number, sessionTimeoutMs: number): Promise<void>;
}

// ── PostgreSQL store ─────────────────────────────────────────────────────────

interface SessionRow {
  state: StoredSessionState;
  last_message_ms: string | number;
  message_count: number;
  rate_limit_window_start_ms: string | number;
}

function toSession(row: SessionRow): StoredSession {
  return {
    state: row.state,
    lastMessageTime: Number(row.last_message_ms),
    messageCount: Number(row.message_count),
    rateLimitWindowStart: Number(row.rate_limit_window_start_ms),
  };
}

export class PostgresStateStore implements StateStore {
  async putOtp(phoneHash: string, codeHash: string, expiresAtMs: number): Promise<void> {
    await query(
      `INSERT INTO whatsapp_otps (phone_hash, code_hash, expires_at_ms, attempts, created_at)
       VALUES ($1, $2, $3::bigint, 0, NOW())
       ON CONFLICT (phone_hash) DO UPDATE
         SET code_hash = EXCLUDED.code_hash,
             expires_at_ms = EXCLUDED.expires_at_ms,
             attempts = 0,
             created_at = NOW()`,
      [phoneHash, codeHash, expiresAtMs]
    );
  }

  async attemptOtp(phoneHash: string, nowMs: number, maxAttempts: number): Promise<OtpAttempt> {
    const counted = await query(
      `UPDATE whatsapp_otps
          SET attempts = attempts + 1
        WHERE phone_hash = $1 AND expires_at_ms >= $2::bigint AND attempts < $3::int
        RETURNING code_hash, attempts`,
      [phoneHash, nowMs, maxAttempts]
    );
    if (counted.rows.length > 0) {
      const row = counted.rows[0] as { code_hash: string; attempts: number };
      return { kind: 'attempt', codeHash: row.code_hash, attempts: Number(row.attempts) };
    }

    const existing = await query(
      'SELECT expires_at_ms, attempts FROM whatsapp_otps WHERE phone_hash = $1',
      [phoneHash]
    );
    if (existing.rows.length === 0) return { kind: 'none' };

    const row = existing.rows[0] as { expires_at_ms: string | number; attempts: number };
    // Only delete the row we actually classified, so a challenge reissued in
    // between (fresh expiry, attempts = 0) is left alone.
    await query(
      `DELETE FROM whatsapp_otps
        WHERE phone_hash = $1 AND (expires_at_ms < $2::bigint OR attempts >= $3::int)`,
      [phoneHash, nowMs, maxAttempts]
    );
    return Number(row.expires_at_ms) < nowMs ? { kind: 'expired' } : { kind: 'locked' };
  }

  async consumeOtp(phoneHash: string, codeHash: string): Promise<boolean> {
    const res = await query(
      'DELETE FROM whatsapp_otps WHERE phone_hash = $1 AND code_hash = $2 RETURNING phone_hash',
      [phoneHash, codeHash]
    );
    return res.rows.length > 0;
  }

  async touchSession(phoneHash: string, nowMs: number, timeoutMs: number): Promise<StoredSession> {
    const res = await query(
      `INSERT INTO whatsapp_sessions
         (phone_hash, state, last_message_ms, message_count, rate_limit_window_start_ms, updated_at)
       VALUES ($1, 'UNVERIFIED', $2::bigint, 0, $2::bigint, NOW())
       ON CONFLICT (phone_hash) DO UPDATE SET
         state = CASE WHEN $2::bigint - whatsapp_sessions.last_message_ms > $3::bigint
                      THEN 'UNVERIFIED' ELSE whatsapp_sessions.state END,
         message_count = CASE WHEN $2::bigint - whatsapp_sessions.last_message_ms > $3::bigint
                              THEN 0 ELSE whatsapp_sessions.message_count END,
         rate_limit_window_start_ms = CASE WHEN $2::bigint - whatsapp_sessions.last_message_ms > $3::bigint
                                           THEN $2::bigint ELSE whatsapp_sessions.rate_limit_window_start_ms END,
         last_message_ms = $2::bigint,
         updated_at = NOW()
       RETURNING state, last_message_ms, message_count, rate_limit_window_start_ms`,
      [phoneHash, nowMs, timeoutMs]
    );
    return toSession(res.rows[0] as SessionRow);
  }

  async setSessionState(phoneHash: string, state: StoredSessionState, nowMs: number): Promise<void> {
    await query(
      `INSERT INTO whatsapp_sessions
         (phone_hash, state, last_message_ms, message_count, rate_limit_window_start_ms, updated_at)
       VALUES ($1, $2, $3::bigint, 0, $3::bigint, NOW())
       ON CONFLICT (phone_hash) DO UPDATE
         SET state = EXCLUDED.state, last_message_ms = EXCLUDED.last_message_ms, updated_at = NOW()`,
      [phoneHash, state, nowMs]
    );
  }

  async bumpMessageCount(phoneHash: string, nowMs: number, windowMs: number): Promise<number> {
    const res = await query(
      `INSERT INTO whatsapp_sessions
         (phone_hash, state, last_message_ms, message_count, rate_limit_window_start_ms, updated_at)
       VALUES ($1, 'UNVERIFIED', $2::bigint, 1, $2::bigint, NOW())
       ON CONFLICT (phone_hash) DO UPDATE SET
         message_count = CASE WHEN $2::bigint - whatsapp_sessions.rate_limit_window_start_ms > $3::bigint
                              THEN 1 ELSE whatsapp_sessions.message_count + 1 END,
         rate_limit_window_start_ms = CASE WHEN $2::bigint - whatsapp_sessions.rate_limit_window_start_ms > $3::bigint
                                           THEN $2::bigint ELSE whatsapp_sessions.rate_limit_window_start_ms END,
         updated_at = NOW()
       RETURNING message_count`,
      [phoneHash, nowMs, windowMs]
    );
    return Number((res.rows[0] as { message_count: number }).message_count);
  }

  async purgeExpired(nowMs: number, sessionTimeoutMs: number): Promise<void> {
    await query('DELETE FROM whatsapp_sessions WHERE last_message_ms < $1::bigint', [nowMs - sessionTimeoutMs]);
    await query('DELETE FROM whatsapp_otps WHERE expires_at_ms < $1::bigint', [nowMs]);
  }
}

// ── In-memory store (tests, single-process development) ─────────────────────

interface MemoryOtp {
  codeHash: string;
  expiresAtMs: number;
  attempts: number;
}

/**
 * Same semantics as the PostgreSQL store. Every method body runs
 * synchronously after one microtask hop, so each operation is atomic with
 * respect to concurrent callers - mirroring single-statement atomicity.
 */
export class InMemoryStateStore implements StateStore {
  readonly otps = new Map<string, MemoryOtp>();
  readonly sessions = new Map<string, StoredSession>();

  async putOtp(phoneHash: string, codeHash: string, expiresAtMs: number): Promise<void> {
    await Promise.resolve();
    this.otps.set(phoneHash, { codeHash, expiresAtMs, attempts: 0 });
  }

  async attemptOtp(phoneHash: string, nowMs: number, maxAttempts: number): Promise<OtpAttempt> {
    await Promise.resolve();
    const record = this.otps.get(phoneHash);
    if (!record) return { kind: 'none' };
    if (record.expiresAtMs < nowMs) {
      this.otps.delete(phoneHash);
      return { kind: 'expired' };
    }
    if (record.attempts >= maxAttempts) {
      this.otps.delete(phoneHash);
      return { kind: 'locked' };
    }
    record.attempts += 1;
    return { kind: 'attempt', codeHash: record.codeHash, attempts: record.attempts };
  }

  async consumeOtp(phoneHash: string, codeHash: string): Promise<boolean> {
    await Promise.resolve();
    const record = this.otps.get(phoneHash);
    if (!record || record.codeHash !== codeHash) return false;
    this.otps.delete(phoneHash);
    return true;
  }

  async touchSession(phoneHash: string, nowMs: number, timeoutMs: number): Promise<StoredSession> {
    await Promise.resolve();
    const existing = this.sessions.get(phoneHash);
    if (!existing) {
      const created: StoredSession = {
        state: 'UNVERIFIED',
        lastMessageTime: nowMs,
        messageCount: 0,
        rateLimitWindowStart: nowMs,
      };
      this.sessions.set(phoneHash, created);
      return { ...created };
    }
    if (nowMs - existing.lastMessageTime > timeoutMs) {
      existing.state = 'UNVERIFIED';
      existing.messageCount = 0;
      existing.rateLimitWindowStart = nowMs;
    }
    existing.lastMessageTime = nowMs;
    return { ...existing };
  }

  async setSessionState(phoneHash: string, state: StoredSessionState, nowMs: number): Promise<void> {
    await Promise.resolve();
    const existing = this.sessions.get(phoneHash);
    if (existing) {
      existing.state = state;
      existing.lastMessageTime = nowMs;
    } else {
      this.sessions.set(phoneHash, {
        state,
        lastMessageTime: nowMs,
        messageCount: 0,
        rateLimitWindowStart: nowMs,
      });
    }
  }

  async bumpMessageCount(phoneHash: string, nowMs: number, windowMs: number): Promise<number> {
    await Promise.resolve();
    const existing = this.sessions.get(phoneHash);
    if (!existing) {
      this.sessions.set(phoneHash, {
        state: 'UNVERIFIED',
        lastMessageTime: nowMs,
        messageCount: 1,
        rateLimitWindowStart: nowMs,
      });
      return 1;
    }
    if (nowMs - existing.rateLimitWindowStart > windowMs) {
      existing.rateLimitWindowStart = nowMs;
      existing.messageCount = 1;
    } else {
      existing.messageCount += 1;
    }
    return existing.messageCount;
  }

  async purgeExpired(nowMs: number, sessionTimeoutMs: number): Promise<void> {
    await Promise.resolve();
    for (const [hash, session] of this.sessions) {
      if (session.lastMessageTime < nowMs - sessionTimeoutMs) this.sessions.delete(hash);
    }
    for (const [hash, otp] of this.otps) {
      if (otp.expiresAtMs < nowMs) this.otps.delete(hash);
    }
  }
}

// ── Store selection ──────────────────────────────────────────────────────────

let activeStore: StateStore = new PostgresStateStore();

export function getStateStore(): StateStore {
  return activeStore;
}

/** Swaps the backing store (tests). */
export function setStateStore(store: StateStore): void {
  activeStore = store;
}
