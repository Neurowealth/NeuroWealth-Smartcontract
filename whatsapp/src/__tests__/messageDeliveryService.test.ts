/**
 * Unit tests for messageDeliveryService.ts (Issue #873).
 *
 * The service depends on `db` (a real pg Pool at module load) and on
 * cryptoUtils (which requires ENCRYPTION_KEY / PHONE_HASH_SALT). We stub the
 * db module with an in-memory fake table via the require cache, the same
 * technique the existing webhook tests use for `twilio`.
 */

process.env.ENCRYPTION_KEY = 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT = 'test-phone-salt';

// ── In-memory fake for `db.query`, keyed by the exact SQL shapes this
//    service issues ────────────────────────────────────────────────────────
interface FakeRow {
  message_sid: string;
  phone_hash: string | null;
  to_encrypted: string | null;
  to_iv: string | null;
  to_tag: string | null;
  status: string;
  error_code: string | null;
  error_reason: string | null;
  retry_count: number;
  escalated_at: string | null;
}

const table = new Map<string, FakeRow>();

function resetTable() {
  table.clear();
}

async function fakeQuery(sql: string, params: any[] = []) {
  const normalized = sql.replace(/\s+/g, ' ').trim().toUpperCase();

  if (normalized.startsWith('SELECT STATUS FROM')) {
    const [sid] = params;
    const row = table.get(sid);
    return { rows: row ? [{ status: row.status }] : [] };
  }

  if (normalized.startsWith('SELECT STATUS, TO_ENCRYPTED')) {
    const [sid] = params;
    const row = table.get(sid);
    return {
      rows: row
        ? [
            {
              status: row.status,
              to_encrypted: row.to_encrypted,
              to_iv: row.to_iv,
              to_tag: row.to_tag,
              retry_count: row.retry_count,
            },
          ]
        : [],
    };
  }

  if (normalized.startsWith('INSERT INTO WHATSAPP_MESSAGE_DELIVERIES')) {
    const [messageSid, phoneHash, toEncrypted, toIv, toTag, status, errorCode, errorReason] = params;
    if (!table.has(messageSid)) {
      table.set(messageSid, {
        message_sid: messageSid,
        phone_hash: phoneHash,
        to_encrypted: toEncrypted,
        to_iv: toIv,
        to_tag: toTag,
        status,
        error_code: errorCode,
        error_reason: errorReason,
        retry_count: 0,
        escalated_at: null,
      });
    }
    return { rows: [] };
  }

  if (normalized.includes('SET STATUS =')) {
    const [messageSid, status, errorCode, errorReason] = params;
    const row = table.get(messageSid);
    if (row) {
      row.status = status;
      row.error_code = errorCode;
      row.error_reason = errorReason;
    }
    return { rows: [] };
  }

  if (normalized.includes('SET RETRY_COUNT')) {
    const [messageSid] = params;
    const row = table.get(messageSid);
    if (row) row.retry_count += 1;
    return { rows: [] };
  }

  if (normalized.includes('SET ESCALATED_AT')) {
    const [messageSid] = params;
    const row = table.get(messageSid);
    if (row && !row.escalated_at) row.escalated_at = new Date().toISOString();
    return { rows: [] };
  }

  throw new Error(`fakeQuery: unrecognized SQL: ${sql}`);
}

const dbModulePath = require.resolve('../db');
require.cache[dbModulePath] = {
  id: dbModulePath,
  filename: dbModulePath,
  loaded: true,
  exports: { query: fakeQuery },
  parent: null,
  children: [],
  paths: [],
} as any;

import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import {
  applyDeliveryStatus,
  redactErrorMessage,
  retryFailedDelivery,
} from '../messageDeliveryService';

let sidCounter = 0;
function nextSid(): string {
  return `SM${String(++sidCounter).padStart(10, '0')}`;
}

beforeEach(() => {
  resetTable();
});

describe('redactErrorMessage', () => {
  test('strips phone-number-like substrings', () => {
    const redacted = redactErrorMessage('Could not deliver to +15550001234, unknown recipient');
    assert.equal(redacted, 'Could not deliver to [redacted], unknown recipient');
  });

  test('passes through text with no phone number', () => {
    assert.equal(redactErrorMessage('Generic failure'), 'Generic failure');
  });

  test('returns null for empty input', () => {
    assert.equal(redactErrorMessage(undefined), null);
    assert.equal(redactErrorMessage(null), null);
    assert.equal(redactErrorMessage(''), null);
  });
});

