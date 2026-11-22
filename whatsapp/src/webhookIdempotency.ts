/**
 * Idempotent processing for inbound Twilio WhatsApp webhooks.
 *
 * Twilio can redeliver the same inbound-message webhook (e.g. after our
 * response timed out). Each delivery is keyed by the provider message
 * identifier plus the webhook event identity, and a processing record is
 * claimed *before* the command runs:
 *
 *   - `processing` - an attempt holds a time-bounded lease on the record.
 *   - `completed`  - the command finished (successfully or with a terminal,
 *                    user-facing error). Duplicates are acknowledged without
 *                    re-running the command or sending a second reply.
 *   - `failed`     - a retryable failure happened *before* any side effect
 *                    started, so the next delivery may safely re-run it.
 *   - `abandoned`  - an attempt's lease expired after side effects had
 *                    started (e.g. the process crashed mid-transaction). The
 *                    outcome is unknown, so it is never re-run automatically.
 *
 * Every attempt carries a fencing token (`attempt`); writes from an attempt
 * whose lease was taken over by a later one are rejected.
 */
import { query } from './db';

export type InboundWebhookStatus = 'processing' | 'completed' | 'failed' | 'abandoned';

export type ClaimResult =
  | { kind: 'acquired'; attempt: number }
  | { kind: 'completed'; responseStatus: number }
  | { kind: 'in_progress' }
  | { kind: 'abandoned' };

export interface ClaimMetadata {
  messageSid: string | null;
  phoneHash: string;
}

export interface IdempotencyStore {
  /** Atomically claims the key for a new processing attempt, or reports why it cannot be. */
  claim(key: string, meta: ClaimMetadata, leaseMs: number): Promise<ClaimResult>;
  /** Records that the attempt is about to mutate state. Returns false if the attempt lost its lease. */
  markSideEffectsStarted(key: string, attempt: number): Promise<boolean>;
  /** Marks the attempt as finished; duplicates will now be acknowledged without re-running. */
  complete(key: string, attempt: number, responseStatus: number): Promise<void>;
  /** Marks the attempt as a retryable failure so the next delivery may resume it. */
  fail(key: string, attempt: number, error: string): Promise<void>;
}

/** Thrown for failures that happened before any side effect, so redelivery can safely re-run the command. */
export class RetryableWebhookError extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message);
    this.name = 'RetryableWebhookError';
    if (options && 'cause' in options) {
      (this as { cause?: unknown }).cause = options.cause;
    }
  }
}

/** Thrown when another attempt took over this delivery's lease before side effects began. */
export class LeaseLostError extends Error {
  constructor() {
    super('Idempotency lease was taken over by another attempt');
    this.name = 'LeaseLostError';
  }
}

const INBOUND_MESSAGE_EVENT = 'inbound-message';

const TRANSIENT_ERROR_CODES = new Set([
  'ECONNREFUSED',
  'ECONNRESET',
  'ETIMEDOUT',
  'EPIPE',
  'EAI_AGAIN',
  'ENOTFOUND',
  // PostgreSQL SQLSTATEs: serialization failure, deadlock, too many
  // connections, admin shutdown, cannot connect now.
  '40001',
  '40P01',
  '53300',
  '57P01',
  '57P03',
]);

/** True for infrastructure errors where retrying the same request may succeed. */
export function isTransientError(error: unknown): boolean {
  if (error instanceof RetryableWebhookError) return true;
  const code = (error as { code?: unknown } | null)?.code;
  if (typeof code !== 'string') return false;
  // SQLSTATE class 08 = connection exception.
  return TRANSIENT_ERROR_CODES.has(code) || /^08[0-9A-Z]{3}$/.test(code);
}

/**
 * Derives the idempotency key for an inbound WhatsApp webhook.
 *
 * Twilio's MessageSid identifies the inbound message and is stable across
 * redeliveries; it is scoped by AccountSid and by the webhook event type so
 * a different event about the same message can never collide with it. When
 * MessageSid is absent, Twilio's `I-Twilio-Idempotency-Token` header (also
 * stable across retries) is used instead. Returns null when neither exists.
 */
