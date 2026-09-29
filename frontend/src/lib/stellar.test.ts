import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Keypair, nativeToScVal } from '@stellar/stellar-sdk';

/** stellar.ts reads NEXT_PUBLIC_VAULT_CONTRACT_ID at module load and throws if
 * unset, so the env must be stubbed and the module freshly imported per test. */
async function importStellar() {
  vi.resetModules();
  return import('./stellar');
}

describe('stellar module env validation', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('throws at import time when NEXT_PUBLIC_VAULT_CONTRACT_ID is unset', async () => {
    vi.stubEnv('NEXT_PUBLIC_VAULT_CONTRACT_ID', '');
    await expect(importStellar()).rejects.toThrow(/NEXT_PUBLIC_VAULT_CONTRACT_ID must be set/);
  });
});

describe('stellar lib fetchVaultState (#753)', () => {
  const dummyUser = Keypair.random().publicKey();

  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('returns default zero state when userAddress is not supplied', async () => {
    const { fetchVaultState, server } = await importStellar();
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
    const { fetchVaultState, server } = await importStellar();
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
    const { fetchVaultState, server } = await importStellar();
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
    const { fetchVaultState, server } = await importStellar();
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
});

describe('shortenAddress', () => {
  const USER_ADDRESS = 'GABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890';

  it('shortens long public keys', async () => {
    const { shortenAddress } = await importStellar();
    const shortened = shortenAddress(USER_ADDRESS);
    expect(shortened).toBe(`${USER_ADDRESS.substring(0, 6)}...${USER_ADDRESS.substring(USER_ADDRESS.length - 4)}`);
  });

  it('respects a custom character count', async () => {
    const { shortenAddress } = await importStellar();
    expect(shortenAddress(USER_ADDRESS, 6)).toBe('GABCDEFG...567890');
  });

  it('returns an empty string for an empty address', async () => {
    const { shortenAddress } = await importStellar();
    expect(shortenAddress('')).toBe('');
  });
});
