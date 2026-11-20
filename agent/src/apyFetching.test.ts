/**
 * Unit tests for APY fetching functions (Issue #1003)
 *
 * Tests cover live Blend supply APR (`get_supply_apr`) and DEX fee APR
 * (`get_fee_apr`) fetched through RPC simulations of the contracts, with
 * caching and staleness detection. RPC responses are stubbed via the live
 * server object.
 */

import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert';
import { Keypair, xdr } from '@stellar/stellar-sdk';
import { server } from './eventListener';
import { fetchBlendApy, fetchDexApy, _setBlendApyCache, _setDexApyCache } from './yieldComparison';

const serverAny = server as unknown as Record<string, unknown>;

type SimulateStub = (tx: unknown) => Promise<unknown>;
let simulateStub: SimulateStub = async () => ({ error: new Error('RPC unavailable') });
serverAny.simulateTransaction = (tx: unknown) => simulateStub(tx);

function scvU64(value: number) {
  return xdr.ScVal.scvU64(new xdr.Uint64(value));
}

function successSimulation(retval: xdr.ScVal) {
  return { transactionData: { sequence: '1' }, result: { retval } };
}

describe('APY fetching with caching and staleness (#1003)', () => {
  beforeEach(() => {
    // Clear cache before each test
    _setBlendApyCache(0);
    _setDexApyCache(0);
    process.env.BLEND_CONTRACT_ID = 'CAR46JFIVWEGO5MFBV2V6I6E36OY5SP7EVJC3BV5KLZ4WVUJBQQTNNOA';
    process.env.DEX_POOL_ID = 'CDQ6ZKZLYPJOGSRPMUJZVHD3KGVLF2R5O3Y6EPIZZ2Z6OTTXQ5E27737';
    process.env.BLEND_APY_STALENESS_MS = '1000'; // 1 second for testing
    process.env.DEX_APY_STALENESS_MS = '1000';
    process.env.SOROBAN_PUBLIC_KEY = Keypair.random().publicKey();
    simulateStub = async () => ({ error: new Error('RPC unavailable') });
  });

  afterEach(() => {
    delete process.env.BLEND_CONTRACT_ID;
    delete process.env.DEX_POOL_ID;
    delete process.env.BLEND_APY_STALENESS_MS;
    delete process.env.DEX_APY_STALENESS_MS;
    delete process.env.SOROBAN_PUBLIC_KEY;
    _setBlendApyCache(0);
    _setDexApyCache(0);
  });

  it('returns null when BLEND_CONTRACT_ID is not configured', async () => {
    delete process.env.BLEND_CONTRACT_ID;
    const result = await fetchBlendApy();
    assert.strictEqual(result, null);
  });

  it('returns null when DEX_POOL_ID is not configured', async () => {
    delete process.env.DEX_POOL_ID;
    const result = await fetchDexApy();
    assert.strictEqual(result, null);
  });

  it('returns null when the APY simulation fails (fail-safe)', async () => {
    simulateStub = async () => ({ error: new Error('simulation failed') });
    const blendResult = await fetchBlendApy();
    const dexResult = await fetchDexApy();
    assert.strictEqual(blendResult, null);
    assert.strictEqual(dexResult, null);
  });

  it('caches APY values and returns cached value within staleness window', async () => {
    // Manually set cache to simulate a successful fetch
    _setBlendApyCache(6.5);
    _setDexApyCache(8.2);

    const blendResult = await fetchBlendApy();
    const dexResult = await fetchDexApy();

    // Should return cached values
    assert.strictEqual(blendResult, 6.5);
    assert.strictEqual(dexResult, 8.2);
  });

  it('stale cache triggers refetch and returns the simulated APY', async () => {
    // Set a very short staleness window
    process.env.BLEND_APY_STALENESS_MS = '10';

    // Manually set cache
    _setBlendApyCache(6.5);

    // Wait for cache to expire
    await new Promise(resolve => setTimeout(resolve, 20));

    // The simulation now succeeds; the stale value is replaced
    simulateStub = async () => successSimulation(scvU64(65));
    const result = await fetchBlendApy();
    assert.strictEqual(result, 65);
  });

  it('returns the simulated Blend supply APR and reuses it while fresh', async () => {
    let calls = 0;
    simulateStub = async () => {
      calls += 1;
      return successSimulation(scvU64(65));
    };

    assert.strictEqual(await fetchBlendApy(), 65);
    assert.strictEqual(await fetchBlendApy(), 65); // served from cache
    assert.strictEqual(calls, 1);
  });

  it('returns the simulated DEX fee APR', async () => {
    simulateStub = async () => successSimulation(scvU64(82));
    assert.strictEqual(await fetchDexApy(), 82);
  });
});
