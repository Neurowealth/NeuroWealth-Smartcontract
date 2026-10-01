import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';
import { generateOTP, verifyOTP } from './otpService';

// OTP issuance and verification (#691). Expiry and the attempt counter decide
// whether a chat user can take over an account, so both run on a controlled
// clock.

const realNow = Date.now;
let clock = 1_700_000_000_000;

beforeEach(() => {
  clock = 1_700_000_000_000;
  Date.now = () => clock;
});

afterEach(() => {
  Date.now = realNow;
});

const phone = (suffix: string) => 'hash-' + suffix;

describe('generateOTP (#691)', () => {
  it('returns a six-digit numeric code', () => {
    for (const suffix of ['a', 'b', 'c']) {
      assert.match(generateOTP(phone(suffix)), /^[0-9]{6}$/);
    }
  });

  it('replaces any previous code for the same phone', () => {
    const first = generateOTP(phone('a'));
    const second = generateOTP(phone('a'));
    assert.strictEqual(verifyOTP(phone('a'), first).success, false);
    assert.strictEqual(verifyOTP(phone('a'), second).success, true);
  });
});

describe('verifyOTP (#691)', () => {
  it('accepts the issued code and clears it afterwards', () => {
    const code = generateOTP(phone('a'));
    assert.strictEqual(verifyOTP(phone('a'), code).success, true);
    assert.strictEqual(verifyOTP(phone('a'), code).success, false, 'single use only');
  });

  it('tolerates surrounding whitespace in the submitted code', () => {
    const code = generateOTP(phone('b'));
    assert.strictEqual(verifyOTP(phone('b'), '  ' + code + '  ').success, true);
  });

  it('reports an unknown phone rather than succeeding', () => {
    const result = verifyOTP('never-requested', '123456');
    assert.strictEqual(result.success, false);
    assert.match(result.message, /No OTP request found/);
  });

  it('counts down the remaining attempts on a wrong code', () => {
    const code = generateOTP(phone('c'));
    const wrong = code === '000000' ? '111111' : '000000';

    assert.match(verifyOTP(phone('c'), wrong).message, /2 attempts remaining/);
    assert.match(verifyOTP(phone('c'), wrong).message, /1 attempts remaining/);
  });

  it('exhausts the attempts and locks out on the next call', () => {
    const code = generateOTP(phone('d'));
    const wrong = code === '000000' ? '111111' : '000000';

    // Three wrong codes are answered with a countdown; the lock is applied
    // when the exhausted record is presented again, so the fourth call is the
    // one that reports 'too many' and clears the record.
    assert.match(verifyOTP(phone('d'), wrong).message, /2 attempts remaining/);
    assert.match(verifyOTP(phone('d'), wrong).message, /1 attempts remaining/);
    assert.match(verifyOTP(phone('d'), wrong).message, /0 attempts remaining/);

    const locked = verifyOTP(phone('d'), wrong);
    assert.strictEqual(locked.success, false);
    assert.match(locked.message, /Too many invalid attempts/);

    // After the lock the correct code no longer works either - a new code has
    // to be requested.
    assert.strictEqual(verifyOTP(phone('d'), code).success, false);
  });

  it('expires a code after five minutes', () => {
    const code = generateOTP(phone('e'));
    clock += 5 * 60 * 1000 + 1;
    const result = verifyOTP(phone('e'), code);
    assert.strictEqual(result.success, false);
    assert.match(result.message, /expired/);
  });

  it('still accepts a code just inside the five-minute window', () => {
    const code = generateOTP(phone('f'));
    clock += 5 * 60 * 1000 - 1;
    assert.strictEqual(verifyOTP(phone('f'), code).success, true);
  });

  it('keeps phones independent', () => {
    const first = generateOTP(phone('g'));
    generateOTP(phone('h'));
    assert.strictEqual(verifyOTP(phone('h'), first).success, false);
  });
});
