import { describe, it } from 'node:test';
import assert from 'node:assert';
import { gracefulShutdown } from './index';
import { stopEventListener } from './eventListener';
import { initPool, isPoolClosed } from './db';

describe('Agent Graceful Shutdown Lifecycle (#750)', () => {
  it('stopEventListener stops listener and waits for in-flight polling without error', async () => {
    // Should succeed even if not started
    await stopEventListener(500);
    assert.ok(true);
  });

  it('gracefulShutdown executes clean shutdown across HTTP server and DB pool', async () => {
    initPool();

    let serverClosed = false;
    const fakeServer = {
      close: (cb: (err?: Error) => void) => {
        serverClosed = true;
        cb();
        return fakeServer as unknown as import('http').Server;
      },
    } as unknown as import('http').Server;

    await gracefulShutdown('SIGTERM', fakeServer, { timeoutMs: 3000, exitProcess: false });

    assert.strictEqual(serverClosed, true, 'HTTP server should be closed');
    assert.strictEqual(isPoolClosed(), true, 'DB pool should be closed');

    // Duplicate shutdown signal should be safely ignored
    await gracefulShutdown('SIGINT', fakeServer, { timeoutMs: 3000, exitProcess: false });
  });
});
