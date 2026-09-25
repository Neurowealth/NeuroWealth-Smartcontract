import { describe, it, expect, vi, beforeEach } from 'vitest';
import { fetchVaultState, shortenAddress, server } from './stellar';
import { Keypair, nativeToScVal } from '@stellar/stellar-sdk';

describe('stellar lib fetchVaultState (#753)', () => {
  const dummyUser = Keypair.random().publicKey();

  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('returns default zero state when userAddress is not supplied', async () => {
    const simulateSpy = vi.spyOn(server, 'simulateTransaction');
    const state = await fetchVaultState();

    expect(state).toEqual({
      balance: 0,
      strategy: 'Balanced',
      exchangeRate: 1.0,
      apy: 0,
    });
    expect(simulateSpy).not.toHaveBeenCalled();
  });

  it('fetches and returns live on-chain values when userAddress is supplied', async () => {
    vi.spyOn(server, 'simulateTransaction').mockImplementation(async (tx: any) => {
      const op = tx.operations[0];
      const fnName = typeof op.func?._value?.functionName === 'function'
        ? op.func._value.functionName().toString()
        : String(op.func?._value?.functionName || op.function || '');

      let retval;
      if (fnName === 'get_balance') {
        retval = nativeToScVal(15000000000n, { type: 'i128' }); // 1,500 USDC
      } else if (fnName === 'get_user_strategy') {
        retval = nativeToScVal('growth', { type: 'symbol' });
      } else if (fnName === 'get_exchange_rate') {
        retval = nativeToScVal(11000000n, { type: 'i128' }); // 1.10
      } else if (fnName === 'get_total_assets') {
        retval = nativeToScVal(110000000000n, { type: 'i128' }); // 11,000
      } else if (fnName === 'get_total_shares') {
        retval = nativeToScVal(100000000000n, { type: 'i128' }); // 10,000
      }

      return {
        transactionData: {},
        result: { retval },
      } as any;
    });

    const state = await fetchVaultState(dummyUser);

    expect(state.balance).toBe(1500);
    expect(state.strategy).toBe('Growth');
    expect(state.exchangeRate).toBe(1.1);
    expect(state.apy).toBeGreaterThan(0);
  });

  it('maps conservative and balanced strategies correctly', async () => {
    vi.spyOn(server, 'simulateTransaction').mockImplementation(async (tx: any) => {
      const op = tx.operations[0];
      const fnName = typeof op.func?._value?.functionName === 'function'
        ? op.func._value.functionName().toString()
        : String(op.func?._value?.functionName || op.function || '');

      let retval;
      if (fnName === 'get_balance') {
        retval = nativeToScVal(500000000n, { type: 'i128' });
      } else if (fnName === 'get_user_strategy') {
        retval = nativeToScVal('conservative', { type: 'symbol' });
      } else if (fnName === 'get_exchange_rate') {
        retval = nativeToScVal(10000000n, { type: 'i128' });
      } else if (fnName === 'get_total_assets') {
        retval = nativeToScVal(10000000000n, { type: 'i128' });
      } else if (fnName === 'get_total_shares') {
        retval = nativeToScVal(10000000000n, { type: 'i128' });
      }

      return {
        transactionData: {},
        result: { retval },
      } as any;
    });

    const state = await fetchVaultState(dummyUser);
    expect(state.balance).toBe(50);
    expect(state.strategy).toBe('Conservative');
    expect(state.exchangeRate).toBe(1.0);
  });

  it('falls back to safe defaults when RPC throws or simulation fails', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.spyOn(server, 'simulateTransaction').mockRejectedValue(new Error('RPC connection failed'));

    const state = await fetchVaultState(dummyUser);

    expect(state).toEqual({
      balance: 0,
      strategy: 'Balanced',
      exchangeRate: 1.0,
      apy: 0,
    });
    warnSpy.mockRestore();
  });

  it('shortenAddress shortens long public keys', () => {
    const shortened = shortenAddress(dummyUser);
    expect(shortened).toBe(`${dummyUser.substring(0, 6)}...${dummyUser.substring(dummyUser.length - 4)}`);
    expect(shortenAddress('')).toBe('');
  });
});
