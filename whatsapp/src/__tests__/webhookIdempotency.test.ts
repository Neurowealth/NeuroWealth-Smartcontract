/**
 * Idempotency tests for inbound WhatsApp webhooks.
 *
 * Twilio may redeliver the same inbound message after a timeout. These tests
 * drive handleWhatsAppWebhook with repeated MessageSids and verify that the
 * command runs at most once across sequential, concurrent and delayed
 * duplicates, and that retryable failures resume without repeating side
 * effects. Stubs twilio and db the same way webhookConcurrency.test.ts does.
 */

process.env.ENCRYPTION_KEY = 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT = 'test-phone-salt';
process.env.SOROBAN_RPC_URL = 'https://soroban-testnet.stellar.org';
process.env.VAULT_CONTRACT_ID = 'CTEST_VAULT';
process.env.WHATSAPP_WEBHOOK_LEASE_MS = '60000';

import path from 'node:path';

// ── Stub twilio's MessagingResponse, recording every reply message ──────────
const sentReplies: string[] = [];

class FakeMessagingResponse {
  private text = '';
  message(t: string) {
    this.text = t;
    sentReplies.push(t);
  }
  toString() {
    return `<Response><Message>${this.text}</Message></Response>`;
  }
}

const twilioStub = { twiml: { MessagingResponse: FakeMessagingResponse } };
for (const key of [
  require.resolve('twilio'),
  path.join(path.dirname(require.resolve('twilio')), 'index.js'),
]) {
  try {
    (require as any).cache[key] = {
      id: key, filename: key, loaded: true,
      exports: twilioStub, parent: null, children: [], paths: [],
    };
  } catch { /* ignore resolve errors */ }
}

// ── Stub db with an in-memory whatsapp_wallets table and fault injection ────
const wallets = new Map<string, { public_key: string; encrypted_data: string; iv: string; tag: string }>();
const faults: { select: number; insert: number } = { select: 0, insert: 0 };

function transientError(): Error {
  return Object.assign(new Error('connect ECONNREFUSED 127.0.0.1:5432'), { code: 'ECONNREFUSED' });
}

async function fakeQuery(sql: string, params: any[] = []) {
  const normalized = sql.replace(/\s+/g, ' ').trim().toUpperCase();

  if (normalized.startsWith('INSERT INTO WHATSAPP_WALLETS')) {
    if (faults.insert > 0) {
      faults.insert -= 1;
      throw transientError();
    }
    const [phoneHash, publicKey, encryptedData, iv, tag] = params;
    if (!wallets.has(phoneHash)) {
      wallets.set(phoneHash, { public_key: publicKey, encrypted_data: encryptedData, iv, tag });
    }
    return { rows: [] };
  }

  if (normalized.startsWith('SELECT * FROM WHATSAPP_WALLETS')) {
    if (faults.select > 0) {
      faults.select -= 1;
      throw transientError();
    }
    const [phoneHash] = params;
    const row = wallets.get(phoneHash);
    return { rows: row ? [{ ...row, created_at: new Date().toISOString() }] : [] };
  }

  throw new Error(`fakeQuery: unrecognized SQL: ${sql}`);
}

const dbModulePath = require.resolve('../db');
require.cache[dbModulePath] = {
  id: dbModulePath, filename: dbModulePath, loaded: true,
  exports: { query: fakeQuery }, parent: null, children: [], paths: [],
} as any;

import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { handleWhatsAppWebhook } from '../webhook';
import { createCustodialWallet } from '../walletService';
import { updateState, UserState } from '../stateManager';
import { InMemoryStateStore, setStateStore } from '../stateStore';
import { hashPhoneNumber } from '../cryptoUtils';
import { smsClient } from '../smsService';
import {
  deriveIdempotencyKey,
  InMemoryIdempotencyStore,
  isTransientError,
  RetryableWebhookError,
  setIdempotencyStore,
} from '../webhookIdempotency';

const ACCOUNT_SID = 'ACtest';
const LEASE_MS = 60_000;

let clock = Date.now();
let store: InMemoryIdempotencyStore;

// OTP codes are delivered via SMS only (#942); capture them from a stubbed
// SMS client instead of the chat reply.
let smsCodes: string[] = [];
function lastSmsCode(): string | undefined {
  return smsCodes[smsCodes.length - 1];
}

