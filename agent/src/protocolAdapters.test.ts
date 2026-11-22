import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
  AdapterContext,
  ProtocolAdapterRegistry,
  PhoenixDexAdapter,
  AquariusAmmAdapter,
  SorobanGateway,
} from './protocolAdapters';
import { ProtocolRiskScoringEngine, RiskDimensionScores } from './riskScoring';

const USDC = 'CUSDCMOCKUSDCMOCKUSDCMOCKUSDCMOCKUSDCMOCKUSDCMOCKUSDCMOCK1234';
const VAULT = 'GVAULTMOCKVAULTMOCKVAULTMOCKVAULTMOCKVAULTMOCKVAULTMOCK1234';

function ctxFor(contractId: string): AdapterContext {
  return {
    protocolId: 'phoenix',
    contractId,
    asset: USDC,
    vaultAddress: VAULT,
  };
}

interface RecordedCall {
  kind: 'call' | 'invoke';
  contractId: string;
  method: string;
  args: unknown[];
  response: unknown;
}

class MockGateway implements SorobanGateway {
  public calls: RecordedCall[] = [];
  public failNext = false;

  constructor(private readonly responses: Record<string, unknown> = {}) {}

  private record(
    kind: 'call' | 'invoke',
    contractId: string,
    method: string,
    args: unknown[],
  ): unknown {
    const response = this.failNext
      ? { ok: false, error: 'venue_unavailable' }
      : this.responses[method];
    this.calls.push({ kind, contractId, method, args, response });
    return response;
  }

  async call(contractId: string, method: string, args: unknown[]): Promise<unknown> {
    return this.record('call', contractId, method, args);
  }

  async invoke(contractId: string, method: string, args: unknown[]): Promise<unknown> {
    return this.record('invoke', contractId, method, args);
  }
}

