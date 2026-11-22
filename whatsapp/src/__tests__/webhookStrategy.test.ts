import { test, mock } from 'node:test';
import assert from 'node:assert/strict';
import { handleWhatsAppWebhook } from '../webhook';
import { Request, Response } from 'express';
import * as stateManager from '../stateManager';
import * as vaultRouter from '../vaultRouter';
import * as intentParser from '../intentParser';
import * as walletService from '../walletService';
import * as contractLimits from '../contractLimits';

test('Webhook handles STRATEGY intent and invokes handleStrategyUpdate', async () => {
  // Mock dependencies
  mock.method(stateManager, 'checkRateLimit', () => true);
  mock.method(stateManager, 'getSession', () => ({ state: stateManager.UserState.VERIFIED }));
  mock.method(intentParser, 'parseIntent', () => ({ type: 'STRATEGY', strategy: 'growth' }));
  mock.method(contractLimits, 'validateIntent', () => ({ ok: true }));
  mock.method(walletService, 'getWallet', () => Promise.resolve({}));
  
  const handleStrategyUpdateMock = mock.method(vaultRouter, 'handleStrategyUpdate', () => Promise.resolve({ success: true, message: 'Strategy successfully updated.' }));

  let responseText = '';
  const req = { body: { From: 'whatsapp:+1234567890', Body: 'switch to growth' } } as Request;
  const res = {
    status: () => res,
    type: () => res,
    send: (text: string) => { responseText = text; }
  } as unknown as Response;

  await handleWhatsAppWebhook(req, res);

  assert.equal(handleStrategyUpdateMock.mock.callCount(), 1);
  assert.match(responseText, /Strategy successfully updated./);
  
  // Cleanup mocks
  mock.restoreAll();
});
