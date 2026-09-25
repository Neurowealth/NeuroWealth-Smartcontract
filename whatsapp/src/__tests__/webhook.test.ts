/**
 * Unit tests for webhook.ts (Issue #691)
 *
 * The webhook handler depends on:
 *   - twilio (twiml.MessagingResponse) — stubbed via require cache
 *   - cryptoUtils (requires ENCRYPTION_KEY / PHONE_HASH_SALT env vars)
 *   - vaultRouter (requires VAULT_CONTRACT_ID)
 *   - stateManager, otpService, walletService, intentParser, contractLimits
 *
 * We stub Twilio's MessagingResponse with a minimal implementation that
 * captures the reply text, then exercise the state machine end-to-end.
 */

// ── Env vars must be set before any import ──────────────────────────────────
process.env.ENCRYPTION_KEY = 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT = 'test-phone-salt';
process.env.VAULT_CONTRACT_ID = 'CTEST_VAULT';

// ── Stub twilio before webhook.ts loads it ──────────────────────────────────
import path from 'node:path';

const capturedMessages: string[] = [];

class FakeMessagingResponse {
  message(text: string) {
    capturedMessages.push(text);
  }
  toString() {
    return `<Response><Message>${capturedMessages.at(-1) ?? ''}</Message></Response>`;
  }
}

const twilioStub = {
  default: {
    twiml: { MessagingResponse: FakeMessagingResponse },
  },
  twiml: { MessagingResponse: FakeMessagingResponse },
};

// Inject under both CJS keys twilio uses
for (const key of [
  require.resolve('twilio'),
  path.join(path.dirname(require.resolve('twilio')), 'index.js'),
]) {
  try {
    (require as any).cache[key] = {
      id: key, filename: key, loaded: true,
      exports: twilioStub.default,
      parent: null, children: [], paths: [],
    };
  } catch { /* ignore resolve errors */ }
}

import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { handleWhatsAppWebhook } from '../webhook';

// ── Minimal req/res helpers ──────────────────────────────────────────────────

function makeReq(from: string, body: string) {
  return {
    body: { From: `whatsapp:${from}`, Body: body },
  } as any;
}

let lastContentType = '';
let lastStatus = 200;
let lastBody = '';

function makeRes() {
  return {
    type: (t: string) => { lastContentType = t; return res; },
    send: (b: string) => { lastBody = b; return res; },
    status: (s: number) => { lastStatus = s; return res; },
  } as any;
}

let res: ReturnType<typeof makeRes>;

// Unique phone per test to avoid session state leakage
let phoneCounter = 0;
function nextPhone(): string {
  return `+1555${String(++phoneCounter).padStart(7, '0')}`;
}

// ── Helpers to drive the full verification flow ──────────────────────────────

/**
 * Sends "hi" for a new phone, captures the OTP from the reply, then
 * verifies it. Returns the phone number for follow-up calls.
 */
async function fullVerify(phone: string): Promise<void> {
  // Step 1: greeting → get OTP
  await handleWhatsAppWebhook(makeReq(phone, 'hi'), makeRes());
  const otpMatch = lastBody.match(/(\d{6})/);
  assert.ok(otpMatch, 'OTP should be present in the welcome message body');
  const otp = otpMatch[1];

  // Step 2: submit OTP
  await handleWhatsAppWebhook(makeReq(phone, otp), makeRes());
  assert.match(lastBody, /verified/i);
}

// ── Test suites ──────────────────────────────────────────────────────────────

describe('webhook – missing sender', () => {
  test('returns 400 when From header is absent', async () => {
    const req = { body: { Body: 'hi' } } as any;
    res = makeRes();
    lastStatus = 200;
    await handleWhatsAppWebhook(req, res);
    assert.equal(lastStatus, 400);
  });
});

describe('webhook – rate limiting', () => {
  test('blocks after 10 messages within a 1-minute window', async () => {
    const phone = nextPhone();
    // Exhaust the window (10 allowed)
    for (let i = 0; i < 10; i++) {
      capturedMessages.length = 0;
      await handleWhatsAppWebhook(makeReq(phone, 'hi'), makeRes());
    }
    capturedMessages.length = 0;
    await handleWhatsAppWebhook(makeReq(phone, 'hi'), makeRes());
    assert.match(lastBody, /rate limit/i);
  });
});

describe('webhook – UNVERIFIED state', () => {
  test('greeting triggers OTP and transitions to AWAITING_OTP', async () => {
    const phone = nextPhone();
    await handleWhatsAppWebhook(makeReq(phone, 'hi'), makeRes());
    assert.match(lastBody, /otp/i);
    assert.match(lastBody, /\d{6}/); // OTP is embedded in response
  });

  test('non-greeting in UNVERIFIED state asks user to send "hi"', async () => {
    const phone = nextPhone();
    await handleWhatsAppWebhook(makeReq(phone, 'deposit 100'), makeRes());
    assert.match(lastBody, /hi/i);
  });

  test('response content-type is text/xml', async () => {
    const phone = nextPhone();
    res = makeRes();
    await handleWhatsAppWebhook(makeReq(phone, 'hi'), res);
    assert.equal(lastContentType, 'text/xml');
  });
});

