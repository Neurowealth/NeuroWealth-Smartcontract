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

  test('tolerates a missing body', async () => {
    const req: any = {};
    const res = makeRes();

    await handleDeliveryStatusCallback(req, res);

    assert.equal(res.statusCode, 200);
    assert.deepEqual(calls, [{ messageSid: '', status: '', to: undefined, errorCode: undefined, errorMessage: undefined }]);
  });
});
