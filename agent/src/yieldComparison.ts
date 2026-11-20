import type { RebalanceDecision } from './userStrategies';
import logger from './logger';
import { Account, Contract, Networks, rpc, scValToNative, TransactionBuilder } from '@stellar/stellar-sdk';

export interface ProtocolYieldData {
  protocolId: string;
  name: string;
  type: 'blend' | 'dex_lp' | 'other';
  currentApy: number; // e.g. 0.085 for 8.5%
  historicalApy7d: number;
  historicalApy30d: number;
  tvlUsdc: number;
  volatility: number; // annualized volatility e.g. 0.02
  riskScore: number;  // 1-100 from risk scoring engine
}

export interface YieldOpportunity {
  protocolId: string;
  name: string;
  netApy: number;
  riskAdjustedScore: number;
  recommendedAllocationPercent: number;
}

export class YieldComparisonEngine {
  private readonly minImprovementThreshold: number;
  private readonly riskFreeRate: number;

  constructor(minImprovementThreshold = 0.005, riskFreeRate = 0.04) {
    this.minImprovementThreshold = minImprovementThreshold;
    this.riskFreeRate = riskFreeRate;
  }

  public calculateSharpeRatio(apy: number, volatility: number): number {
    if (volatility <= 0) return apy / 0.01;
    return (apy - this.riskFreeRate) / volatility;
  }

  public rankOpportunities(protocols: ProtocolYieldData[]): YieldOpportunity[] {
    const scored = protocols.map((p) => {
      const sharpe = this.calculateSharpeRatio(p.currentApy, p.volatility);
      // Risk-adjusted metric combines sharpe with inverse risk penalty
      const riskPenalty = (100 - p.riskScore) / 100;
      const riskAdjustedScore = sharpe * riskPenalty;

      return {
        protocolId: p.protocolId,
        name: p.name,
        netApy: p.currentApy,
        riskAdjustedScore: Math.round(riskAdjustedScore * 100) / 100,
        weight: Math.max(riskAdjustedScore, 0),
      };
    });

    const allocations = allocateByWeight(scored.map((row) => row.weight));
    return scored
      .map((row, index) => ({
        protocolId: row.protocolId,
        name: row.name,
        netApy: row.netApy,
        riskAdjustedScore: row.riskAdjustedScore,
        recommendedAllocationPercent: allocations[index] ?? 0,
      }))
      .sort((a, b) => b.riskAdjustedScore - a.riskAdjustedScore);
  }

  /**
   * Rebalance when the improvement clears the threshold.
   *
   * When live protocol quotes are supplied, the target APY is taken from that
   * set (the highest net APY) instead of the caller-supplied number. An empty
   * quote list never rebalances.
   */
  public shouldRebalance(
    currentApy: number,
    targetApy: number,
    protocols?: ProtocolYieldData[],
  ): boolean {
    if (protocols) {
      if (protocols.length === 0) return false;
      const best = this.rankOpportunities(protocols)[0];
      if (!best) return false;
      return best.netApy - currentApy >= this.minImprovementThreshold;
    }
    return targetApy - currentApy >= this.minImprovementThreshold;
  }
}

/** Live venue the comparison engine can query for a current APY. */
export interface LiveApyAdapter {
  readonly protocolId: string;
  readonly name: string;
  /** Decimal APY, e.g. 0.085 for 8.5%, matching protocol adapter `getApy`. */
  getApy(ctx: LiveApyContext): Promise<number>;
}

export interface LiveApyContext {
  protocolId: string;
  contractId: string;
  asset: string;
  vaultAddress: string;
}

export interface LiveApyProfile {
  type: ProtocolYieldData['type'];
  historicalApy7d?: number;
  historicalApy30d?: number;
  tvlUsdc?: number;
  volatility?: number;
  riskScore?: number;
}

/**
 * Read current APYs from protocol adapters and shape them for allocation.
 * One adapter failure fails the ingest so callers do not rank a partial book.
 */
export async function ingestLiveApys(
  adapters: LiveApyAdapter[],
  contexts: LiveApyContext[],
  profiles: Record<string, LiveApyProfile> = {},
): Promise<ProtocolYieldData[]> {
  const quotes: ProtocolYieldData[] = [];

  for (const adapter of adapters) {
    const ctx = contexts.find((candidate) => candidate.protocolId === adapter.protocolId);
    if (!ctx) continue;

    const currentApy = await adapter.getApy(ctx);
    if (!Number.isFinite(currentApy) || currentApy < 0) {
      throw new Error(`invalid live APY for ${adapter.protocolId}`);
    }

    const profile = profiles[adapter.protocolId] ?? { type: 'other' };
    quotes.push({
      protocolId: adapter.protocolId,
      name: adapter.name,
      type: profile.type,
      currentApy,
      historicalApy7d: profile.historicalApy7d ?? currentApy,
      historicalApy30d: profile.historicalApy30d ?? currentApy,
      tvlUsdc: profile.tvlUsdc ?? 0,
      volatility: profile.volatility ?? 0,
      riskScore: profile.riskScore ?? 50,
    });
  }

  return quotes;
}

