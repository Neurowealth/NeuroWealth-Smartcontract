const assert = require('node:assert/strict');
const test = require('node:test');

const {
  EMPTY_VAULT_STATE,
  deriveVaultState,
  mapVaultStrategy,
  shortenAddress,
} = require('../dist/stellar.js');

test('shortenAddress keeps both ends of a public key', () => {
  const address = 'GABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890';
  assert.equal(
    shortenAddress(address),
    `${address.substring(0, 6)}...${address.substring(address.length - 4)}`,
  );
  assert.equal(shortenAddress(address, 6), 'GABCDEFG...567890');
  assert.equal(shortenAddress(''), '');
});

test('mapVaultStrategy accepts the vault symbols and defaults the rest', () => {
  assert.equal(mapVaultStrategy('conservative'), 'Conservative');
  assert.equal(mapVaultStrategy('Balanced'), 'Balanced');
  assert.equal(mapVaultStrategy('GROWTH'), 'Growth');
  assert.equal(mapVaultStrategy(''), 'Balanced');
  assert.equal(mapVaultStrategy('unknown'), 'Balanced');
});

test('deriveVaultState uses live readings, including a share-price fallback', () => {
  const quoted = deriveVaultState({
    balance: 1500,
    rawStrategy: 'growth',
    exchangeRateRaw: 1.1,
    totalAssets: 11000,
    totalShares: 10000,
  });
  assert.equal(quoted.balance, 1500);
  assert.equal(quoted.strategy, 'Growth');
  assert.equal(quoted.exchangeRate, 1.1);
  assert.equal(quoted.apy, 3650);

  const fallback = deriveVaultState({
    balance: 50,
    rawStrategy: 'conservative',
    exchangeRateRaw: 0,
    totalAssets: 200,
    totalShares: 100,
  });
  assert.equal(fallback.strategy, 'Conservative');
  assert.equal(fallback.exchangeRate, 2);
  assert.equal(fallback.apy, 36500);

  assert.deepEqual(
    deriveVaultState({
      balance: 0,
      rawStrategy: '',
      exchangeRateRaw: 0,
      totalAssets: 0,
      totalShares: 0,
    }),
    EMPTY_VAULT_STATE,
  );
});
