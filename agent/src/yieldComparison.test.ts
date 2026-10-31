import { describe, it, beforeEach } from 'node:test';
import assert from 'node:assert';
import {
  YieldComparisonEngine,
  ProtocolYieldData,
  evaluateYield,
  ingestLiveApys,
  LiveApyAdapter,
  LiveApyContext,
} from './yieldComparison';

describe('YieldComparisonEngine', () => {
  let engine: YieldComparisonEngine;

  beforeEach(() => {
    engine = new YieldComparisonEngine(0.005, 0.04);
  });

  const mockProtocols: ProtocolYieldData[] = [
    {
      protocolId: 'blend-usdc',
      name: 'Blend USDC Lending',
      type: 'blend',
      currentApy: 0.082,
      historicalApy7d: 0.081,
      historicalApy30d: 0.080,
      tvlUsdc: 5_000_000,
      volatility: 0.015,
      riskScore: 20,
    },
    {
      protocolId: 'dex-usdc-xlm',
      name: 'Soroswap USDC/XLM Pool',
      type: 'dex_lp',
      currentApy: 0.125,
      historicalApy7d: 0.118,
      historicalApy30d: 0.110,
      tvlUsdc: 1_200_000,
      volatility: 0.065,
      riskScore: 45,
    },
  ];

  it('ranks opportunities by risk-adjusted return', () => {
    const ranked = engine.rankOpportunities(mockProtocols);
    assert.strictEqual(ranked.length, 2);
    assert.ok(ranked[0].riskAdjustedScore > 0);
  });

  it('enforces 0.5% minimum improvement rebalance threshold', () => {
    assert.strictEqual(engine.shouldRebalance(0.080, 0.086), true);  // +0.6% > 0.5%
    assert.strictEqual(engine.shouldRebalance(0.080, 0.083), false); // +0.3% < 0.5%
  });

  it('allocates from live APYs instead of a zero placeholder', () => {
    const ranked = engine.rankOpportunities([
      fixtureProtocol('blend', 0.10),
      fixtureProtocol('dex', 0.05),
    ]);
    const byId = Object.fromEntries(ranked.map((row) => [row.protocolId, row.recommendedAllocationPercent]));
    assert.strictEqual(byId.blend, 66.67);
    assert.strictEqual(byId.dex, 33.33);
    assert.strictEqual(
      Math.round((byId.blend + byId.dex) * 100) / 100,
      100,
    );
  });

  it('uses ingested protocol APYs and ignores a stale target', () => {
    const protocols = [
      fixtureProtocol('blend', 0.082),
      fixtureProtocol('dex', 0.086),
    ];
    // +0.6% over 0.080 clears the threshold; a stale 0.081 target must not win.
    assert.strictEqual(engine.shouldRebalance(0.080, 0.081, protocols), true);
    // Best live APY is only +0.2%, below the threshold, even if the caller passes 0.20.
    assert.strictEqual(engine.shouldRebalance(0.084, 0.20, [fixtureProtocol('blend', 0.086)]), false);
    assert.strictEqual(engine.shouldRebalance(0.080, 0.20, []), false);
  });
});

function fixtureProtocol(protocolId: string, currentApy: number): ProtocolYieldData {
  return {
    protocolId,
    name: protocolId,
    type: protocolId === 'dex' ? 'dex_lp' : 'blend',
    currentApy,
    historicalApy7d: currentApy,
    historicalApy30d: currentApy,
    tvlUsdc: 1_000_000,
    volatility: 0,
    riskScore: 0,
  };
}

const LIVE_CONTEXTS: LiveApyContext[] = [
  {
    protocolId: 'blend',
    contractId: 'CBLEND',
    asset: 'CUSDC',
    vaultAddress: 'GVAULT',
  },
  {
    protocolId: 'dex',
    contractId: 'CDEX',
    asset: 'CUSDC',
    vaultAddress: 'GVAULT',
  },
];