export function deriveIdempotencyKey(
  body: Record<string, unknown> | undefined,
  idempotencyTokenHeader?: string | string[]
): string | null {
  const accountSid = typeof body?.AccountSid === 'string' && body.AccountSid ? body.AccountSid : '-';
  const messageSid = typeof body?.MessageSid === 'string' ? body.MessageSid.trim() : '';
  if (messageSid) {
    return `twilio:${INBOUND_MESSAGE_EVENT}:${accountSid}:${messageSid}`;
  }

  const token = Array.isArray(idempotencyTokenHeader) ? idempotencyTokenHeader[0] : idempotencyTokenHeader;
  if (token && token.trim()) {
    return `twilio:${INBOUND_MESSAGE_EVENT}:${accountSid}:token:${token.trim()}`;
  }

  return null;
}

/**
 * Tracks whether the current attempt has started mutating state. Errors
 * raised before `begin()` are safe to retry; errors after it are not, since
 * the command may already have partially applied.
 */
export class SideEffectGuard {
  private started = false;

  constructor(
    private readonly store: IdempotencyStore | null,
    private readonly key: string | null,
    private readonly attempt: number
  ) {}

  get hasStarted(): boolean {
    return this.started;
  }

  async begin(): Promise<void> {
    if (this.started) return;
    if (this.store && this.key) {
      const stillOwned = await this.store.markSideEffectsStarted(this.key, this.attempt);
      if (!stillOwned) throw new LeaseLostError();
    }
    this.started = true;
  }
}

// ── PostgreSQL store ─────────────────────────────────────────────────────────

export class PostgresIdempotencyStore implements IdempotencyStore {
  async claim(key: string, meta: ClaimMetadata, leaseMs: number): Promise<ClaimResult> {
    // A single atomic upsert decides ownership: a new key is inserted, and an
    // existing one is only taken over if its last attempt failed retryably or
    // its lease expired before any side effect started.
    const claimed = await query(
      `INSERT INTO whatsapp_inbound_webhooks
         (idempotency_key, message_sid, phone_hash, status, attempt_count, side_effects_started,
          lease_expires_at, created_at, updated_at)
       VALUES ($1, $2, $3, 'processing', 1, FALSE, NOW() + $4::double precision * INTERVAL '1 millisecond', NOW(), NOW())
       ON CONFLICT (idempotency_key) DO UPDATE
         SET status = 'processing',
             attempt_count = whatsapp_inbound_webhooks.attempt_count + 1,
             lease_expires_at = EXCLUDED.lease_expires_at,
             last_error = NULL,
             updated_at = NOW()
       WHERE whatsapp_inbound_webhooks.status = 'failed'
          OR (whatsapp_inbound_webhooks.status = 'processing'
              AND whatsapp_inbound_webhooks.lease_expires_at < NOW()
              AND NOT whatsapp_inbound_webhooks.side_effects_started)
       RETURNING attempt_count`,
      [key, meta.messageSid, meta.phoneHash, leaseMs]
    );
    if (claimed.rows.length > 0) {
      return { kind: 'acquired', attempt: Number(claimed.rows[0].attempt_count) };
    }

    const existing = await query(
      `SELECT status, response_status, side_effects_started, lease_expires_at < NOW() AS lease_expired
         FROM whatsapp_inbound_webhooks WHERE idempotency_key = $1`,
      [key]
    );
    const row = existing.rows[0];
    if (!row) {
      // Only reachable if the row was deleted between the two statements.
      throw new RetryableWebhookError('Idempotency record vanished during claim');
    }

    return this.classifyExisting(key, row);
  }

  private async classifyExisting(key: string, row: Record<string, unknown>): Promise<ClaimResult> {
    if (row.status === 'completed') {
      return { kind: 'completed', responseStatus: Number(row.response_status) || 200 };
    }
    if (row.status === 'abandoned') {
      return { kind: 'abandoned' };
    }
    if (row.status === 'processing' && row.lease_expired && row.side_effects_started) {
      await query(
        `UPDATE whatsapp_inbound_webhooks
            SET status = 'abandoned',
                last_error = 'lease expired after side effects started; manual reconciliation required',
                updated_at = NOW()
          WHERE idempotency_key = $1 AND status = 'processing'`,
        [key]
      );
      return { kind: 'abandoned' };
    }
    return { kind: 'in_progress' };
  }

  async markSideEffectsStarted(key: string, attempt: number): Promise<boolean> {
    const res = await query(
      `UPDATE whatsapp_inbound_webhooks
          SET side_effects_started = TRUE, updated_at = NOW()
        WHERE idempotency_key = $1 AND attempt_count = $2 AND status = 'processing'`,
      [key, attempt]
    );
    return (res.rowCount ?? 0) > 0;
  }