beforeEach(() => {
  clock = Date.now();
  store = new InMemoryIdempotencyStore(() => clock);
  setStateStore(new InMemoryStateStore());
  setIdempotencyStore(store);
  faults.select = 0;
  faults.insert = 0;
  smsCodes = [];
  smsClient.send = async (_to: string, body: string) => {
    const match = /\d{6}/.exec(body);
    if (match) smsCodes.push(match[0]);
  };
});

let phoneCounter = 0;
function nextPhone(): string {
  return `+1666${String(++phoneCounter).padStart(7, '0')}`;
}

let sidCounter = 0;
function nextSid(): string {
  return `SM${String(++sidCounter).padStart(32, '0')}`;
}

function keyFor(sid: string): string {
  return deriveIdempotencyKey({ AccountSid: ACCOUNT_SID, MessageSid: sid }) as string;
}

function makeReq(from: string, body: string, messageSid: string) {
  return {
    body: { From: `whatsapp:${from}`, Body: body, MessageSid: messageSid, AccountSid: ACCOUNT_SID },
    headers: {},
  } as any;
}

interface Captured {
  status: number;
  body: string;
}

async function deliver(from: string, body: string, messageSid: string): Promise<Captured> {
  const captured: Captured = { status: 200, body: '' };
  const res: any = {
    type: () => res,
    status: (s: number) => {
      captured.status = s;
      return res;
    },
    send: (b: string) => {
      captured.body = b;
      return res;
    },
  };
  await handleWhatsAppWebhook(makeReq(from, body, messageSid), res);
  return captured;
}

function countReplies(pattern: RegExp): number {
  return sentReplies.filter((r) => pattern.test(r)).length;
}

function hasNoMessage(captured: Captured): boolean {
  return !captured.body.includes('<Message>');
}

async function verifiedPhone(): Promise<string> {
  const phone = nextPhone();
  const phoneHash = hashPhoneNumber(`whatsapp:${phone}`);
  await updateState(phoneHash, UserState.VERIFIED);
  await createCustodialWallet(phoneHash);
  return phone;
}

describe('deriveIdempotencyKey', () => {
  test('is derived from the provider message id, scoped by account and event', () => {
    const key = deriveIdempotencyKey({ AccountSid: 'AC1', MessageSid: 'SM1' });
    assert.equal(key, 'twilio:inbound-message:AC1:SM1');
    assert.notEqual(key, deriveIdempotencyKey({ AccountSid: 'AC2', MessageSid: 'SM1' }));
    assert.equal(key, deriveIdempotencyKey({ AccountSid: 'AC1', MessageSid: 'SM1', Body: 'different' }));
  });

  test('falls back to the Twilio idempotency header, else null', () => {
    assert.equal(
      deriveIdempotencyKey({ AccountSid: 'AC1' }, 'tok-123'),
      'twilio:inbound-message:AC1:token:tok-123'
    );
    assert.equal(deriveIdempotencyKey({}, undefined), null);
    assert.equal(deriveIdempotencyKey({ MessageSid: '   ' }, ''), null);
  });

  test('classifies transient infrastructure errors as retryable', () => {
    assert.ok(isTransientError(transientError()));
    assert.ok(isTransientError(Object.assign(new Error('x'), { code: '08006' })));
    assert.ok(isTransientError(new RetryableWebhookError('x')));
    assert.ok(!isTransientError(new Error('Wallet not found')));
    assert.ok(!isTransientError(Object.assign(new Error('x'), { code: '23505' })));
  });
});

