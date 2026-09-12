import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';
import { getSession, updateState, checkRateLimit, UserState } from './stateManager';

// Conversation state and the per-phone rate limit (#691). Both are time-based,
// so the clock is controlled rather than waited on.

const realNow = Date.now;
let clock = 1_700_000_000_000;

beforeEach(() => {
  clock = 1_700_000_000_000;
  Date.now = () => clock;
});

afterEach(() => {
  Date.now = realNow;
});

let counter = 0;
const freshPhone = () => 'session-test-' + (counter += 1);

describe('getSession / updateState (#691)', () => {
  it('starts a new conversation unverified', () => {
    const session = getSession(freshPhone());
    assert.strictEqual(session.state, UserState.UNVERIFIED);
    assert.strictEqual(session.messageCount, 0);
  });

  it('remembers a state change for the same phone', () => {
    const phone = freshPhone();
    updateState(phone, UserState.AWAITING_OTP);
    assert.strictEqual(getSession(phone).state, UserState.AWAITING_OTP);
  });

  it('keeps phones separate', () => {
    const first = freshPhone();
    const second = freshPhone();
    updateState(first, UserState.VERIFIED);
    assert.strictEqual(getSession(second).state, UserState.UNVERIFIED);
  });

  it('resets state and counters after the inactivity timeout', () => {
    const phone = freshPhone();
    updateState(phone, UserState.VERIFIED);

    clock += 15 * 60 * 1000 + 1;
    const session = getSession(phone);

    assert.strictEqual(session.state, UserState.UNVERIFIED, 'a timed-out session must not stay verified');
    assert.strictEqual(session.messageCount, 0);
  });

  it('keeps the state inside the inactivity window', () => {
    const phone = freshPhone();
    updateState(phone, UserState.VERIFIED);
    clock += 15 * 60 * 1000 - 1;
    assert.strictEqual(getSession(phone).state, UserState.VERIFIED);
  });
});

describe('checkRateLimit (#691)', () => {
  it('allows ten messages in the window and rejects the eleventh', () => {
    const phone = freshPhone();
    for (let i = 1; i <= 10; i += 1) {
      assert.strictEqual(checkRateLimit(phone), true, 'message ' + i + ' should be allowed');
    }
    assert.strictEqual(checkRateLimit(phone), false);
  });

  it('lets the limit recover once the window passes', () => {
    const phone = freshPhone();
    for (let i = 0; i < 11; i += 1) checkRateLimit(phone);
    assert.strictEqual(checkRateLimit(phone), false);

    clock += 60 * 1000 + 1;
    assert.strictEqual(checkRateLimit(phone), true);
  });

  it('counts each phone separately', () => {
    const noisy = freshPhone();
    const quiet = freshPhone();
    for (let i = 0; i < 11; i += 1) checkRateLimit(noisy);
    assert.strictEqual(checkRateLimit(quiet), true);
  });
});