describe('applyDeliveryStatus', () => {
  test('ignores a callback with no MessageSid', async () => {
    const result = await applyDeliveryStatus({ messageSid: '', status: 'sent' });
    assert.deepEqual(result, { applied: false, reason: 'missing_message_sid' });
  });

  test('ignores a callback with an unrecognized status', async () => {
    const sid = nextSid();
    const result = await applyDeliveryStatus({ messageSid: sid, status: 'read' });
    assert.deepEqual(result, { applied: false, reason: 'unknown_status' });
    assert.equal(table.has(sid), false);
  });

  test('creates a record on the first callback seen for a MessageSid', async () => {
    const sid = nextSid();
    const result = await applyDeliveryStatus({ messageSid: sid, status: 'queued', to: 'whatsapp:+15550009999' });
    assert.deepEqual(result, { applied: true, created: true });
    assert.equal(table.get(sid)!.status, 'queued');
    assert.ok(table.get(sid)!.phone_hash);
  });

  test('advances status forward on a later callback', async () => {
    const sid = nextSid();
    await applyDeliveryStatus({ messageSid: sid, status: 'queued' });
    const result = await applyDeliveryStatus({ messageSid: sid, status: 'sent' });
    assert.deepEqual(result, { applied: true, created: false });
    assert.equal(table.get(sid)!.status, 'sent');
  });

  test('does not regress a terminal delivered state on a stale out-of-order callback', async () => {
    const sid = nextSid();
    await applyDeliveryStatus({ messageSid: sid, status: 'queued' });
    await applyDeliveryStatus({ messageSid: sid, status: 'sent' });
    await applyDeliveryStatus({ messageSid: sid, status: 'delivered' });

    const stale = await applyDeliveryStatus({ messageSid: sid, status: 'sent' });
    assert.deepEqual(stale, { applied: false, reason: 'stale_status' });
    assert.equal(table.get(sid)!.status, 'delivered');
  });

  test('a duplicate terminal callback is idempotent', async () => {
    const sid = nextSid();
    await applyDeliveryStatus({ messageSid: sid, status: 'delivered' });
    await applyDeliveryStatus({ messageSid: sid, status: 'delivered' });
    assert.equal(table.get(sid)!.status, 'delivered');
  });

  test('persists a redacted failure reason', async () => {
    const sid = nextSid();
    await applyDeliveryStatus({
      messageSid: sid,
      status: 'failed',
      errorCode: '30003',
      errorMessage: 'Unreachable destination handset +15550001234',
    });
    const row = table.get(sid)!;
    assert.equal(row.error_code, '30003');
    assert.equal(row.error_reason, 'Unreachable destination handset [redacted]');
  });
});

describe('retryFailedDelivery', () => {
  test('is a no-op for an unknown MessageSid', async () => {
    const outcome = await retryFailedDelivery(nextSid(), async () => {});
    assert.deepEqual(outcome, { retried: false, escalated: false });
  });

  test('is a no-op for a non-terminal status', async () => {
    const sid = nextSid();
    await applyDeliveryStatus({ messageSid: sid, status: 'sent', to: 'whatsapp:+15550009999' });

    let resendCalled = false;
    const outcome = await retryFailedDelivery(sid, async () => {
      resendCalled = true;
    });

    assert.deepEqual(outcome, { retried: false, escalated: false });
    assert.equal(resendCalled, false);
  });

  test('resends to the original recipient and increments the retry count', async () => {
    const sid = nextSid();
    await applyDeliveryStatus({ messageSid: sid, status: 'failed', to: 'whatsapp:+15550009999' });

    let resentTo: string | null = null;
    const outcome = await retryFailedDelivery(sid, async (to) => {
      resentTo = to;
    });

    assert.deepEqual(outcome, { retried: true, escalated: false });
    assert.equal(resentTo, 'whatsapp:+15550009999');
    assert.equal(table.get(sid)!.retry_count, 1);
  });

  test('escalates instead of retrying once the retry budget is exhausted', async () => {
    const sid = nextSid();
    await applyDeliveryStatus({ messageSid: sid, status: 'failed', to: 'whatsapp:+15550009999' });

    let resendCount = 0;
    for (let i = 0; i < 3; i++) {
      await retryFailedDelivery(sid, async () => {
        resendCount += 1;
      });
    }
    assert.equal(resendCount, 3);
    assert.equal(table.get(sid)!.retry_count, 3);

    let escalated = false;
    const outcome = await retryFailedDelivery(
      sid,
      async () => {
        resendCount += 1;
      },
      () => {
        escalated = true;
      }
    );

    assert.deepEqual(outcome, { retried: false, escalated: true });
    assert.equal(escalated, true);
    assert.equal(resendCount, 3, 'must not resend once escalated');
    assert.ok(table.get(sid)!.escalated_at);
  });

  test('escalates immediately when no recipient was ever captured', async () => {
    const sid = nextSid();
    await applyDeliveryStatus({ messageSid: sid, status: 'undelivered' });

    let escalated = false;
    const outcome = await retryFailedDelivery(
      sid,
      async () => {
        throw new Error('should not be called');
      },
      () => {
        escalated = true;
      }
    );

    assert.deepEqual(outcome, { retried: false, escalated: true });
    assert.equal(escalated, true);
  });
});
