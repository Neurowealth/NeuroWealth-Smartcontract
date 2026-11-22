import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseIntent } from '../intentParser';
import { ContractLimits, getContractLimits, validateIntent } from '../contractLimits';

const limits: ContractLimits = { minDeposit: 0.1, maxDeposit: 1000, userDepositCap: 1000, tvlCap: 10000 };
const check = (msg: string) => validateIntent(parseIntent(msg), limits);

test('defaults mirror the contract initialize values', () => {
  assert.deepEqual(getContractLimits(), limits);
});

test('accepts deposits within caps', () => {
  assert.equal(check('deposit 50 USDC').ok, true);
  assert.equal(check('deposit 0.1').ok, true);
  assert.equal(check('deposit 1,000 into growth').ok, true);
});

test('rejects deposits outside caps', () => {
  assert.equal(check('deposit 0.05').ok, false);
  assert.equal(check('deposit 1000.01').ok, false);
  assert.equal(check('deposit 5,000').ok, false);
  assert.equal(check('deposit -5').ok, false);
  assert.equal(check('deposit 0').ok, false);
  assert.equal(check('deposit').ok, false);
  assert.equal(check('deposit 1.123456789').ok, false);
});

test('user cap applies when tighter than max deposit', () => {
  const tight = { ...limits, userDepositCap: 200 };
  assert.equal(validateIntent(parseIntent('deposit 300'), tight).ok, false);
  assert.equal(validateIntent(parseIntent('deposit 200'), tight).ok, true);
});

test('validates withdraw amounts', () => {
  assert.equal(check('withdraw 50').ok, true);
  assert.equal(check('withdraw all').ok, true);
  assert.equal(check('withdraw').ok, false);
  assert.equal(check('withdraw -1').ok, false);
});

test('validates strategy names against the contract set', () => {
  assert.equal(check('switch to growth').ok, true);
  assert.equal(check('change strategy to conservative').ok, true);
  const bad = check('switch to aggressive');
  assert.equal(bad.ok, false);
  assert.match(bad.ok ? '' : bad.error, /aggressive/);
  assert.equal(check('change strategy').ok, false);
});

test('env overrides are read in stroops', () => {
  process.env.VAULT_MAX_DEPOSIT = '5000000000';
  try {
    assert.equal(getContractLimits().maxDeposit, 500);
  } finally {
    delete process.env.VAULT_MAX_DEPOSIT;
  }
  process.env.VAULT_MIN_DEPOSIT = 'abc';
  try {
    assert.throws(() => getContractLimits());
  } finally {
    delete process.env.VAULT_MIN_DEPOSIT;
  }
});
