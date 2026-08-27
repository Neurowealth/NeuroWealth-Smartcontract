import { describe, it } from 'node:test';
import assert from 'node:assert';
import { OpenAIKeyManager } from './openAiKeyManager';

describe('OpenAIKeyManager (#712)', () => {
  it('initializes with multiple keys and rotates between them', async () => {
    const manager = new OpenAIKeyManager(['key-1', 'key-2', 'key-3']);
    assert.strictEqual(manager.keyCount, 3);

    const executedKeys: string[] = [];

    // Call 3 times
    for (let i = 0; i < 3; i++) {
      await manager.executeWithRotation(async (client) => {
        executedKeys.push(client.apiKey);
        return 'ok';
      });
    }

    assert.deepStrictEqual(executedKeys, ['key-1', 'key-2', 'key-3']);
  });

  it('automatically rotates to next key when an error occurs', async () => {
    const manager = new OpenAIKeyManager(['bad-key', 'good-key']);
    let callCount = 0;

    const result = await manager.executeWithRotation(async (client) => {
      callCount++;
      if (client.apiKey === 'bad-key') {
        const err: any = new Error('Rate limit exceeded');
        err.status = 429;
        throw err;
      }
      return 'success-from-good-key';
    });

    assert.strictEqual(result, 'success-from-good-key');
    assert.strictEqual(callCount, 2);

    const status = manager.getHealthStatus();
    assert.strictEqual(status[0]?.isHealthy, false);
    assert.strictEqual(status[1]?.isHealthy, true);
  });
});
