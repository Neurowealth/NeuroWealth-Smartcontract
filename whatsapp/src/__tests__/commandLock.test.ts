import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { runExclusive, pendingLockCount } from '../commandLock';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('runExclusive', () => {
  test('serializes tasks for the same key in arrival order', async () => {
    const order: string[] = [];
    const first = deferred<void>();

    const p1 = runExclusive('user-a', async () => {
      order.push('start-1');
      await first.promise;
      order.push('end-1');
    });

    const p2 = runExclusive('user-a', async () => {
      order.push('start-2');
      order.push('end-2');
    });

    // The second task must not start until the first one resolves.
    await Promise.resolve();
    await Promise.resolve();
    assert.deepEqual(order, ['start-1']);

    first.resolve();
    await p1;
    await p2;

    assert.deepEqual(order, ['start-1', 'end-1', 'start-2', 'end-2']);
  });

  test('a failed task releases the lock for the next one', async () => {
    const order: string[] = [];

    const p1 = runExclusive('user-b', async () => {
      order.push('task-1');
      throw new Error('boom');
    });

    const p2 = runExclusive('user-b', async () => {
      order.push('task-2');
    });

    await assert.rejects(p1, /boom/);
    await p2;

    assert.deepEqual(order, ['task-1', 'task-2']);
  });

  test('different keys run independently and concurrently', async () => {
    const order: string[] = [];
    const blockA = deferred<void>();

    const pA = runExclusive('user-c', async () => {
      order.push('a-start');
      await blockA.promise;
      order.push('a-end');
    });

    const pB = runExclusive('user-d', async () => {
      order.push('b-start');
      order.push('b-end');
      return 'b-result';
    });

    const bResult = await pB;
    assert.equal(bResult, 'b-result');
    assert.deepEqual(order, ['a-start', 'b-start', 'b-end']);

    blockA.resolve();
    await pA;
    assert.deepEqual(order, ['a-start', 'b-start', 'b-end', 'a-end']);
  });

  test('resolves with the task result', async () => {
    const result = await runExclusive('user-e', async () => 42);
    assert.equal(result, 42);
  });

  test('cleans up the lock entry once the queue drains', async () => {
    await runExclusive('user-f', async () => {});
    // Give the trailing .then() cleanup a turn to run.
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(pendingLockCount(), 0);
  });
});
