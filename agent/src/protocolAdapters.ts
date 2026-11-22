/**
 * Protocol adapter layer for additional Stellar DeFi venues (Issue #656).
 *
 * Phase 2 protocols:
 *  - `phoenix`  — Phoenix orderbook DEX (orderbook-based liquidity provision)
 *  - `aquarius` — Aquarius AMM (constant-product liquidity pools)
 *
 * Every adapter implements the same four operations exposed by the vault's
 * on-chain adapter interface (see `docs/PROTOCOL_ADAPTER_INTERFACE.md`):
 *
 *   1. `supply`      — deploy USDC into the venue
 *   2. `withdraw`    — pull USDC back out of the venue
 *   3. `get_apy`     — current annualized yield (decimal fraction, e.g. 0.085)
 *   4. `get_balance` — the vault's currently deployed balance (USDC stroops)
 *
 * Adapters are registered in the `ProtocolAdapterRegistry`, which enforces
 * the owner-managed protocol whitelist and the risk-scoring gate (issue #12)
 * before a venue becomes eligible to receive funds. Adding a new venue is a
 * matter of implementing `ProtocolAdapter` and registering it — no changes
 * to the yield comparison engine or rebalance scheduler are required.
 */

import { ProtocolRiskScoringEngine, RiskDimensionScores } from './riskScoring';

export type ProtocolId = 'blend' | 'dex' | 'phoenix' | 'aquarius' | 'none';

/** Describes where and for whom an adapter operation executes. */
export interface AdapterContext {
  /** Logical protocol identifier (e.g. `phoenix`). */
  protocolId: ProtocolId;
  /** Adapter/venue contract id the operation targets. */
  contractId: string;
  /** Asset being deployed (USDC contract id). */
  asset: string;
  /** Address that owns the deployed position (the vault). */
  vaultAddress: string;
}

/** Outcome of a state-changing adapter operation. */
export interface AdapterResult {
  success: boolean;
  /** Actual amount supplied/withdrawn in stroops (0 when nothing moved). */
  amount: number;
  /** Short machine-readable reason when `success` is false. */
  error?: string;
}

/**
 * Minimal Soroban transport used by adapters.
 *
 * Kept as an interface so unit tests can inject mock transports and so the
 * concrete RPC binding (Soroban RPC via `@stellar/stellar-sdk`) lives in one
 * place.
 */
export interface SorobanGateway {
  /** Read-only contract invocation. */
  call(contractId: string, method: string, args: unknown[]): Promise<unknown>;
  /** State-changing contract invocation (signed by the agent). */
  invoke(contractId: string, method: string, args: unknown[]): Promise<unknown>;
}

/** Uniform protocol adapter interface (mirrors the on-chain adapter contract). */
export interface ProtocolAdapter {
  readonly protocolId: ProtocolId;
  readonly name: string;
  /** Deploy `amount` stroops of `ctx.asset` into the venue. */
  supply(ctx: AdapterContext, amount: number): Promise<AdapterResult>;
  /** Withdraw `amount` stroops from the venue (0 = exit the full position). */
  withdraw(ctx: AdapterContext, amount: number): Promise<AdapterResult>;
  /** Current APY as a decimal fraction (0.085 = 8.5%). */
  getApy(ctx: AdapterContext): Promise<number>;
  /** Vault's deployed balance in the venue, in stroops. */
  getBalance(ctx: AdapterContext): Promise<number>;
}

/** Envelope returned by every venue contract method: `{ ok, value | error }`. */
interface VenueEnvelope {
  ok?: boolean;
  value?: unknown;
  error?: string;
}

/** Shared implementation for venue-backed adapters using a Soroban gateway. */
abstract class GatewayAdapter implements ProtocolAdapter {
  abstract readonly protocolId: ProtocolId;
  abstract readonly name: string;

  protected constructor(protected readonly gateway: SorobanGateway) {}