/** Percent weights from non-negative scores. They sum to 100 when any weight is positive. */
function allocateByWeight(weights: number[]): number[] {
  if (weights.length === 0) return [];
  const total = weights.reduce((sum, weight) => sum + weight, 0);
  const raw = total > 0
    ? weights.map((weight) => (weight / total) * 100)
    : weights.map(() => 100 / weights.length);
  const rounded = raw.map((value) => Math.round(value * 100) / 100);
  const drift = Math.round((100 - rounded.reduce((sum, value) => sum + value, 0)) * 100) / 100;
  if (drift !== 0) {
    let largest = 0;
    for (let i = 1; i < rounded.length; i++) {
      if (rounded[i] > rounded[largest]) largest = i;
    }
    rounded[largest] = Math.round((rounded[largest] + drift) * 100) / 100;
  }
  return rounded;
}

/** Minimum APY improvement, in percentage points, before a rebalance is proposed. */
export const MIN_REBALANCE_IMPROVEMENT_PCT = 0.5;

/** Inclusion fee for APY read-only simulations (stroops). */
const APY_SIMULATION_FEE = '100';
/** Timeout for APY simulations in seconds. */
const APY_SIMULATION_TIMEOUT_SECONDS = 30;

interface CachedApy {
  value: number;
  timestamp: number;
}

const DEFAULT_STALENESS_MS = 5 * 60 * 1000; // 5 minutes
const blendCache: CachedApy = { value: 0, timestamp: 0 };
const dexCache: CachedApy = { value: 0, timestamp: 0 };

/**
 * Reads a contract's numeric return value via a read-only
 * `simulateTransaction` call — the same pattern as `fetchVaultTotalAssets` in
 * sorobanTx.ts. Returns null when the contract cannot be read, so callers
 * never fall back to fabricated APYs.
 */
async function simulateContractApy(contractId: string, method: string): Promise<number | null> {
  const { server } = await import('./eventListener');
  const sourcePublicKey = process.env.SOROBAN_PUBLIC_KEY;
  if (!contractId || !sourcePublicKey) return null;

  try {
    const tx = new TransactionBuilder(new Account(sourcePublicKey, '0'), {
      fee: APY_SIMULATION_FEE,
      networkPassphrase: process.env.SOROBAN_NETWORK_PASSPHRASE || Networks.TESTNET,
    })
      .addOperation(new Contract(contractId).call(method))
      .setTimeout(APY_SIMULATION_TIMEOUT_SECONDS)
      .build();
    const simulation = await server.simulateTransaction(tx);
    if (!rpc.Api.isSimulationSuccess(simulation) || !simulation.result?.retval) return null;
    const value = Number(scValToNative(simulation.result.retval));
    if (!Number.isFinite(value) || value < 0) {
      logger?.warn({ contractId, method, value }, `${method} returned an invalid APY`);
      return null;
    }
    return value;
  } catch (error) {
    logger?.warn({ error: error instanceof Error ? error.message : String(error), contractId, method }, `Failed to simulate ${method}`);
    return null;
  }
}

/**
 * Fetches the real Blend supply APR from the pool contract via a
 * `get_supply_apr` RPC simulation. Results are cached with staleness
 * detection. A failed or stale fetch returns null.
 */
export async function fetchBlendApy(): Promise<number | null> {
  const stalenessMs = parseInt(process.env.BLEND_APY_STALENESS_MS || String(DEFAULT_STALENESS_MS), 10);
  const now = Date.now();

  // Return cached value if still fresh
  if (blendCache.value > 0 && now - blendCache.timestamp < stalenessMs) {
    return blendCache.value;
  }

  try {
    const blendContractId = process.env.BLEND_CONTRACT_ID;
    if (!blendContractId) {
      logger?.warn('BLEND_CONTRACT_ID not configured; using null APY (no rebalance)');
      return null;
    }

    // Query the Blend pool contract for supply APR
    const apy = await simulateContractApy(blendContractId, 'get_supply_apr');
    if (apy === null) {
      logger?.warn('Blend APY simulation failed; using null APY (no rebalance)');
      return null;
    }
    blendCache.value = apy;
    blendCache.timestamp = now;
    return apy;
  } catch (error) {
    logger?.error({ error: error instanceof Error ? error.message : String(error) }, 'Failed to fetch Blend APY');
    return null;
  }
}

