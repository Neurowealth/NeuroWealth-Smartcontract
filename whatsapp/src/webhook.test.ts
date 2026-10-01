import { describe, it } from 'node:test';
import assert from 'node:assert';
import { handleWhatsAppWebhook } from './webhook';
import { hashPhoneNumber } from './cryptoUtils';
import { getWallet } from './walletService';
import { getSession, updateState, UserState } from './stateManager';

// Webhook state machine (#691). Each test uses its own phone number, because the
// session, wallet and OTP stores are module-level maps with no reset hook - and
// the rate limiter counts per phone, so sharing one would couple the tests.

let counter = 0;
const freshFrom = () => 'whatsapp:+1555' + String(counter += 1).padStart(7, '0');

interface Captured {
  status?: number;
  type?: string;
  body: string;
}

async function send(from: string, body: string): Promise<Captured> {
  const captured: Captured = { body: '' };
  const res: any = {
    status(code: number) {
      captured.status = code;
      return res;
    },
    type(value: string) {
      captured.type = value;
      return res;
    },
    send(value: string) {
      captured.body = value;
      return res;
    },
  };
  await handleWhatsAppWebhook({ body: { From: from, Body: body } } as any, res);
  return captured;
}

async function verifiedPhone(): Promise<string> {
  const from = freshFrom();
  const greeting = await send(from, 'hi');
  const code = greeting.body.match(/[0-9]{6}/);
  assert.ok(code, 'the greeting must issue an OTP');
  const confirmation = await send(from, code![0]);
  assert.match(confirmation.body, /verified/i);
  return from;
}

describe('webhook rejected input (#691)', () => {
  it('rejects a payload with no sender', async () => {
    const captured = await send('', 'hi');
    assert.strictEqual(captured.status, 400);
    assert.match(captured.body, /Missing sender phone number/);
  });
});

describe('webhook verification flow (#691)', () => {
  it('answers a greeting with an OTP and moves the session to AWAITING_OTP', async () => {
    const from = freshFrom();
    const captured = await send(from, 'hi');

    assert.strictEqual(captured.type, 'text/xml');
    assert.match(captured.body, /OTP code is: [0-9]{6}/);
    assert.strictEqual(getSession(hashPhoneNumber(from)).state, UserState.AWAITING_OTP);
  });

  it('asks an unverified user to greet first when they send something else', async () => {
    const captured = await send(freshFrom(), 'what is my balance');
    assert.match(captured.body, /Send "hi" to begin/);
  });

  it('creates a wallet once the OTP is accepted', async () => {
    const from = freshFrom();
    const issued = (await send(from, 'hi')).body.match(/[0-9]{6}/)![0];
    const captured = await send(from, issued);

    assert.match(captured.body, /Public Key: G/);
    assert.strictEqual(getSession(hashPhoneNumber(from)).state, UserState.VERIFIED);
    assert.ok(getWallet(hashPhoneNumber(from)), 'a wallet should exist after verification');
  });

  it('rejects a wrong OTP and says how many attempts remain', async () => {
    const from = freshFrom();
    const issued = (await send(from, 'hi')).body.match(/[0-9]{6}/)![0];
    const wrong = issued === '000000' ? '111111' : '000000';
    const captured = await send(from, wrong);

    assert.match(captured.body, /attempts remaining/);
    assert.strictEqual(getSession(hashPhoneNumber(from)).state, UserState.AWAITING_OTP);
  });

  it('prompts again when the awaiting-OTP user sends something that is not a code', async () => {
    const from = freshFrom();
    await send(from, 'hi');
    const captured = await send(from, 'what now');

    assert.match(captured.body, /6-digit verification code/);
  });
});

describe('webhook verified flows (#691)', () => {
  it('answers a greeting with the menu', async () => {
    const captured = await send(await verifiedPhone(), 'hi');
    assert.match(captured.body, /How can I assist your portfolio today/);
  });

  it('answers a balance question with the portfolio', async () => {
    const captured = await send(await verifiedPhone(), 'balance');
    assert.match(captured.body, /NeuroWealth Portfolio/);
    assert.match(captured.body, /Current APY/);
  });

  it('handles a deposit and echoes the amount', async () => {
    const captured = await send(await verifiedPhone(), 'deposit 100 usdc');
    assert.match(captured.body, /Deposited 100 USDC/);
    assert.match(captured.body, /Transaction Hash/);
  });

  it('handles a partial withdrawal', async () => {
    const captured = await send(await verifiedPhone(), 'withdraw 50');
    assert.match(captured.body, /Withdrew 50 USDC/);
  });

  it('handles a full withdrawal', async () => {
    const captured = await send(await verifiedPhone(), 'withdraw all');
    assert.match(captured.body, /all funds/);
  });

  it('confirms a strategy switch in upper case', async () => {
    const captured = await send(await verifiedPhone(), 'switch to growth');
    assert.match(captured.body, /Strategy updated to GROWTH/);
  });

  it('offers examples when it does not understand', async () => {
    const captured = await send(await verifiedPhone(), 'what is the weather');
    assert.match(captured.body, /didn't quite catch that/);
  });

  it('resets a verified session whose wallet is missing', async () => {
    const from = freshFrom();
    const phoneHash = hashPhoneNumber(from);
    updateState(phoneHash, UserState.VERIFIED);

    const captured = await send(from, 'balance');
    assert.match(captured.body, /Session expired/);
    assert.strictEqual(getSession(phoneHash).state, UserState.UNVERIFIED);
  });
});

describe('webhook rate limiting (#691)', () => {
  it('answers the eleventh message in a minute with a rate-limit notice', async () => {
    const from = freshFrom();
    for (let i = 0; i < 10; i += 1) {
      await send(from, 'hi');
    }
    const captured = await send(from, 'hi');

    assert.match(captured.body, /Rate limit exceeded/);
  });
});
