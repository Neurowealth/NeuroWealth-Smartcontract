import { describe, it } from 'node:test';
import assert from 'node:assert';
import { Keypair, nativeToScVal, xdr } from '@stellar/stellar-sdk';
import { decodeVaultTransfer, rawToUsdc, vaultEventType } from './vaultEventPayload';

const user = Keypair.random().publicKey();

function transferEvent(topic: string, amount: bigint, shares = amount) {
  return {
    topic: [nativeToScVal(topic, { type: 'symbol' }), nativeToScVal(user, { type: 'address' })],
    value: nativeToScVal(
      { amount, shares, user },
      { type: { amount: ['symbol', 'i128'], shares: ['symbol', 'i128'], user: ['symbol', 'address'] } },
    ),
  };
}

describe('Vault event payload decoding (#748)', () => {
  it('classifies deposit and withdraw topics', () => {
    assert.strictEqual(vaultEventType(transferEvent('deposit', BigInt(1)).topic), 'deposit');
    assert.strictEqual(vaultEventType(transferEvent('withdraw', BigInt(1)).topic), 'withdraw');
    assert.strictEqual(vaultEventType([nativeToScVal('pause', { type: 'symbol' })]), null);
  });

  it('rejects topic substring collisions (#746)', () => {
    // Substring matches must not trigger event classification
    assert.strictEqual(vaultEventType([nativeToScVal('emergency_withdraw', { type: 'symbol' })]), null);
    assert.strictEqual(vaultEventType([nativeToScVal('deposit_batch', { type: 'symbol' })]), null);
    assert.strictEqual(vaultEventType([nativeToScVal('auto_deposit', { type: 'symbol' })]), null);
    assert.strictEqual(vaultEventType([nativeToScVal('withdraw_queued', { type: 'symbol' })]), null);
    assert.strictEqual(vaultEventType(['emergency_withdraw']), null);
    assert.strictEqual(vaultEventType(['deposit_tokens']), null);

    // Secondary topic containing deposit/withdraw must not trigger classification
    assert.strictEqual(
      vaultEventType([
        nativeToScVal('custom_event', { type: 'symbol' }),
        nativeToScVal('deposit', { type: 'symbol' }),
      ]),
      null,
    );
  });

  it('uses the real on-chain amount instead of a placeholder', () => {
    const payload = decodeVaultTransfer(transferEvent('withdraw', BigInt('2500000000')));
    assert.ok(payload);
    assert.strictEqual(payload.amount, 250);
    assert.strictEqual(payload.rawAmount, BigInt('2500000000'));
    assert.strictEqual(payload.user, user);
  });

  it('keeps fractional USDC precision', () => {
    assert.strictEqual(rawToUsdc(BigInt(15_000_000)), 1.5);
    assert.strictEqual(rawToUsdc(BigInt(1)), 0.0000001);
  });

  it('returns null when the payload has no amount', () => {
    assert.strictEqual(decodeVaultTransfer({ topic: [], value: xdr.ScVal.scvVoid() }), null);
    assert.strictEqual(decodeVaultTransfer({ topic: [], value: nativeToScVal({ foo: 1 }) }), null);
  });
});