describe('inbound webhook idempotency', () => {
  test('sequential duplicate after success returns success without a second reply', async () => {
    const phone = await verifiedPhone();
    const sid = nextSid();

    const first = await deliver(phone, 'switch to growth', sid);
    const second = await deliver(phone, 'switch to growth', sid);

    assert.equal(first.status, 200);
    assert.match(first.body, /Strategy updated to GROWTH/);
    assert.equal(second.status, 200);
    assert.ok(hasNoMessage(second), `duplicate must not reply, got ${second.body}`);
    assert.equal(store.records.get(keyFor(sid))?.attempt, 1);
  });

  test('distinct messages with the same text are not deduplicated', async () => {
    const phone = await verifiedPhone();
    const before = countReplies(/Deposited 15 USDC/);

    await deliver(phone, 'deposit 15 USDC', nextSid());
    await deliver(phone, 'deposit 15 USDC', nextSid());

    assert.equal(countReplies(/Deposited 15 USDC/) - before, 2);
  });

  test('concurrent deliveries of the same webhook execute the command at most once', async () => {
    const phone = await verifiedPhone();
    const sid = nextSid();
    const before = countReplies(/Strategy updated to CONSERVATIVE/);

    const results = await Promise.all(
      Array.from({ length: 5 }, () => deliver(phone, 'switch to conservative', sid))
    );

    assert.equal(countReplies(/Strategy updated to CONSERVATIVE/) - before, 1);
    assert.equal(results.filter((r) => /Strategy updated/.test(r.body)).length, 1);
    assert.ok(results.every((r) => r.status === 200));
    assert.equal(store.records.get(keyFor(sid))?.attempt, 1);
  });

  test('concurrent claims across workers grant the key to exactly one', async () => {
    const claims = await Promise.all(
      Array.from({ length: 10 }, () => store.claim('k-concurrent', { messageSid: 'SMx', phoneHash: 'h' }, LEASE_MS))
    );
    assert.equal(claims.filter((c) => c.kind === 'acquired').length, 1);
    assert.equal(claims.filter((c) => c.kind === 'in_progress').length, 9);
  });

  test('a duplicate while another worker holds the lease is rejected as retryable, then acknowledged', async () => {
    const phone = await verifiedPhone();
    const sid = nextSid();
    const key = keyFor(sid);

    // Another instance claimed this delivery and is still processing it.
    const other = await store.claim(key, { messageSid: sid, phoneHash: 'h' }, LEASE_MS);
    assert.equal(other.kind, 'acquired');

    const during = await deliver(phone, 'withdraw 5', sid);
    assert.equal(during.status, 409);
    assert.ok(hasNoMessage(during));

    if (other.kind === 'acquired') await store.complete(key, other.attempt, 200);

    const after = await deliver(phone, 'withdraw 5', sid);
    assert.equal(after.status, 200);
    assert.ok(hasNoMessage(after));
  });

  test('delayed duplicate long after success is still not re-executed', async () => {
    const phone = await verifiedPhone();
    const sid = nextSid();
    const before = countReplies(/Deposited 25 USDC/);

    await deliver(phone, 'deposit 25 USDC', sid);
    clock += 24 * 60 * 60 * 1000; // a day later, far past the lease

    const late = await deliver(phone, 'deposit 25 USDC', sid);
    assert.equal(late.status, 200);
    assert.ok(hasNoMessage(late));
    assert.equal(countReplies(/Deposited 25 USDC/) - before, 1);
  });

  test('retryable failure before side effects returns 503 and resumes on redelivery', async () => {
    const phone = await verifiedPhone();
    const sid = nextSid();
    const before = countReplies(/Deposited 40 USDC/);

    faults.select = 1; // getWallet hits a transient DB outage
    const failed = await deliver(phone, 'deposit 40 USDC', sid);
    assert.equal(failed.status, 503);
    assert.ok(hasNoMessage(failed));
    assert.equal(store.records.get(keyFor(sid))?.status, 'failed');
    assert.equal(countReplies(/Deposited 40 USDC/) - before, 0);

    const retried = await deliver(phone, 'deposit 40 USDC', sid);
    assert.equal(retried.status, 200);
    assert.match(retried.body, /Deposited 40 USDC/);

    const duplicate = await deliver(phone, 'deposit 40 USDC', sid);
    assert.ok(hasNoMessage(duplicate));
    assert.equal(countReplies(/Deposited 40 USDC/) - before, 1);
    assert.equal(store.records.get(keyFor(sid))?.attempt, 2);
  });

  test('a failure after side effects started is terminal and never re-run', async () => {
    const phone = nextPhone();

    await deliver(phone, 'hi', nextSid());
    const otp = lastSmsCode();
    assert.ok(otp, 'expected an OTP to be sent via SMS');

    // OTP is consumed, then wallet creation hits a transient error.
    const sid = nextSid();
    faults.insert = 1;
    const first = await deliver(phone, otp, sid);
    assert.equal(first.status, 200);
    assert.match(first.body, /An error occurred/);
    assert.equal(store.records.get(keyFor(sid))?.status, 'completed');

    const duplicate = await deliver(phone, otp, sid);
    assert.equal(duplicate.status, 200);
    assert.ok(hasNoMessage(duplicate));
  });

  test('delayed duplicate after a crash before side effects re-runs the command', async () => {
    const phone = await verifiedPhone();
    const sid = nextSid();
    const key = keyFor(sid);

    // A worker claimed the delivery and died before doing anything.
    await store.claim(key, { messageSid: sid, phoneHash: 'h' }, LEASE_MS);
    clock += LEASE_MS + 1;

    const resumed = await deliver(phone, 'switch to growth', sid);
    assert.equal(resumed.status, 200);
    assert.match(resumed.body, /Strategy updated to GROWTH/);
    assert.equal(store.records.get(key)?.attempt, 2);
    assert.equal(store.records.get(key)?.status, 'completed');
  });

  test('delayed duplicate after a crash mid side-effect is abandoned, not replayed', async () => {
    const phone = await verifiedPhone();
    const sid = nextSid();
    const key = keyFor(sid);
    const before = countReplies(/Withdrew 7 USDC/);

    const crashed = await store.claim(key, { messageSid: sid, phoneHash: 'h' }, LEASE_MS);
    if (crashed.kind === 'acquired') await store.markSideEffectsStarted(key, crashed.attempt);
    clock += LEASE_MS + 1;

    const late = await deliver(phone, 'withdraw 7', sid);
    assert.equal(late.status, 200);
    assert.ok(hasNoMessage(late));
    assert.equal(store.records.get(key)?.status, 'abandoned');
    assert.equal(countReplies(/Withdrew 7 USDC/) - before, 0);
  });

  test('a stale worker cannot complete over a newer attempt', async () => {
    const key = 'k-fencing';
    const meta = { messageSid: 'SMf', phoneHash: 'h' };
    const stale = await store.claim(key, meta, LEASE_MS);
    clock += LEASE_MS + 1;
    const fresh = await store.claim(key, meta, LEASE_MS);
    assert.equal(stale.kind, 'acquired');
    assert.equal(fresh.kind, 'acquired');

    if (stale.kind === 'acquired') {
      assert.equal(await store.markSideEffectsStarted(key, stale.attempt), false);
      await store.complete(key, stale.attempt, 200);
    }
    assert.equal(store.records.get(key)?.status, 'processing');
  });
});