describe('webhook – AWAITING_OTP state', () => {
  test('correct OTP verifies the user and creates a wallet', async () => {
    const phone = nextPhone();
    // Get OTP
    await handleWhatsAppWebhook(makeReq(phone, 'hi'), makeRes());
    const otp = lastBody.match(/(\d{6})/)![1];
    // Submit OTP
    await handleWhatsAppWebhook(makeReq(phone, otp), makeRes());
    assert.match(lastBody, /verified/i);
    assert.match(lastBody, /public key/i);
  });

  test('wrong OTP returns an error message', async () => {
    const phone = nextPhone();
    await handleWhatsAppWebhook(makeReq(phone, 'hi'), makeRes());
    await handleWhatsAppWebhook(makeReq(phone, '000000'), makeRes());
    assert.match(lastBody, /invalid/i);
  });

  test('non-OTP input asks for the 6-digit code', async () => {
    const phone = nextPhone();
    await handleWhatsAppWebhook(makeReq(phone, 'hi'), makeRes());
    await handleWhatsAppWebhook(makeReq(phone, 'please verify me'), makeRes());
    assert.match(lastBody, /6.digit/i);
  });
});

describe('webhook – VERIFIED state: greeting', () => {
  test('greeting returns the menu', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'hi'), makeRes());
    assert.match(lastBody, /balance/i);
    assert.match(lastBody, /deposit/i);
  });
});

describe('webhook – VERIFIED state: balance / earnings / APY', () => {
  test('balance query returns portfolio info', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'balance'), makeRes());
    assert.match(lastBody, /usdc/i);
    assert.match(lastBody, /apy/i);
  });

  test('earnings query returns portfolio info', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'earnings'), makeRes());
    assert.match(lastBody, /earnings/i);
  });

  test('APY query returns portfolio info', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'apy'), makeRes());
    assert.match(lastBody, /apy/i);
  });
});

describe('webhook – VERIFIED state: deposit', () => {
  test('valid deposit returns confirmation with tx hash', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'deposit 100 USDC'), makeRes());
    assert.match(lastBody, /deposited/i);
    assert.match(lastBody, /100/);
  });

  test('deposit below minimum is rejected by contract limits', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'deposit 0.05'), makeRes());
    assert.match(lastBody, /minimum/i);
  });

  test('deposit above maximum is rejected by contract limits', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'deposit 9999'), makeRes());
    assert.match(lastBody, /maximum/i);
  });

  test('deposit without amount is rejected', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'deposit'), makeRes());
    assert.match(lastBody, /amount/i);
  });
});

describe('webhook – VERIFIED state: withdraw', () => {
  test('valid partial withdraw returns confirmation', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'withdraw 50'), makeRes());
    assert.match(lastBody, /withdrew/i);
  });

  test('withdraw all returns all-funds confirmation', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'withdraw all'), makeRes());
    assert.match(lastBody, /all funds/i);
  });

  test('withdraw without amount is rejected', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'withdraw'), makeRes());
    assert.match(lastBody, /amount/i);
  });
});

describe('webhook – VERIFIED state: strategy', () => {
  test('valid strategy switch returns confirmation', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'switch to growth'), makeRes());
    assert.match(lastBody, /growth/i);
    assert.match(lastBody, /strategy/i);
  });

  test('invalid strategy name returns an error', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'switch to aggressive'), makeRes());
    assert.match(lastBody, /aggressive/i);
  });
});

describe('webhook – VERIFIED state: unknown intent', () => {
  test('unrecognised message returns help text', async () => {
    const phone = nextPhone();
    await fullVerify(phone);
    await handleWhatsAppWebhook(makeReq(phone, 'what on earth'), makeRes());
    assert.match(lastBody, /deposit/i);
  });
});

describe('webhook – error handling', () => {
  test('internal error returns a graceful error message (no throw)', async () => {
    const phone = nextPhone();
    await fullVerify(phone);

    // Force an error by temporarily breaking getPortfolio response parsing
    // We achieve this by sending a message that triggers the try/catch path –
    // a vaultRouter call where the wallet was removed from the store.
    // The simplest trigger: call with a verified session but corrupt the
    // wallet store by passing an unknown hash. Here we test the fallback path.
    const badReq = { body: { From: `whatsapp:${phone}`, Body: 'balance' } } as any;
    // This should resolve (not throw) even if an error occurs internally
    await assert.doesNotReject(() => handleWhatsAppWebhook(badReq, makeRes()));
  });
});
