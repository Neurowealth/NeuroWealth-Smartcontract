import pino from 'pino';
import { query } from './db';
import { hashPhoneNumber, encryptSecretKey, decryptSecretKey } from './cryptoUtils';

const logger = pino({ name: 'whatsapp-message-delivery' });

/**
 * Delivery states Twilio reports for an outbound WhatsApp message, in the
 * order they can legitimately progress. `failed` and `undelivered` are both
 * terminal and share the same rank as `delivered` - once any of the three is
 * reached, the record no longer moves.
 */
export type MessageDeliveryStatus = 'queued' | 'sent' | 'delivered' | 'failed' | 'undelivered';

const STATUS_RANK: Record<MessageDeliveryStatus, number> = {
  queued: 0,
  sent: 1,
  delivered: 2,
  failed: 2,
  undelivered: 2,
};

function isKnownStatus(value: string): value is MessageDeliveryStatus {
  return Object.prototype.hasOwnProperty.call(STATUS_RANK, value);
}

function isTerminalFailure(status: MessageDeliveryStatus): boolean {
  return status === 'failed' || status === 'undelivered';
}

// Matches phone-number-like runs of digits (with optional +, spaces, dashes,
// parens) so provider error text never ends up storing a raw MSISDN.
const PHONE_LIKE_PATTERN = /\+?\d[\d\s().-]{6,}\d/g;

/** Strips phone-number-like substrings from provider error text before it is persisted. */
export function redactErrorMessage(message: string | null | undefined): string | null {
  if (!message) return null;
  return message.replace(PHONE_LIKE_PATTERN, '[redacted]');
}

export interface DeliveryStatusCallback {
  messageSid: string;
  status: string;
  to?: string;
  errorCode?: string;
  errorMessage?: string;
}

export type ApplyResult =
  | { applied: true; created: boolean }
  | { applied: false; reason: 'missing_message_sid' | 'unknown_status' | 'stale_status' };

/**
 * Applies a Twilio delivery-status callback to the reconciliation ledger.
 *
 * Idempotent and order-safe: a callback can only ever move a record's status
 * forward (queued -> sent -> terminal). A duplicate or out-of-order callback
 * (e.g. a delayed "sent" arriving after "delivered" was already recorded) is
 * a no-op rather than regressing a terminal state. The very first callback
 * seen for a MessageSid creates its record - Twilio never gives us a chance
 * to pre-register one, since TwiML-generated replies don't expose their Sid
 * until Twilio assigns it after we've already responded.
 */
export async function applyDeliveryStatus(callback: DeliveryStatusCallback): Promise<ApplyResult> {
  const { messageSid, status, to, errorCode, errorMessage } = callback;

  if (!messageSid) {
    logger.warn({ status, to }, 'Ignoring delivery status callback with no MessageSid');
    return { applied: false, reason: 'missing_message_sid' };
  }

  if (!isKnownStatus(status)) {
    logger.warn({ messageSid, status }, 'Ignoring delivery status callback with an unrecognized status');
    return { applied: false, reason: 'unknown_status' };
  }

  const redactedError = redactErrorMessage(errorMessage);
  const existing = await query(
    'SELECT status FROM whatsapp_message_deliveries WHERE message_sid = $1',
    [messageSid]
  );

  if (existing.rows.length === 0) {
    const phoneHash = to ? hashPhoneNumber(to) : null;
    const encryptedTo = to ? encryptSecretKey(to) : null;

    await query(
      `INSERT INTO whatsapp_message_deliveries
         (message_sid, phone_hash, to_encrypted, to_iv, to_tag, status, error_code, error_reason, retry_count, created_at, updated_at)
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, NOW(), NOW())
       ON CONFLICT (message_sid) DO NOTHING`,
      [
        messageSid,
        phoneHash,
        encryptedTo?.encryptedData ?? null,
        encryptedTo?.iv ?? null,
        encryptedTo?.tag ?? null,
        status,
        errorCode ?? null,
        redactedError,
      ]
    );

    logger.info({ messageSid, status }, 'Recorded new outbound message delivery status');
    return { applied: true, created: true };
  }

  const currentStatus = existing.rows[0].status as MessageDeliveryStatus;
  if (STATUS_RANK[status] < STATUS_RANK[currentStatus]) {
    logger.info(
      { messageSid, status, currentStatus },
      'Ignoring out-of-order delivery status callback; current state is already more advanced'
    );
    return { applied: false, reason: 'stale_status' };
  }

  await query(
    `UPDATE whatsapp_message_deliveries
       SET status = $2, error_code = $3, error_reason = $4, updated_at = NOW()
     WHERE message_sid = $1`,
    [messageSid, status, errorCode ?? null, redactedError]
  );

  logger.info({ messageSid, status, previousStatus: currentStatus }, 'Updated outbound message delivery status');
  return { applied: true, created: false };
}

const MAX_DELIVERY_RETRIES = 3;

export interface RetryOutcome {
  retried: boolean;
  escalated: boolean;
}

/**
 * Bounded retry for a terminally-failed delivery. The caller supplies the
 * actual resend action (e.g. a Twilio REST send) so this module stays
 * transport-agnostic and testable. After `MAX_DELIVERY_RETRIES` attempts we
 * stop retrying and escalate instead of retrying forever.
 */
export async function retryFailedDelivery(
  messageSid: string,
  resend: (to: string) => Promise<void>,
  onEscalate: (messageSid: string) => void = (sid) =>
    logger.error({ messageSid: sid }, 'Delivery failure escalated after exhausting retries')
): Promise<RetryOutcome> {
  const existing = await query(
    'SELECT status, to_encrypted, to_iv, to_tag, retry_count FROM whatsapp_message_deliveries WHERE message_sid = $1',
    [messageSid]
  );

  if (existing.rows.length === 0) {
    return { retried: false, escalated: false };
  }

  const row = existing.rows[0];
  const status = row.status as MessageDeliveryStatus;
  const retryCount = Number(row.retry_count) || 0;

  if (!isTerminalFailure(status)) {
    return { retried: false, escalated: false };
  }

  if (retryCount >= MAX_DELIVERY_RETRIES || !row.to_encrypted) {
    await query(
      'UPDATE whatsapp_message_deliveries SET escalated_at = NOW() WHERE message_sid = $1 AND escalated_at IS NULL',
      [messageSid]
    );
    onEscalate(messageSid);
    return { retried: false, escalated: true };
  }

  const to = decryptSecretKey(row.to_encrypted, row.to_iv, row.to_tag);

  await query(
    'UPDATE whatsapp_message_deliveries SET retry_count = retry_count + 1, updated_at = NOW() WHERE message_sid = $1',
    [messageSid]
  );

  await resend(to);

  return { retried: true, escalated: false };
}
