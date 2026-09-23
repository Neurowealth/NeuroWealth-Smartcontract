import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';
import { parseIntent } from './intentParser';
import { openAiKeyManager } from './openAiKeyManager';

/**
 * Unit tests for the intent parser (#689).
 *
 * parseIntent goes straight to OpenAI through the key-rotation singleton, so the
 * rotation entry point is replaced with a canned model response: what is under
 * test here is the prompt contract and the validation rules around the returned
 * JSON, not the network call itself.
 */

const originalExecuteWithRotation = openAiKeyManager.executeWithRotation;
const originalError = console.error;
let responseContent: string | null = null;
let capturedRequest: Record<string, any> | null = null;
let rotationFails: Error | null = null;

beforeEach(() => {
  responseContent = null;
  capturedRequest = null;
  rotationFails = null;
  console.error = () => {};
  (openAiKeyManager as any).executeWithRotation = async (operation: (client: any) => Promise<unknown>) => {
    if (rotationFails) throw rotationFails;
    const client = {
      chat: {
        completions: {
          create: async (request: Record<string, any>) => {
            capturedRequest = request;
            return { choices: [{ message: { content: responseContent } }] };
          },
        },
      },
    };
    return operation(client);
  };
});

afterEach(() => {
  (openAiKeyManager as any).executeWithRotation = originalExecuteWithRotation;
  console.error = originalError;
});

describe('parseIntent prompt contract (#689)', () => {
  it('asks the model for a JSON object and forwards the user message verbatim', async () => {
    responseContent = '{"action":"balance"}';
    await parseIntent('how much do I have?');

    assert.ok(capturedRequest, 'the model should have been called');
    assert.strictEqual(capturedRequest!.model, 'gpt-4-turbo');
    assert.deepStrictEqual(capturedRequest!.response_format, { type: 'json_object' });
    assert.strictEqual(capturedRequest!.messages[0].role, 'system');
    assert.match(capturedRequest!.messages[0].content, /intent parser/i);
    assert.strictEqual(capturedRequest!.messages[1].content, 'how much do I have?');
  });
});

describe('parseIntent common intents (#689)', () => {
  it('parses a deposit with an amount', async () => {
    responseContent = '{"action":"deposit","amount":50}';
    assert.deepStrictEqual(await parseIntent('put in 50 usdc'), {
      action: 'deposit',
      amount: 50,
    });
  });

  it("parses a withdraw-everything request as the string 'all'", async () => {
    responseContent = '{"action":"withdraw","amount":"all"}';
    assert.deepStrictEqual(await parseIntent('take everything out'), {
      action: 'withdraw',
      amount: 'all',
    });
  });

  it('parses a balance question that carries no amount', async () => {
    responseContent = '{"action":"balance"}';
    assert.deepStrictEqual(await parseIntent('what is my balance'), { action: 'balance' });
  });

  it('parses a strategy switch with the strategy name', async () => {
    responseContent = '{"action":"switch_strategy","strategy":"growth"}';
    assert.deepStrictEqual(await parseIntent('move me to growth'), {
      action: 'switch_strategy',
      strategy: 'growth',
    });
  });

  it('parses an apy question', async () => {
    responseContent = '{"action":"get_apy"}';
    assert.deepStrictEqual(await parseIntent('what apy am I getting'), { action: 'get_apy' });
  });
});

describe('parseIntent validation (#689)', () => {
  it('rejects a deposit with no amount', async () => {
    responseContent = '{"action":"deposit"}';
    await assert.rejects(() => parseIntent('deposit please'), /requires an amount/);
  });

  it('rejects a withdrawal with no amount', async () => {
    responseContent = '{"action":"withdraw"}';
    await assert.rejects(() => parseIntent('withdraw'), /amount or 'all'/);
  });

  it('rejects a zero amount', async () => {
    responseContent = '{"action":"deposit","amount":0}';
    await assert.rejects(() => parseIntent('deposit zero'), /greater than 0/);
  });

  it('rejects a negative amount', async () => {
    responseContent = '{"action":"withdraw","amount":-5}';
    await assert.rejects(() => parseIntent('withdraw minus five'), /greater than 0/);
  });
});

describe('parseIntent malformed and failing responses (#689)', () => {
  it('rejects an empty model response instead of returning undefined', async () => {
    responseContent = null;
    await assert.rejects(() => parseIntent('hello'), /Empty response from OpenAI/);
  });

  it('rejects a response that is not JSON', async () => {
    responseContent = 'I think you want to deposit';
    await assert.rejects(() => parseIntent('deposit'), SyntaxError);
  });

  it('rejects JSON that is not an object rather than returning it', async () => {
    responseContent = 'null';
    await assert.rejects(() => parseIntent('deposit 5'));
  });

  it('rejects an empty array response', async () => {
    responseContent = '[]';
    await assert.rejects(() => parseIntent('withdraw all'));
  });

  it('propagates a failure from the key-rotation layer', async () => {
    rotationFails = new Error('all OpenAI keys exhausted');
    await assert.rejects(() => parseIntent('balance'), /all OpenAI keys exhausted/);
  });
});