/**
 * Updates the Blend APY cache (for testing purposes).
 * In production, this would be updated by fetchBlendApy on successful RPC calls.
 */
export function _setBlendApyCache(value: number): void {
  blendCache.value = value;
  blendCache.timestamp = Date.now();
}

/**
 * Fetches the real DEX fee APR from the pool contract via a `get_fee_apr` RPC
 * simulation. Results are cached with staleness detection. A failed or stale
 * fetch returns null.
 */
export async function fetchDexApy(): Promise<number | null> {
  const stalenessMs = parseInt(process.env.DEX_APY_STALENESS_MS || String(DEFAULT_STALENESS_MS), 10);
  const now = Date.now();

  // Return cached value if still fresh
  if (dexCache.value > 0 && now - dexCache.timestamp < stalenessMs) {
    return dexCache.value;
  }

  try {
    const dexPoolId = process.env.DEX_POOL_ID;
    if (!dexPoolId) {
      logger?.warn('DEX_POOL_ID not configured; using null APY (no rebalance)');
      return null;
    }

    // Query the DEX pool for fee APR
    const apy = await simulateContractApy(dexPoolId, 'get_fee_apr');
    if (apy === null) {
      logger?.warn('DEX APY simulation failed; using null APY (no rebalance)');
      return null;
    }
    dexCache.value = apy;
    dexCache.timestamp = now;
    return apy;
  } catch (error) {
    logger?.error({ error: error instanceof Error ? error.message : String(error) }, 'Failed to fetch DEX APY');
    return null;
  }
}

/**
 * Updates the DEX APY cache (for testing purposes).
 * In production, this would be updated by fetchDexApy on successful RPC calls.
 */
export function _setDexApyCache(value: number): void {
  dexCache.value = value;
  dexCache.timestamp = Date.now();
}

/**
 * Decide whether to move funds for a given strategy (#467).
 *
 * APYs are in percent (e.g. 6.5), matching `rebalances.apy_after`.
 * Conservative strategies stay on Blend lending; balanced/growth may use the
 * DEX when it pays more. A rebalance is proposed only when the target
 * protocol differs and improves APY by more than 0.5 percentage points.
 * Fetch failures never trigger a rebalance.
 *
 * Restored after the class refactor dropped it while `index.ts` and
 * `eventListener.ts` still called it.
 */
export async function evaluateYield(
  strategy: string,
  currentProtocol: string,
  currentApy: number,
  liveProtocols?: ProtocolYieldData[],
): Promise<RebalanceDecision> {
  try {
    let blendApy: number;
    let dexApy: number;
    if (liveProtocols) {
      if (liveProtocols.length === 0) return { shouldRebalance: false };
      const blend = liveProtocols.find((protocol) => protocol.type === 'blend');
      const dex = liveProtocols.find((protocol) => protocol.type === 'dex_lp');
      // Adapter quotes are decimal fractions; this function compares percent points.
      blendApy = (blend?.currentApy ?? 0) * 100;
      dexApy = (dex?.currentApy ?? 0) * 100;
    } else {
      const [blendResult, dexResult] = await Promise.all([fetchBlendApy(), fetchDexApy()]);
      // Fail-safe: if either fetch fails or is stale, do not rebalance
      if (blendResult === null || dexResult === null) {
        logger?.warn({ blendApy: blendResult, dexApy: dexResult }, 'APY fetch failed or stale; skipping rebalance');
        return { shouldRebalance: false };
      }
      blendApy = blendResult;
      dexApy = dexResult;
    }

    let targetProtocol = 'blend';
    let targetApy = blendApy;
    if (strategy !== 'conservative' && dexApy > blendApy) {
      targetProtocol = 'dex';
      targetApy = dexApy;
    }

    const improves = targetProtocol !== currentProtocol
      && targetApy - currentApy > MIN_REBALANCE_IMPROVEMENT_PCT;
    if (!improves) return { shouldRebalance: false };

    if (liveProtocols) {
      const engine = new YieldComparisonEngine();
      const eligible = targetProtocol === 'dex'
        ? liveProtocols.filter((protocol) => protocol.type === 'dex_lp')
        : liveProtocols.filter((protocol) => protocol.type === 'blend');
      if (!engine.shouldRebalance(currentApy / 100, targetApy / 100, eligible)) {
        return { shouldRebalance: false };
      }
    }

    return { shouldRebalance: true, targetProtocol, targetApy };
  } catch {
    return { shouldRebalance: false };
  }
}
