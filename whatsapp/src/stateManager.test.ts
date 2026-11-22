import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';
import { InMemoryStateStore, setStateStore } from './stateStore';
import { getSession, updateState, checkRateLimit, UserState } from './stateManager';

// Conversation state and the per-phone rate limit (#691). Both are time-based,
// so the clock is controlled rather than waited on.

const realNow = Date.now;
let clock = 1_700_000_000_000;

beforeEach(() => {
  setStateStore(new InMemoryStateStore());
  clock = 1_700_000_000_000;
  Date.now = () => clock;
});

afterEach(() => {
  Date.now = realNow;
});

let counter = 0;
const freshPhone = () => 'session-test-' + (counter += 1);

describe('getSession / updateState (#691)', () => {
  it('starts a new conversation unverified', async () => {
    const session = (await getSession(freshPhone()));
    assert.strictEqual(session.state, UserState.UNVERIFIED);
    assert.strictEqual(session.messageCount, 0);
  });

  it('remembers a state change for the same phone', async () => {
    const phone = freshPhone();
    (await updateState(phone, UserState.AWAITING_OTP));
    assert.strictEqual((await getSession(phone)).state, UserState.AWAITING_OTP);
  });

  it('keeps phones separate', async () => {
    const first = freshPhone();
    const second = freshPhone();
    (await updateState(first, UserState.VERIFIED));
    assert.strictEqual((await getSession(second)).state, UserState.UNVERIFIED);
  });

  it('resets state and counters after the inactivity timeout', async () => {
    const phone = freshPhone();
    (await updateState(phone, UserState.VERIFIED));

    clock += 15 * 60 * 1000 + 1;
    const session = (await getSession(phone));

    assert.strictEqual(session.state, UserState.UNVERIFIED, 'a timed-out session must not stay verified');
    assert.strictEqual(session.messageCount, 0);
  });

  it('keeps the state inside the inactivity window', async () => {
    const phone = freshPhone();
    (await updateState(phone, UserState.VERIFIED));
    clock += 15 * 60 * 1000 - 1;
    assert.strictEqual((await getSession(phone)).state, UserState.VERIFIED);
  });
});

describe('checkRateLimit (#691)', () => {
  it('allows ten messages in the window and rejects the eleventh', async () => {
    const phone = freshPhone();
    for (let i = 1; i <= 10; i += 1) {
      assert.strictEqual((await checkRateLimit(phone)), true, 'message ' + i + ' should be allowed');
    }
    assert.strictEqual((await checkRateLimit(phone)), false);
  });

  it('lets the limit recover once the window passes', async () => {
    const phone = freshPhone();
    for (let i = 0; i < 11; i += 1) (await checkRateLimit(phone));
    assert.strictEqual((await checkRateLimit(phone)), false);

    clock += 60 * 1000 + 1;
    assert.strictEqual((await checkRateLimit(phone)), true);
  });

  it('counts each phone separately', async () => {
    const noisy = freshPhone();
    const quiet = freshPhone();
    for (let i = 0; i < 11; i += 1) (await checkRateLimit(noisy));
    assert.strictEqual((await checkRateLimit(quiet)), true);
  });
});