  async supply(ctx: AdapterContext, amount: number): Promise<AdapterResult> {
    if (!Number.isFinite(amount) || amount <= 0) {
      return { success: false, amount: 0, error: 'invalid_amount' };
    }
    try {
      const raw = await this.gateway.invoke(ctx.contractId, 'supply', [
        ctx.vaultAddress,
        ctx.asset,
        amount,
      ]);
      return { success: true, amount: expectNumber(raw, 'supply') };
    } catch (err) {
      return {
        success: false,
        amount: 0,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }

  async withdraw(ctx: AdapterContext, amount: number): Promise<AdapterResult> {
    if (!Number.isFinite(amount) || amount < 0) {
      return { success: false, amount: 0, error: 'invalid_amount' };
    }
    try {
      const raw = await this.gateway.invoke(ctx.contractId, 'withdraw', [
        ctx.vaultAddress,
        ctx.asset,
        amount,
      ]);
      return { success: true, amount: expectNumber(raw, 'withdraw') };
    } catch (err) {
      return {
        success: false,
        amount: 0,
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }

  async getApy(ctx: AdapterContext): Promise<number> {
    const raw = await this.gateway.call(ctx.contractId, 'get_apy', []);
    return expectNumber(raw, 'get_apy');
  }

  async getBalance(ctx: AdapterContext): Promise<number> {
    const raw = await this.gateway.call(ctx.contractId, 'get_balance', [
      ctx.asset,
      ctx.vaultAddress,
    ]);
    return expectNumber(raw, 'get_balance');
  }
}

/**
 * Adapter for the Phoenix orderbook DEX.
 *
 * Phoenix provides yield through maker rebates and spread capture on the
 * orderbook. Deployment wraps the on-chain single-asset adapter contract,
 * which manages bid/ask placement internally.
 */
export class PhoenixDexAdapter extends GatewayAdapter {
  readonly protocolId: ProtocolId = 'phoenix';
  readonly name = 'Phoenix Orderbook DEX';

  constructor(gateway: SorobanGateway) {
    super(gateway);
  }
}

/**
 * Adapter for the Aquarius AMM.
 *
 * Aquarius provides yield through constant-product liquidity pools. The
 * on-chain adapter wraps single-sided USDC deposits into the two-asset pool
 * and exposes USDC-equivalent balances.
 */
export class AquariusAmmAdapter extends GatewayAdapter {
  readonly protocolId: ProtocolId = 'aquarius';
  readonly name = 'Aquarius AMM';

  constructor(gateway: SorobanGateway) {
    super(gateway);
  }
}

/** Normalizes the venue envelope into a strict number, or throws. */
function expectNumber(raw: unknown, what: string): number {
  const envelope = raw as VenueEnvelope | undefined;
  if (envelope && typeof envelope === 'object' && envelope.ok === false) {
    throw new Error(envelope.error || 'venue_error');
  }
  const value =
    envelope && typeof envelope === 'object' && 'value' in envelope
      ? envelope.value
      : raw;
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    throw new Error(`${what}: non-numeric response from venue`);
  }
  return value;
}

/** Risk gate outcome for a protocol venue. */
export interface AdapterEligibility {
  eligible: boolean;
  riskScore: number;
  riskCategory: 'LOW' | 'MEDIUM' | 'HIGH' | 'CRITICAL';
  reasons: string[];
}

/**
 * Registry of protocol adapters with owner-managed whitelist and risk gating.
 *
 * Mirrors the on-chain `ProtocolWhitelist` instance storage: only whitelisted
 * protocols with a configured adapter may receive vault funds. The registry
 * additionally applies the risk-scoring engine (issue #12) so that a venue
 * must be both *authorized* (whitelisted) and *safe* (risk score below the
 * configured ceiling) before deployment.
 */
export class ProtocolAdapterRegistry {
  private readonly adapters = new Map<ProtocolId, ProtocolAdapter>();
  private readonly whitelist = new Set<ProtocolId>();
  private maxRiskScore = 80;
  private readonly riskEngine: ProtocolRiskScoringEngine;

  constructor(riskEngine: ProtocolRiskScoringEngine = new ProtocolRiskScoringEngine()) {
    this.riskEngine = riskEngine;
  }

  /** Registers (or replaces) the adapter for a protocol. */
  register(adapter: ProtocolAdapter): this {
    this.adapters.set(adapter.protocolId, adapter);
    return this;
  }

  /** Returns the adapter for a protocol, if registered. */
  get(protocolId: ProtocolId): ProtocolAdapter | undefined {
    return this.adapters.get(protocolId);
  }

  /** Lists all registered adapters. */
  list(): ProtocolAdapter[] {
    return [...this.adapters.values()];
  }

  /** Owner action: enables/disables a protocol on the whitelist. */
  setWhitelisted(protocolId: ProtocolId, enabled: boolean): this {
    if (enabled) {
      this.whitelist.add(protocolId);
    } else {
      this.whitelist.delete(protocolId);
    }
    return this;
  }

  /** Whether the protocol is on the owner-managed whitelist. */
  isWhitelisted(protocolId: ProtocolId): boolean {
    return this.whitelist.has(protocolId);
  }

  /** Snapshot of the current whitelist. */
  getWhitelist(): ProtocolId[] {
    return [...this.whitelist];
  }

  /** Sets the maximum acceptable composite risk score (1-100). */
  setMaxRiskScore(maxRiskScore: number): this {
    this.maxRiskScore = maxRiskScore;
    return this;
  }

  /**
   * Full eligibility check: registered + whitelisted + risk score below the
   * ceiling. Reasons are returned for observability and alerting.
   */
  assessEligibility(
    protocolId: ProtocolId,
    riskScores: RiskDimensionScores,
  ): AdapterEligibility {
    const reasons: string[] = [];
    const composite = this.riskEngine.calculateCompositeRisk(riskScores);
    const category = this.riskEngine.getRiskCategory(composite);

    if (!this.adapters.has(protocolId)) {
      reasons.push('adapter_not_registered');
    }
    if (!this.isWhitelisted(protocolId)) {
      reasons.push('not_whitelisted');
    }
    if (composite > this.maxRiskScore) {
      reasons.push('risk_score_above_ceiling');
    }

    return {
      eligible: reasons.length === 0,
      riskScore: composite,
      riskCategory: category,
      reasons,
    };
  }
}

