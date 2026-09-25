import { describe, it } from 'node:test';
import assert from 'node:assert';
import { EventCircuitBreaker, CircuitBreakerState } from './eventCircuitBreaker';
import { getCircuitBreakerStatus, resetCircuitBreaker, stopEventListener } from './eventListener';

describe('EventCircuitBreaker - Soroban Polling Retry & Circuit Breaker (#751)', () => {
  it('initializes in CLOSED state with 0 failures and base delay', () => {
    const cb = new EventCircuitBreaker({
      failureThreshold: 3,
      baseDelayMs: 1000,
      maxDelayMs: 8000,
      cooldownMs: 5000,
      jitter: false,
    });

    assert.strictEqual(cb.getState(), 'CLOSED');
    assert.strictEqual(cb.canExecute(), true);
    assert.strictEqual(cb.getNextDelayMs(), 1000);

    const status = cb.getStatus();
    assert.strictEqual(status.state, 'CLOSED');
    assert.strictEqual(status.consecutiveFailures, 0);
    assert.strictEqual(status.failureThreshold, 3);
    assert.strictEqual(status.lastError, null);
  });

  it('calculates exponential backoff on consecutive failures without tripping below threshold', () => {
    const now = 10000;
    const cb = new EventCircuitBreaker({
      failureThreshold: 4,
      baseDelayMs: 1000,
      maxDelayMs: 10000,
      cooldownMs: 15000,
      jitter: false,
      now: () => now,
    });

    // 1st failure: 1000 * 2^0 = 1000ms
    cb.recordFailure(new Error('RPC rate limit'));
    assert.strictEqual(cb.getState(), 'CLOSED');
    assert.strictEqual(cb.canExecute(), true);
    assert.strictEqual(cb.getStatus().consecutiveFailures, 1);
    assert.strictEqual(cb.getNextDelayMs(), 1000);

    // 2nd failure: 1000 * 2^1 = 2000ms
    cb.recordFailure(new Error('Connection timeout'));
    assert.strictEqual(cb.getState(), 'CLOSED');
    assert.strictEqual(cb.getStatus().consecutiveFailures, 2);
    assert.strictEqual(cb.getNextDelayMs(), 2000);

    // 3rd failure: 1000 * 2^2 = 4000ms
    cb.recordFailure(new Error('503 Service Unavailable'));
    assert.strictEqual(cb.getState(), 'CLOSED');
    assert.strictEqual(cb.getStatus().consecutiveFailures, 3);
    assert.strictEqual(cb.getNextDelayMs(), 4000);

    // A success resets consecutive failures back to 0 and base delay
    cb.recordSuccess();
    assert.strictEqual(cb.getState(), 'CLOSED');
    assert.strictEqual(cb.getStatus().consecutiveFailures, 0);
    assert.strictEqual(cb.getNextDelayMs(), 1000);
  });

  it('trips to OPEN state when failures reach threshold and emits alert callbacks', () => {
    const now = 100000;
    const stateTransitions: { from: CircuitBreakerState; to: CircuitBreakerState }[] = [];
    let trippedFailures: number | null = null;
    let trippedError: string | undefined;

    const cb = new EventCircuitBreaker({
      failureThreshold: 3,
      baseDelayMs: 1000,
      maxDelayMs: 8000,
      cooldownMs: 20000,
      jitter: false,
      now: () => now,
      onStateChange: (from, to) => stateTransitions.push({ from, to }),
      onTrip: (failures, lastError) => {
        trippedFailures = failures;
        trippedError = lastError;
      },
    });

    cb.recordFailure(new Error('fail 1'));
    cb.recordFailure(new Error('fail 2'));
    assert.strictEqual(cb.getState(), 'CLOSED');

    // 3rd failure trips the breaker
    cb.recordFailure(new Error('RPC cluster down'));
    assert.strictEqual(cb.getState(), 'OPEN');
    assert.strictEqual(cb.canExecute(), false);
    assert.strictEqual(trippedFailures, 3);
    assert.strictEqual(trippedError, 'RPC cluster down');
    assert.deepStrictEqual(stateTransitions, [{ from: 'CLOSED', to: 'OPEN' }]);

    // While OPEN, getNextDelayMs returns remaining cooldown time
    const delay = cb.getNextDelayMs();
    assert.ok(delay > 0 && delay <= 20000, `Delay ${delay} should be within cooldown window`);
  });

  it('transitions from OPEN to HALF_OPEN after cooldown and recovers to CLOSED on successful probe', () => {
    let now = 200000;
    const stateTransitions: { from: CircuitBreakerState; to: CircuitBreakerState }[] = [];

    const cb = new EventCircuitBreaker({
      failureThreshold: 2,
      baseDelayMs: 1000,
      maxDelayMs: 10000,
      cooldownMs: 15000,
      jitter: false,
      now: () => now,
      onStateChange: (from, to) => stateTransitions.push({ from, to }),
    });

    cb.recordFailure(new Error('fail 1'));
    cb.recordFailure(new Error('fail 2'));
    assert.strictEqual(cb.getState(), 'OPEN');
    assert.strictEqual(cb.canExecute(), false);

    // Before cooldown expires (e.g. 10s elapsed), breaker remains OPEN
    now += 10000;
    assert.strictEqual(cb.getState(), 'OPEN');
    assert.strictEqual(cb.canExecute(), false);

    // After cooldown expires (15s total elapsed), transitions to HALF_OPEN
    now += 6000; // total +16000ms
    assert.strictEqual(cb.getState(), 'HALF_OPEN');
    assert.strictEqual(cb.canExecute(), true, 'HALF_OPEN should permit probe poll');

    // Successful probe returns circuit to CLOSED
    cb.recordSuccess();
    assert.strictEqual(cb.getState(), 'CLOSED');
    assert.strictEqual(cb.getStatus().consecutiveFailures, 0);
    assert.strictEqual(cb.getNextDelayMs(), 1000);

    assert.deepStrictEqual(stateTransitions, [
      { from: 'CLOSED', to: 'OPEN' },
      { from: 'OPEN', to: 'HALF_OPEN' },
      { from: 'HALF_OPEN', to: 'CLOSED' },
    ]);
  });

  it('re-trips to OPEN if probe fails in HALF_OPEN state', () => {
    let now = 300000;
    const cb = new EventCircuitBreaker({
      failureThreshold: 2,
      baseDelayMs: 1000,
      maxDelayMs: 10000,
      cooldownMs: 15000,
      jitter: false,
      now: () => now,
    });

    cb.recordFailure(new Error('fail 1'));
    cb.recordFailure(new Error('fail 2'));
    assert.strictEqual(cb.getState(), 'OPEN');

    // Fast-forward cooldown
    now += 20000;
    assert.strictEqual(cb.getState(), 'HALF_OPEN');

    // Probe poll fails
    cb.recordFailure(new Error('Probe failed'));
    assert.strictEqual(cb.getState(), 'OPEN');
    assert.strictEqual(cb.canExecute(), false);
    assert.strictEqual(cb.getStatus().consecutiveFailures, 3);
  });

  it('supports manual reset and trip controls', () => {
    const cb = new EventCircuitBreaker({
      failureThreshold: 5,
      cooldownMs: 10000,
    });

    cb.trip('Administrative maintenance');
    assert.strictEqual(cb.getState(), 'OPEN');
    assert.strictEqual(cb.canExecute(), false);

    cb.reset();
    assert.strictEqual(cb.getState(), 'CLOSED');
    assert.strictEqual(cb.canExecute(), true);
    assert.strictEqual(cb.getStatus().consecutiveFailures, 0);
  });

  it('exports getCircuitBreakerStatus and resetCircuitBreaker from eventListener', () => {
    const status = getCircuitBreakerStatus();
    assert.ok(status);
    assert.ok(['CLOSED', 'OPEN', 'HALF_OPEN'].includes(status.state));
    assert.strictEqual(typeof status.failureThreshold, 'number');

    resetCircuitBreaker();
    const updatedStatus = getCircuitBreakerStatus();
    assert.strictEqual(updatedStatus.state, 'CLOSED');
    assert.strictEqual(updatedStatus.consecutiveFailures, 0);

    stopEventListener();
  });
});
