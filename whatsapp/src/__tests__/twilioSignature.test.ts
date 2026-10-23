import { test } from 'node:test';
import assert from 'node:assert/strict';
import { Request, Response } from 'express';
import twilio from 'twilio';
import { verifyTwilioSignature } from '../twilioSignature';

const AUTH_TOKEN = 'test-auth-token';
const URL = 'https://bot.example.com/api/whatsapp/webhook';
const BODY = { From: 'whatsapp:+15550001111', Body: 'hi' };

function fakeReq(headers: Record<string, string>, body: object = BODY): Request {
  const lower: Record<string, string> = {};
  for (const [k, v] of Object.entries(headers)) lower[k.toLowerCase()] = v;
  return {
    body,
    protocol: 'https',
    originalUrl: '/api/whatsapp/webhook',
    get: (name: string) => (name.toLowerCase() === 'host' ? 'bot.example.com' : lower[name.toLowerCase()]),
  } as unknown as Request;
}

function run(middleware: ReturnType<typeof verifyTwilioSignature>, req: Request) {
  const result = { status: 200, nextCalled: false };
  const res = {
    status(code: number) { result.status = code; return this; },
    send() { return this; },
  } as unknown as Response;
  middleware(req, res, () => { result.nextCalled = true; });
  return result;
}

const sign = (body: object = BODY, url = URL) =>
  twilio.getExpectedTwilioSignature(AUTH_TOKEN, url, body as Record<string, string>);

test('accepts a correctly signed request', () => {
  const r = run(verifyTwilioSignature({ authToken: AUTH_TOKEN }), fakeReq({ 'X-Twilio-Signature': sign() }));
  assert.equal(r.nextCalled, true);
});

test('uses TWILIO_WEBHOOK_URL when configured', () => {
  const publicUrl = 'https://public.example.com/api/whatsapp/webhook';
  const mw = verifyTwilioSignature({ authToken: AUTH_TOKEN, webhookUrl: publicUrl });
  assert.equal(run(mw, fakeReq({ 'X-Twilio-Signature': sign(BODY, publicUrl) })).nextCalled, true);
  assert.equal(run(mw, fakeReq({ 'X-Twilio-Signature': sign() })).status, 403);
});

test('rejects a missing signature', () => {
  const r = run(verifyTwilioSignature({ authToken: AUTH_TOKEN }), fakeReq({}));
  assert.equal(r.status, 403);
  assert.equal(r.nextCalled, false);
});

test('rejects a forged sender (body tampered after signing)', () => {
  const sig = sign();
  const r = run(
    verifyTwilioSignature({ authToken: AUTH_TOKEN }),
    fakeReq({ 'X-Twilio-Signature': sig }, { ...BODY, From: 'whatsapp:+15559999999' }),
  );
  assert.equal(r.status, 403);
  assert.equal(r.nextCalled, false);
});

test('fails closed when TWILIO_AUTH_TOKEN is unset', () => {
  const r = run(verifyTwilioSignature({}), fakeReq({ 'X-Twilio-Signature': sign() }));
  assert.equal(r.status, 500);
  assert.equal(r.nextCalled, false);
});

test('skip flag works in development but is ignored in production', () => {
  const dev = verifyTwilioSignature({ skipValidation: true, nodeEnv: 'development' });
  assert.equal(run(dev, fakeReq({})).nextCalled, true);

  const prod = verifyTwilioSignature({ authToken: AUTH_TOKEN, skipValidation: true, nodeEnv: 'production' });
  assert.equal(run(prod, fakeReq({})).status, 403);
});

test('rejects HTTP in production', () => {
  const prod = verifyTwilioSignature({ authToken: AUTH_TOKEN, nodeEnv: 'production' });
  const req = fakeReq({ 'X-Twilio-Signature': sign() });
  (req as any).protocol = 'http';
  (req as any).secure = false;
  // @ts-ignore
  req.headers = { 'x-forwarded-proto': 'http' };
  
  assert.equal(run(prod, req).status, 403);
});
