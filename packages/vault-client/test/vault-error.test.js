const assert = require('node:assert/strict');
const test = require('node:test');

const { VaultError, VaultErrorCode } = require('../dist/index.js');

test('VaultError wraps contract errors with typed codes', () => {
  const err = VaultError.fromContractError({ code: VaultErrorCode.ExceedsTvlCap, message: 'TVL cap exceeded' });

  assert.ok(err instanceof VaultError);
  assert.equal(err.code, VaultErrorCode.ExceedsTvlCap);
  assert.match(err.message, /TVL cap exceeded/);
});

test('VaultError constructor preserves the provided code', () => {
  const err = new VaultError(VaultErrorCode.InvalidStrategy, 'strategy is invalid');

  assert.equal(err.code, VaultErrorCode.InvalidStrategy);
  assert.equal(err.message, 'strategy is invalid');
});

test('VaultErrorCode defines distinct codes for LegacyUnauthorizedAgentError (90) and UnauthorizedAgentError (102)', () => {
  assert.equal(VaultErrorCode.LegacyUnauthorizedAgentError, 90);
  assert.equal(VaultErrorCode.UnauthorizedAgentError, 102);
  assert.notEqual(VaultErrorCode.LegacyUnauthorizedAgentError, VaultErrorCode.UnauthorizedAgentError);
});
