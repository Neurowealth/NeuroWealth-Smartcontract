process.env.ENCRYPTION_KEY = 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT = 'test-phone-salt';

const calls: any[] = [];
let shouldThrow = false;

const messageDeliveryServicePath = require.resolve('../messageDeliveryService');
require.cache[messageDeliveryServicePath] = {
  id: messageDeliveryServicePath,
  filename: messageDeliveryServicePath,
  loaded: true,
  exports: {
    applyDeliveryStatus: async (callback: any) => {
      calls.push(callback);
      if (shouldThrow) throw new Error('db unavailable');
      return { applied: true, created: true };
    },
  },
  parent: null,
  children: [],
  paths: [],
} as any;

import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { handleDeliveryStatusCallback } from '../statusWebhook';
import { INPUT_LIMITS } from '../inputBounds';

function makeRes() {
  const res: any = {
    statusCode: 0,
    sent: false,
    status(code: number) {
      res.statusCode = code;
      return res;
    },
    send() {
      res.sent = true;
      return res;
    },
  };
  return res;
}

beforeEach(() => {
  calls.length = 0;
  shouldThrow = false;
});

describe('handleDeliveryStatusCallback', () => {
  test('forwards a well-formed callback and acknowledges with 200', async () => {
    const req: any = {
      body: {
        MessageSid: 'SM123',
        MessageStatus: 'Delivered',
        To: 'whatsapp:+15550009999',
      },
    };
    const res = makeRes();

    await handleDeliveryStatusCallback(req, res);

    assert.equal(res.statusCode, 200);
    assert.equal(res.sent, true);
    assert.deepEqual(calls, [
      { messageSid: 'SM123', status: 'delivered', to: 'whatsapp:+15550009999', errorCode: undefined, errorMessage: undefined },
    ]);
  });

  test('always acknowledges with 200 even when persistence fails', async () => {
    shouldThrow = true;
    const req: any = { body: { MessageSid: 'SM999', MessageStatus: 'failed' } };
    const res = makeRes();

    await handleDeliveryStatusCallback(req, res);

    assert.equal(res.statusCode, 200);
    assert.equal(res.sent, true);
  });

  test('forwards a callback field exactly at the limit and drops one byte over before persistence', async () => {
    const exact = 'S'.repeat(INPUT_LIMITS.providerCallback);
    const res = makeRes();
    await handleDeliveryStatusCallback({
      body: { MessageSid: exact, MessageStatus: 'sent' },
    } as any, res);

    assert.equal(res.statusCode, 200);
    assert.equal(calls.length, 1);
    assert.equal(calls[0].messageSid, exact);

    calls.length = 0;
    const over = `${exact}X`;
    const payload = `boom-${over}`;
    await handleDeliveryStatusCallback({
      body: { MessageSid: 'SM1', MessageStatus: 'failed', ErrorMessage: payload },
    } as any, res);

    assert.equal(res.statusCode, 200);
    assert.equal(calls.length, 0);
  });

  test('drops a malformed provider field before persistence', async () => {
    const res = makeRes();
    await handleDeliveryStatusCallback({
      body: { MessageSid: 'SM1', MessageStatus: 'sent', ErrorMessage: 'bad\uD800' },
    } as any, res);

    assert.equal(calls.length, 0);
    assert.equal(res.statusCode, 200);
  });

  test('tolerates a missing body', async () => {
    const req: any = {};
    const res = makeRes();

    await handleDeliveryStatusCallback(req, res);

    assert.equal(res.statusCode, 200);
    assert.deepEqual(calls, [{ messageSid: '', status: '', to: undefined, errorCode: undefined, errorMessage: undefined }]);
  });
});