describe('ingestLiveApys fixtures', () => {
  it('ranks allocation from adapter getApy readings', async () => {
    const reads: string[] = [];
    const adapters: LiveApyAdapter[] = [
      {
        protocolId: 'blend',
        name: 'Blend',
        async getApy(ctx) {
          reads.push(ctx.contractId);
          return 0.05;
        },
      },
      {
        protocolId: 'dex',
        name: 'DEX',
        async getApy(ctx) {
          reads.push(ctx.contractId);
          return 0.10;
        },
      },
    ];

    const live = await ingestLiveApys(adapters, LIVE_CONTEXTS, {
      blend: { type: 'blend', volatility: 0, riskScore: 0 },
      dex: { type: 'dex_lp', volatility: 0, riskScore: 0 },
    });
    assert.deepStrictEqual(reads, ['CBLEND', 'CDEX']);
    assert.deepStrictEqual(live.map((row) => row.currentApy), [0.05, 0.10]);

    const ranked = new YieldComparisonEngine().rankOpportunities(live);
    assert.strictEqual(ranked[0].protocolId, 'dex');
    assert.strictEqual(ranked[0].recommendedAllocationPercent, 66.67);
    assert.notStrictEqual(ranked[0].recommendedAllocationPercent, 0);

    assert.deepStrictEqual(await evaluateYield('balanced', 'blend', 5, live), {
      shouldRebalance: true,
      targetProtocol: 'dex',
      targetApy: 10,
    });
    assert.deepStrictEqual(await evaluateYield('conservative', 'none', 0, live), {
      shouldRebalance: true,
      targetProtocol: 'blend',
      targetApy: 5,
    });
  });

  it('does not rebalance when an adapter quote fails', async () => {
    const adapters: LiveApyAdapter[] = [
      {
        protocolId: 'blend',
        name: 'Blend',
        async getApy() {
          throw new Error('rpc down');
        },
      },
    ];
    await assert.rejects(() => ingestLiveApys(adapters, LIVE_CONTEXTS), /invalid live APY|rpc down/);
    assert.strictEqual((await evaluateYield('growth', 'blend', 1, [])).shouldRebalance, false);
  });
});

describe('evaluateYield (#467)', () => {
  it('moves balanced funds to the higher-yielding DEX when real APYs are available', async () => {
    // When live protocols are provided, it uses those instead of the fallback functions
    const liveProtocols = [
      fixtureProtocol('blend', 0.065),
      fixtureProtocol('dex', 0.082),
    ];
    const { targetApy, ...decision } = await evaluateYield('balanced', 'blend', 6.5, liveProtocols);
    assert.deepStrictEqual(decision, {
      shouldRebalance: true,
      targetProtocol: 'dex',
    });
    assert.ok(Math.abs((targetApy ?? 0) - 8.2) < 1e-9);
  });

  it('keeps conservative strategies on Blend', async () => {
    const liveProtocols = [
      fixtureProtocol('blend', 0.065),
      fixtureProtocol('dex', 0.082),
    ];
    assert.deepStrictEqual(await evaluateYield('conservative', 'none', 0, liveProtocols), {
      shouldRebalance: true,
      targetProtocol: 'blend',
      targetApy: 6.5,
    });
    assert.strictEqual((await evaluateYield('conservative', 'blend', 6.5, liveProtocols)).shouldRebalance, false);
  });

  it('does not rebalance below the 0.5pp improvement threshold', async () => {
    const liveProtocols = [
      fixtureProtocol('blend', 0.078),
      fixtureProtocol('dex', 0.082),
    ];
    assert.strictEqual((await evaluateYield('growth', 'blend', 7.8, liveProtocols)).shouldRebalance, false);
  });

  it('does not rebalance when fetch functions return null (fail-safe #1003)', async () => {
    // Without live protocols, evaluateYield calls fetchBlendApy and fetchDexApy
    // These return null when not configured or on error, triggering the fail-safe
    const result = await evaluateYield('balanced', 'blend', 6.5);
    assert.strictEqual(result.shouldRebalance, false);
  });
});
