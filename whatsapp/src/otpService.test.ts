import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';
import { InMemoryStateStore, setStateStore } from './stateStore';
import { generateOTP, verifyOTP } from './otpService';

// OTP issuance and verification (#691). Expiry and the attempt counter decide
// whether a chat user can take over an account, so both run on a controlled
// clock.

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

const phone = (suffix: string) => 'hash-' + suffix;

describe('generateOTP (#691)', () => {
  it('returns a six-digit numeric code', async () => {
    for (const suffix of ['a', 'b', 'c']) {
      assert.match((await generateOTP(phone(suffix))), /^[0-9]{6}$/);
    }
  });

  it('replaces any previous code for the same phone', async () => {
    const first = (await generateOTP(phone('a')));
    const second = (await generateOTP(phone('a')));
    assert.strictEqual((await verifyOTP(phone('a'), first)).success, false);
    assert.strictEqual((await verifyOTP(phone('a'), second)).success, true);
  });
});

describe('verifyOTP (#691)', () => {
  it('accepts the issued code and clears it afterwards', async () => {
    const code = (await generateOTP(phone('a')));
    assert.strictEqual((await verifyOTP(phone('a'), code)).success, true);
    assert.strictEqual((await verifyOTP(phone('a'), code)).success, false, 'single use only');
  });

  it('tolerates surrounding whitespace in the submitted code', async () => {
    const code = (await generateOTP(phone('b')));
    assert.strictEqual((await verifyOTP(phone('b'), '  ' + code + '  ')).success, true);
  });

  it('reports an unknown phone rather than succeeding', async () => {
    const result = (await verifyOTP('never-requested', '123456'));
    assert.strictEqual(result.success, false);
    assert.match(result.message, /No OTP request found/);
  });

  it('counts down the remaining attempts on a wrong code', async () => {
    const code = (await generateOTP(phone('c')));
    const wrong = code === '000000' ? '111111' : '000000';

    assert.match((await verifyOTP(phone('c'), wrong)).message, /2 attempts remaining/);
    assert.match((await verifyOTP(phone('c'), wrong)).message, /1 attempts remaining/);
  });

  it('exhausts the attempts and locks out on the next call', async () => {
    const code = (await generateOTP(phone('d')));
    const wrong = code === '000000' ? '111111' : '000000';

    // Three wrong codes are answered with a countdown; the lock is applied
    // when the exhausted record is presented again, so the fourth call is the
    // one that reports 'too many' and clears the record.
    assert.match((await verifyOTP(phone('d'), wrong)).message, /2 attempts remaining/);
    assert.match((await verifyOTP(phone('d'), wrong)).message, /1 attempts remaining/);
    assert.match((await verifyOTP(phone('d'), wrong)).message, /0 attempts remaining/);

    const locked = (await verifyOTP(phone('d'), wrong));
    assert.strictEqual(locked.success, false);
    assert.match(locked.message, /Too many invalid attempts/);

    // After the lock the correct code no longer works either - a new code has
    // to be requested.
    assert.strictEqual((await verifyOTP(phone('d'), code)).success, false);
  });

  it('expires a code after five minutes', async () => {
    const code = (await generateOTP(phone('e')));
    clock += 5 * 60 * 1000 + 1;
    const result = (await verifyOTP(phone('e'), code));
    assert.strictEqual(result.success, false);
    assert.match(result.message, /expired/);
  });

  it('still accepts a code just inside the five-minute window', async () => {
    const code = (await generateOTP(phone('f')));
    clock += 5 * 60 * 1000 - 1;
    assert.strictEqual((await verifyOTP(phone('f'), code)).success, true);
  });

  it('keeps phones independent', async () => {
    const first = (await generateOTP(phone('g')));
    (await generateOTP(phone('h')));
    assert.strictEqual((await verifyOTP(phone('h'), first)).success, false);
  });
});
