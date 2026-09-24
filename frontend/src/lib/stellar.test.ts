import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { simulateContractInvocationMock, ServerMock } = vi.hoisted(() => {
  const simulateContractInvocationMock = vi.fn();
  const ServerMock = vi.fn().mockImplementation(() => ({
    simulateContractInvocation: simulateContractInvocationMock,
  }));
  return { simulateContractInvocationMock, ServerMock };
});

vi.mock('@stellar/stellar-sdk', () => ({
  Address: class {
    constructor(private value: string) {}
    toScVal() {
      return `scval:${this.value}`;
    }
  },
  Contract: class {
    constructor(public id: string) {}
  },
  rpc: { Server: ServerMock },
  scValToNative: (v: unknown) => v,
}));

const VAULT_CONTRACT_ID = 'CDLZFC3SYJYD7M6LJEFAPCHRLHAFKP6WYTHRF3EGO5CYD3EP4GZGM37T';
const USER_ADDRESS = 'GABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890';

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

describe('fetchVaultState', () => {
  beforeEach(() => {
    vi.stubEnv('NEXT_PUBLIC_VAULT_CONTRACT_ID', VAULT_CONTRACT_ID);
    simulateContractInvocationMock.mockReset();
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('returns the default state without calling the RPC when no user address is given', async () => {
    const { fetchVaultState } = await importStellar();

    const state = await fetchVaultState(undefined);

    expect(state).toEqual({ balance: 0, strategy: 'Balanced', exchangeRate: 1.0, apy: 0 });
    expect(simulateContractInvocationMock).not.toHaveBeenCalled();
  });

  it('parses balance, strategy, exchange rate, and computes APY from simulated calls', async () => {
    simulateContractInvocationMock
      .mockResolvedValueOnce({ result: { retval: 50_0000000 } }) // balance
      .mockResolvedValueOnce({ result: { retval: 'growth' } }) // strategy
      .mockResolvedValueOnce({ result: { retval: 0 } }) // exchangeRate (unset -> derive)
      .mockResolvedValueOnce({ result: { retval: 110_0000000 } }) // totalAssets
      .mockResolvedValueOnce({ result: { retval: 100_0000000 } }); // totalShares

    const { fetchVaultState } = await importStellar();
    const state = await fetchVaultState(USER_ADDRESS);

    expect(state.balance).toBe(50);
    expect(state.strategy).toBe('Growth');
    expect(state.exchangeRate).toBeCloseTo(1.1, 5);
    expect(state.apy).toBeGreaterThan(0);
  });

  it('defaults strategy to Balanced when the contract returns an unrecognized value', async () => {
    simulateContractInvocationMock
      .mockResolvedValueOnce({ result: { retval: 0 } })
      .mockResolvedValueOnce({ result: { retval: 'unknown-strategy' } })
      .mockResolvedValueOnce({ result: { retval: 1_0000000 } })
      .mockResolvedValueOnce({ result: { retval: 0 } })
      .mockResolvedValueOnce({ result: { retval: 0 } });

    const { fetchVaultState } = await importStellar();
    const state = await fetchVaultState(USER_ADDRESS);

    expect(state.strategy).toBe('Balanced');
  });

  it('falls back to the default state when a simulated call rejects', async () => {
    simulateContractInvocationMock.mockRejectedValue(new Error('RPC unavailable'));

    const { fetchVaultState } = await importStellar();
    const state = await fetchVaultState(USER_ADDRESS);

    expect(state).toEqual({ balance: 0, strategy: 'Balanced', exchangeRate: 1.0, apy: 0 });
  });

  it('reports zero APY when there are no shares yet', async () => {
    simulateContractInvocationMock
      .mockResolvedValueOnce({ result: { retval: 0 } })
      .mockResolvedValueOnce({ result: { retval: 'conservative' } })
      .mockResolvedValueOnce({ result: { retval: 0 } })
      .mockResolvedValueOnce({ result: { retval: 0 } })
      .mockResolvedValueOnce({ result: { retval: 0 } });

    const { fetchVaultState } = await importStellar();
    const state = await fetchVaultState(USER_ADDRESS);

    expect(state.apy).toBe(0);
    expect(state.exchangeRate).toBe(1.0);
  });
});

describe('shortenAddress', () => {
  beforeEach(() => {
    vi.stubEnv('NEXT_PUBLIC_VAULT_CONTRACT_ID', VAULT_CONTRACT_ID);
  });

  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('shortens a long address to head...tail', async () => {
    const { shortenAddress } = await importStellar();
    expect(shortenAddress(USER_ADDRESS)).toBe('GABCDE...7890');
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
