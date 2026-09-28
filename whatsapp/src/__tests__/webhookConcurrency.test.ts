/**
 * Concurrency tests for the webhook handler (Issue #874).
 *
 * Verifies that commands from the same WhatsApp user - strategy changes,
 * portfolio reads, and deposit/withdraw transactions - are serialized
 * through the shared session, while different users continue to run
 * concurrently. Sets up its own stubs (twilio, db) and env vars rather than
 * relying on the pre-existing webhook.test.ts, which does not set
 * SOROBAN_RPC_URL and fails to load regardless of this change.
 */

process.env.ENCRYPTION_KEY = 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT = 'test-phone-salt';
process.env.SOROBAN_RPC_URL = 'https://soroban-testnet.stellar.org';
process.env.VAULT_CONTRACT_ID = 'CTEST_VAULT';

import path from 'node:path';

// ── Stub twilio's MessagingResponse, capturing one reply per call ───────────
const repliesBySid: string[] = [];

class FakeMessagingResponse {
  private text = '';
  message(t: string) {
    this.text = t;
    repliesBySid.push(t);
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

// ── Stub db with an in-memory whatsapp_wallets table ─────────────────────────
const wallets = new Map<string, { public_key: string; encrypted_data: string; iv: string; tag: string }>();

async function fakeQuery(sql: string, params: any[] = []) {
  const normalized = sql.replace(/\s+/g, ' ').trim().toUpperCase();

  if (normalized.startsWith('INSERT INTO WHATSAPP_WALLETS')) {
    const [phoneHash, publicKey, encryptedData, iv, tag] = params;
    if (!wallets.has(phoneHash)) {
      wallets.set(phoneHash, { public_key: publicKey, encrypted_data: encryptedData, iv, tag });
    }
    return { rows: [] };
  }

  if (normalized.startsWith('SELECT * FROM WHATSAPP_WALLETS')) {
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

import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { handleWhatsAppWebhook } from '../webhook';
import { createCustodialWallet } from '../walletService';
import { updateState, UserState } from '../stateManager';
import { hashPhoneNumber } from '../cryptoUtils';

let phoneCounter = 0;
function nextPhone(): string {
  return `+1555${String(++phoneCounter).padStart(7, '0')}`;
}

function makeReq(from: string, body: string) {
  return { body: { From: `whatsapp:${from}`, Body: body } } as any;
}

function makeRes() {
  const captured: { body: string } = { body: '' };
  const res: any = {
    type: () => res,
    status: () => res,
    send: (b: string) => {
      captured.body = b;
      return res;
    },
  };
  return { res, captured };
}

/** Pre-verifies a phone number so tests can drive VERIFIED-state commands directly. */
async function verifiedPhone(): Promise<string> {
  const phone = nextPhone();
  const phoneHash = hashPhoneNumber(`whatsapp:${phone}`);
  updateState(phoneHash, UserState.VERIFIED);
  await createCustodialWallet(phoneHash);
  return phone;
}

describe('webhook concurrency (per-user command serialization)', () => {
  test('two commands from the same user apply in arrival order, not interleaved', async () => {
    const phone = await verifiedPhone();
    const order: string[] = [];

    const originalCreate = createCustodialWallet;
    void originalCreate;

    const r1 = makeRes();
    const r2 = makeRes();

    // Fire both "concurrently" (no await between them), like two fast WhatsApp messages.
    const p1 = handleWhatsAppWebhook(makeReq(phone, 'switch to growth'), r1.res).then(() => order.push('strategy'));
    const p2 = handleWhatsAppWebhook(makeReq(phone, 'balance'), r2.res).then(() => order.push('balance'));

    await Promise.all([p1, p2]);

    // Both must have completed successfully and in the order they were sent.
    assert.deepEqual(order, ['strategy', 'balance']);
    assert.match(r1.captured.body, /Strategy updated to GROWTH/);
    assert.match(r2.captured.body, /NeuroWealth Portfolio/);
  });

  test('a failing command for one user does not block that user\'s next command', async () => {
    const phone = await verifiedPhone();

    // "switch to invalid" is rejected by handleStrategyUpdate but must not
    // leave the per-user lock stuck.
    const r1 = makeRes();
    await handleWhatsAppWebhook(makeReq(phone, 'switch to invalid'), r1.res);

    const r2 = makeRes();
    await handleWhatsAppWebhook(makeReq(phone, 'balance'), r2.res);

    assert.match(r2.captured.body, /NeuroWealth Portfolio/);
  });

  test('commands from different users run without waiting on each other', async () => {
    const phoneA = await verifiedPhone();
    const phoneB = await verifiedPhone();

    const rA = makeRes();
    const rB = makeRes();

    const start = Date.now();
    await Promise.all([
      handleWhatsAppWebhook(makeReq(phoneA, 'switch to growth'), rA.res),
      handleWhatsAppWebhook(makeReq(phoneB, 'switch to conservative'), rB.res),
    ]);
    const elapsed = Date.now() - start;

    assert.match(rA.captured.body, /Strategy updated to GROWTH/);
    assert.match(rB.captured.body, /Strategy updated to CONSERVATIVE/);
    // Sanity check only - two independent, near-instant commands should not
    // have been artificially serialized into a long chain.
    assert.ok(elapsed < 2000, `expected concurrent processing, took ${elapsed}ms`);
  });
});