describe('Protocol adapters (Issue #656)', () => {
  it('phoenix adapter implements supply, withdraw, get_apy, get_balance', async () => {
    const gateway = new MockGateway({
      supply: { ok: true, value: 1_000_000 },
      withdraw: { ok: true, value: 1_000_000 },
      get_apy: { ok: true, value: 0.071 },
      get_balance: { ok: true, value: 1_000_000 },
    });
    const adapter = new PhoenixDexAdapter(gateway);
    const ctx = ctxFor('CPHOENIX');

    const supplied = await adapter.supply(ctx, 1_000_000);
    assert.strictEqual(supplied.success, true);
    assert.strictEqual(supplied.amount, 1_000_000);

    const withdrawn = await adapter.withdraw(ctx, 1_000_000);
    assert.strictEqual(withdrawn.success, true);
    assert.strictEqual(withdrawn.amount, 1_000_000);

    const apy = await adapter.getApy(ctx);
    assert.strictEqual(apy, 0.071);

    const balance = await adapter.getBalance(ctx);
    assert.strictEqual(balance, 1_000_000);

    // Four operations used exactly the four uniform venue entrypoints.
    assert.deepStrictEqual(
      gateway.calls.map((c) => `${c.kind}:${c.method}`),
      ['invoke:supply', 'invoke:withdraw', 'call:get_apy', 'call:get_balance'],
    );
  });

  it('aquarius adapter implements supply, withdraw, get_apy, get_balance', async () => {
    const gateway = new MockGateway({
      supply: { ok: true, value: 500_000 },
      withdraw: { ok: true, value: 495_000 },
      get_apy: { ok: true, value: 0.093 },
      get_balance: { ok: true, value: 495_000 },
    });
    const adapter = new AquariusAmmAdapter(gateway);
    const ctx = { ...ctxFor('CAQUARIUS'), protocolId: 'aquarius' as const };

    assert.strictEqual(adapter.protocolId, 'aquarius');
    const supplied = await adapter.supply(ctx, 500_000);
    assert.strictEqual(supplied.amount, 500_000);

    const withdrawn = await adapter.withdraw(ctx, 0); // 0 = exit full position
    assert.strictEqual(withdrawn.amount, 495_000);

    assert.strictEqual(await adapter.getApy(ctx), 0.093);
    assert.strictEqual(await adapter.getBalance(ctx), 495_000);
  });

  it('rejects invalid supply amounts without contacting the venue', async () => {
    const gateway = new MockGateway({ supply: { ok: true, value: 1 } });
    const adapter = new PhoenixDexAdapter(gateway);
    const ctx = ctxFor('CPHOENIX');

    const zero = await adapter.supply(ctx, 0);
    assert.strictEqual(zero.success, false);
    assert.strictEqual(zero.error, 'invalid_amount');

    const negative = await adapter.supply(ctx, -5);
    assert.strictEqual(negative.success, false);

    const nan = await adapter.supply(ctx, Number.NaN);
    assert.strictEqual(nan.success, false);
    assert.strictEqual(gateway.calls.length, 0);
  });

  it('surfaces venue failures as unsuccessful results', async () => {
    const gateway = new MockGateway({});
    gateway.failNext = true;
    const adapter = new AquariusAmmAdapter(gateway);

    const result = await adapter.supply(
      { ...ctxFor('CAQUARIUS'), protocolId: 'aquarius' as const },
      1_000,
    );
    assert.strictEqual(result.success, false);
    assert.strictEqual(result.amount, 0);
    assert.strictEqual(result.error, 'venue_unavailable');
  });

  it('registry enforces registration and owner-managed whitelist', async () => {
    const registry = new ProtocolAdapterRegistry();
    const phoenix = new PhoenixDexAdapter(new MockGateway());
    const aquarius = new AquariusAmmAdapter(new MockGateway());
    registry.register(phoenix).register(aquarius);

    // Registered but not yet whitelisted -> ineligible.
    let eligibility = registry.assessEligibility('phoenix', LOW_RISK);
    assert.strictEqual(eligibility.eligible, false);
    assert.ok(eligibility.reasons.includes('not_whitelisted'));

    // Owner whitelists phoenix only.
    registry.setWhitelisted('phoenix', true);
    eligibility = registry.assessEligibility('phoenix', LOW_RISK);
    assert.strictEqual(eligibility.eligible, true);
    assert.deepStrictEqual(registry.getWhitelist(), ['phoenix']);

    // Aquarius remains blocked.
    eligibility = registry.assessEligibility('aquarius', LOW_RISK);
    assert.strictEqual(eligibility.eligible, false);

    // Owner can revoke.
    registry.setWhitelisted('phoenix', false);
    assert.strictEqual(registry.isWhitelisted('phoenix'), false);
  });

  it('registry blocks venues whose risk score exceeds the ceiling (#12)', () => {
    const registry = new ProtocolAdapterRegistry(new ProtocolRiskScoringEngine());
    registry
      .register(new PhoenixDexAdapter(new MockGateway()))
      .register(new AquariusAmmAdapter(new MockGateway()))
      .setWhitelisted('phoenix', true)
      .setWhitelisted('aquarius', true)
      .setMaxRiskScore(60);

    // Phoenix is low-risk -> eligible.
    const phoenixCheck = registry.assessEligibility('phoenix', LOW_RISK);
    assert.strictEqual(phoenixCheck.eligible, true);
    assert.strictEqual(phoenixCheck.riskCategory, 'LOW');

    // Aquarius is high-risk -> blocked with the risk reason.
    const aquariusCheck = registry.assessEligibility('aquarius', HIGH_RISK);
    assert.strictEqual(aquariusCheck.eligible, false);
    assert.ok(aquariusCheck.reasons.includes('risk_score_above_ceiling'));

    // Raising the ceiling admits it.
    registry.setMaxRiskScore(90);
    assert.strictEqual(
      registry.assessEligibility('aquarius', HIGH_RISK).eligible,
      true,
    );
  });

  it('unregistered protocols are always ineligible', () => {
    const registry = new ProtocolAdapterRegistry();
    const eligibility = registry.assessEligibility('blend', LOW_RISK);
    assert.strictEqual(eligibility.eligible, false);
    assert.ok(eligibility.reasons.includes('adapter_not_registered'));
  });
});

const LOW_RISK: RiskDimensionScores = {
  smartContractRisk: 20,
  liquidityRisk: 20,
  oracleRisk: 20,
  governanceRisk: 20,
  centralizationRisk: 20,
};

const HIGH_RISK: RiskDimensionScores = {
  smartContractRisk: 85,
  liquidityRisk: 80,
  oracleRisk: 75,
  governanceRisk: 70,
  centralizationRisk: 65,
};

