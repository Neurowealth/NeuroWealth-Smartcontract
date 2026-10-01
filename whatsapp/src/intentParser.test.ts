import { describe, it } from 'node:test';
import assert from 'node:assert';
import { parseIntent } from './intentParser';

// Rule-based intent parsing (#691). The handlers act on whatever this returns,
// so the boundaries - an exact greeting, a six-digit code, which keyword wins
// when several appear - are what matter.

describe('parseIntent codes and greetings (#691)', () => {
  it('recognises a six-digit code as an OTP', () => {
    const intent = parseIntent(' 123456 ');
    assert.strictEqual(intent.type, 'OTP_CODE');
    assert.strictEqual(intent.otpCode, '123456');
  });

  it('does not treat five or seven digits as an OTP', () => {
    assert.notStrictEqual(parseIntent('12345').type, 'OTP_CODE');
    assert.notStrictEqual(parseIntent('1234567').type, 'OTP_CODE');
  });

  it('recognises the greeting words', () => {
    for (const word of ['hi', 'hello', 'hey', 'start', 'menu', 'help']) {
      assert.strictEqual(parseIntent(word).type, 'GREETING', word + ' should greet');
    }
  });

  it('treats a greeting with extra words as unknown, not as a greeting', () => {
    assert.strictEqual(parseIntent('hi there').type, 'UNKNOWN');
  });

  it('is case and whitespace insensitive', () => {
    assert.strictEqual(parseIntent('  HELLO  ').type, 'GREETING');
  });
});

describe('parseIntent queries (#691)', () => {
  it('reads balance questions', () => {
    assert.strictEqual(parseIntent('what is my balance').type, 'BALANCE');
    assert.strictEqual(parseIntent('show my funds').type, 'BALANCE');
  });

  it('reads earnings questions', () => {
    assert.strictEqual(parseIntent('how much have i made').type, 'EARNINGS');
    assert.strictEqual(parseIntent('show me my yield').type, 'EARNINGS');
  });

  it('reads apy questions', () => {
    assert.strictEqual(parseIntent('what is the apy').type, 'APY');
    assert.strictEqual(parseIntent('what interest rate do i get').type, 'APY');
  });

  it('lets balance win when a message mentions several things', () => {
    // The balance check runs before deposit and earnings.
    assert.strictEqual(parseIntent('deposit or show balance').type, 'BALANCE');
  });
});

describe('parseIntent actions (#691)', () => {
  it('extracts the amount from a deposit', () => {
    const intent = parseIntent('deposit 250 usdc');
    assert.strictEqual(intent.type, 'DEPOSIT');
    assert.strictEqual(intent.amount, 250);
  });

  it('extracts a decimal amount', () => {
    assert.strictEqual(parseIntent('deposit 12.5').amount, 12.5);
  });

  it('carries the strategy when one is named', () => {
    assert.strictEqual(parseIntent('deposit 100 into growth').strategy, 'growth');
  });

  it('accepts the alternate deposit phrasings', () => {
    assert.strictEqual(parseIntent('put in 10').type, 'DEPOSIT');
    assert.strictEqual(parseIntent('add money 10').type, 'DEPOSIT');
  });

  it('leaves the amount undefined when no number is given', () => {
    const intent = parseIntent('deposit please');
    assert.strictEqual(intent.type, 'DEPOSIT');
    assert.strictEqual(intent.amount, undefined);
  });

  it("marks 'withdraw everything' as a full withdrawal", () => {
    const intent = parseIntent('withdraw all');
    assert.strictEqual(intent.type, 'WITHDRAW');
    assert.strictEqual(intent.withdrawAll, true);
    assert.strictEqual(intent.amount, undefined);
  });

  it('reads a partial withdrawal amount', () => {
    const intent = parseIntent('withdraw 50');
    assert.strictEqual(intent.amount, 50);
    assert.strictEqual(intent.withdrawAll, false);
  });

  it("treats 'cash out everything' as a full withdrawal", () => {
    assert.strictEqual(parseIntent('cash out everything').withdrawAll, true);
  });

  it('reads a strategy switch', () => {
    const intent = parseIntent('switch to growth');
    assert.strictEqual(intent.type, 'STRATEGY');
    assert.strictEqual(intent.strategy, 'growth');
  });

  it('does not read a strategy without a recognised name', () => {
    assert.strictEqual(parseIntent('switch to aggressive').type, 'UNKNOWN');
  });

  it('falls back to unknown for anything else', () => {
    assert.strictEqual(parseIntent('what is the weather').type, 'UNKNOWN');
  });

  it('keeps the raw text for logging', () => {
    assert.strictEqual(parseIntent('  Deposit 5  ').rawText, 'Deposit 5');
  });
});
