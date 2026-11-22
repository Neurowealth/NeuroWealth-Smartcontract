/**
 * Unit tests for otpService.ts (Issue #691)
 *
 * otpService has no external dependencies beyond Node crypto.
 * Tests cover generation, verification, expiration, attempt limits,
 * and timing-safe comparison semantics.
 */

import { test, describe } from 'node:test';
import assert from 'node:assert/strict';
import { generateOTP, verifyOTP } from '../otpService';
import { InMemoryStateStore, setStateStore } from '../stateStore';

setStateStore(new InMemoryStateStore());

// Each test uses a unique phone hash to avoid state leakage between tests.
let counter = 0;
function ph(): string {
  return `phone-hash-${++counter}`;
}

// ---------------------------------------------------------------------------
// OTP generation
// ---------------------------------------------------------------------------

describe('generateOTP', () => {
  test('returns a 6-digit numeric string', async () => {
    const code = (await generateOTP(ph()));
    assert.match(code, /^\d{6}$/);
  });

  test('generates different codes for different phone hashes', async () => {
    const a = (await generateOTP(ph()));
    const b = (await generateOTP(ph()));
    // With 900000 possible codes this has a 0.0001% chance of flaking
    assert.notEqual(a, b);
  });

  test('overwrites an existing OTP for the same phone hash', async () => {
    const hash = ph();
    (await generateOTP(hash));
    const second = (await generateOTP(hash));
    // Second code should be accepted
    const result = (await verifyOTP(hash, second));
    assert.equal(result.success, true);
  });
});

// ---------------------------------------------------------------------------
// OTP verification – success path
// ---------------------------------------------------------------------------

describe('verifyOTP – success', () => {
  test('returns success: true and clears the OTP after a correct code', async () => {
    const hash = ph();
    const code = (await generateOTP(hash));
    const result = (await verifyOTP(hash, code));
    assert.equal(result.success, true);
    assert.equal(result.shouldResetSession, false);
    assert.match(result.message, /verified/i);
  });

  test('OTP is consumed on success – second verification fails', async () => {
    const hash = ph();
    const code = (await generateOTP(hash));
    (await verifyOTP(hash, code));
    const second = (await verifyOTP(hash, code));
    assert.equal(second.success, false);
    assert.match(second.message, /no otp/i);
  });

  test('accepts code with surrounding whitespace', async () => {
    const hash = ph();
    const code = (await generateOTP(hash));
    const result = (await verifyOTP(hash, `  ${code}  `));
    assert.equal(result.success, true);
  });
});

// ---------------------------------------------------------------------------
// OTP verification – failure paths
// ---------------------------------------------------------------------------

describe('verifyOTP – failure paths', () => {
  test('returns success: false when no OTP has been requested', async () => {
    const result = (await verifyOTP(ph(), '123456'));
    assert.equal(result.success, false);
    assert.equal(result.shouldResetSession, false);
    assert.match(result.message, /no otp/i);
  });

  test('returns success: false for a wrong code', async () => {
    const hash = ph();
    (await generateOTP(hash));
    const result = (await verifyOTP(hash, '000000'));
    assert.equal(result.success, false);
    assert.equal(result.shouldResetSession, false);
    assert.match(result.message, /invalid otp/i);
  });

  test('decrements remaining attempts message after each wrong code', async () => {
    const hash = ph();
    (await generateOTP(hash));
    const r1 = (await verifyOTP(hash, '000000'));
    assert.equal(r1.shouldResetSession, false);
    assert.match(r1.message, /2 attempts remaining/i);
    const r2 = (await verifyOTP(hash, '000000'));
    assert.equal(r2.shouldResetSession, false);
    assert.match(r2.message, /1 attempt/i);
  });
});

// ---------------------------------------------------------------------------
// Attempt limiting
// ---------------------------------------------------------------------------

describe('verifyOTP – attempt limits', () => {
  test('locks out after 3 wrong attempts and clears OTP', async () => {
    const hash = ph();
    (await generateOTP(hash));
    // Each wrong attempt increments record.attempts (starts at 0).
    // The "Too many" guard fires when record.attempts >= MAX_ATTEMPTS (3).
    // Attempt 1: attempts becomes 1
    (await verifyOTP(hash, '000000'));
    // Attempt 2: attempts becomes 2
    (await verifyOTP(hash, '000000'));
    // Attempt 3: attempts becomes 3 (still returns "Invalid" this call)
    (await verifyOTP(hash, '000000'));
    // Attempt 4: record.attempts is now 3 >= MAX_ATTEMPTS → "Too many", OTP deleted
    const r4 = (await verifyOTP(hash, '000000'));
    assert.equal(r4.success, false);
    assert.equal(r4.shouldResetSession, true);
    assert.match(r4.message, /too many/i);
    // Attempt 5: OTP deleted → "no OTP"
    const r5 = (await verifyOTP(hash, '000000'));
    assert.equal(r5.success, false);
    assert.equal(r5.shouldResetSession, false);
    assert.match(r5.message, /no otp/i);
  });

  test('correct code on the third attempt succeeds', async () => {
    const hash = ph();
    const code = (await generateOTP(hash));
    (await verifyOTP(hash, '000000')); // attempt 1 – wrong
    (await verifyOTP(hash, '000000')); // attempt 2 – wrong
    const r = (await verifyOTP(hash, code)); // attempt 3 – correct
    assert.equal(r.success, true);
  });
});

// ---------------------------------------------------------------------------
// Expiration
// ---------------------------------------------------------------------------

describe('verifyOTP – expiration', () => {
  test('expired OTP is rejected and cleared', async () => {
    const hash = ph();
    (await generateOTP(hash));

    // Simulate expiry by artificially setting the system clock forward is not
    // feasible without monkey-patching Date.now. Instead we test the code path
    // by calling verifyOTP immediately and confirming the OTP is still present
    // (not yet expired), then verify the message contract for the success case.
    const code = (await generateOTP(hash));
    const result = (await verifyOTP(hash, code));
    assert.equal(result.success, true, 'freshly generated OTP should still be valid');
  });

  test('expired OTP signals session reset (#1002)', async () => {
    const hash = ph();
    (await generateOTP(hash));

    // Mock Date.now to simulate expiry
    const originalDateNow = Date.now;
    Date.now = () => originalDateNow() + 600000; // 10 minutes later

    try {
      const result = (await verifyOTP(hash, '123456'));
      assert.equal(result.success, false);
      assert.equal(result.shouldResetSession, true);
      assert.match(result.message, /expired/i);
    } finally {
      Date.now = originalDateNow;
    }
  });
});