  async complete(key: string, attempt: number, responseStatus: number): Promise<void> {
    await query(
      `UPDATE whatsapp_inbound_webhooks
          SET status = 'completed', response_status = $3, completed_at = NOW(), updated_at = NOW()
        WHERE idempotency_key = $1 AND attempt_count = $2 AND status = 'processing'`,
      [key, attempt, responseStatus]
    );
  }

  async fail(key: string, attempt: number, error: string): Promise<void> {
    await query(
      `UPDATE whatsapp_inbound_webhooks
          SET status = 'failed', last_error = $3, updated_at = NOW()
        WHERE idempotency_key = $1 AND attempt_count = $2 AND status = 'processing'`,
      [key, attempt, error.slice(0, 500)]
    );
  }
}

// ── In-memory store (tests, single-process development) ─────────────────────

interface MemoryRecord {
  status: InboundWebhookStatus;
  attempt: number;
  sideEffectsStarted: boolean;
  leaseExpiresAt: number;
  responseStatus: number | null;
  lastError: string | null;
}

/**
 * Same semantics as the PostgreSQL store. Each operation yields to the event
 * loop first (like a real round-trip) and then runs synchronously, so it is
 * atomic with respect to concurrent callers - mirroring row-level atomicity.
 */
export class InMemoryIdempotencyStore implements IdempotencyStore {
  readonly records = new Map<string, MemoryRecord>();

  constructor(private readonly now: () => number = Date.now) {}

  async claim(key: string, _meta: ClaimMetadata, leaseMs: number): Promise<ClaimResult> {
    await Promise.resolve();
    const now = this.now();
    const record = this.records.get(key);

    if (!record) {
      this.records.set(key, {
        status: 'processing',
        attempt: 1,
        sideEffectsStarted: false,
        leaseExpiresAt: now + leaseMs,
        responseStatus: null,
        lastError: null,
      });
      return { kind: 'acquired', attempt: 1 };
    }

    const leaseExpired = record.leaseExpiresAt < now;
    if (record.status === 'failed' || (record.status === 'processing' && leaseExpired && !record.sideEffectsStarted)) {
      record.status = 'processing';
      record.attempt += 1;
      record.leaseExpiresAt = now + leaseMs;
      record.lastError = null;
      return { kind: 'acquired', attempt: record.attempt };
    }

    if (record.status === 'completed') {
      return { kind: 'completed', responseStatus: record.responseStatus ?? 200 };
    }
    if (record.status === 'abandoned') {
      return { kind: 'abandoned' };
    }
    if (leaseExpired && record.sideEffectsStarted) {
      record.status = 'abandoned';
      record.lastError = 'lease expired after side effects started; manual reconciliation required';
      return { kind: 'abandoned' };
    }
    return { kind: 'in_progress' };
  }

  async markSideEffectsStarted(key: string, attempt: number): Promise<boolean> {
    await Promise.resolve();
    const record = this.owned(key, attempt);
    if (!record) return false;
    record.sideEffectsStarted = true;
    return true;
  }

  async complete(key: string, attempt: number, responseStatus: number): Promise<void> {
    await Promise.resolve();
    const record = this.owned(key, attempt);
    if (!record) return;
    record.status = 'completed';
    record.responseStatus = responseStatus;
  }

  async fail(key: string, attempt: number, error: string): Promise<void> {
    await Promise.resolve();
    const record = this.owned(key, attempt);
    if (!record) return;
    record.status = 'failed';
    record.lastError = error;
  }

  private owned(key: string, attempt: number): MemoryRecord | undefined {
    const record = this.records.get(key);
    return record && record.attempt === attempt && record.status === 'processing' ? record : undefined;
  }
}

// ── Store selection ──────────────────────────────────────────────────────────

let activeStore: IdempotencyStore = new PostgresIdempotencyStore();

export function getIdempotencyStore(): IdempotencyStore {
  return activeStore;
}

/** Swaps the backing store (tests). */
export function setIdempotencyStore(store: IdempotencyStore): void {
  activeStore = store;
}

export const WEBHOOK_LEASE_MS = parseInt(process.env.WHATSAPP_WEBHOOK_LEASE_MS || '120000', 10);