describe('replayed verification-flow webhooks (#945)', () => {
  test('a replayed greeting does not issue a second OTP', async () => {
    const phone = nextPhone();
    const sid = nextSid();

    await deliver(phone, 'hi', sid);
    const code = lastSmsCode();
    assert.ok(code, 'expected an OTP to be sent via SMS');

    const replay = await deliver(phone, 'hi', sid);
    assert.equal(replay.status, 200);
    assert.ok(hasNoMessage(replay), 'a replay must not reveal or reissue an OTP');
    assert.equal(smsCodes.length, 1, 'a replay must not send a second OTP SMS');

    // The original code is still the live one (a reissue would have replaced it).
    const verified = await deliver(phone, code, nextSid());
    assert.match(verified.body, /Phone number verified/);
  });

  test('a replayed OTP submission does not verify twice or create a second wallet', async () => {
    const phone = nextPhone();
    await deliver(phone, 'hi', nextSid());
    const code = lastSmsCode() as string;

    const sid = nextSid();
    const before = countReplies(/Phone number verified/);
    const first = await deliver(phone, code, sid);
    assert.match(first.body, /Phone number verified/);

    const replay = await deliver(phone, code, sid);
    assert.equal(replay.status, 200);
    assert.ok(hasNoMessage(replay));
    assert.equal(countReplies(/Phone number verified/) - before, 1);
    assert.equal(wallets.size > 0, true);
  });

  test('a replay of a failed OTP guess does not burn an extra attempt', async () => {
    const phone = nextPhone();
    await deliver(phone, 'hi', nextSid());
    const code = lastSmsCode() as string;
    const wrong = code === '000000' ? '111111' : '000000';

    const sid = nextSid();
    const first = await deliver(phone, wrong, sid);
    assert.match(first.body, /2 attempts remaining/);

    // Twilio redelivers the same message three more times.
    for (let i = 0; i < 3; i += 1) {
      const replay = await deliver(phone, wrong, sid);
      assert.ok(hasNoMessage(replay));
    }

    // Still two attempts left and the real code works.
    const second = await deliver(phone, wrong, nextSid());
    assert.match(second.body, /1 attempt/);
    const ok = await deliver(phone, code, nextSid());
    assert.match(ok.body, /Phone number verified/);
  });
});
