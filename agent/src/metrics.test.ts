import { describe, it, after } from 'node:test';
import assert from 'node:assert';
import { MetricsEngine } from './metrics';
import { createMetricsApp, startMetricsServer } from './metricsApi';

const T0 = 1_700_000_000_000;

function makeEngine(): { engine: MetricsEngine; now: { t: number } } {
  const now = { t: T0 };
  const engine = new MetricsEngine(
    { tvlDropBps: 1_000, maxConsecutiveFailures: 3, heartbeatStaleMs: 300_000 },
    () => now.t,
  );
  return { engine, now };
}

describe('Metrics engine (Issue #652)', () => {
  it('tracks TVL, yield, and peak-drop in bps', () => {
    const { engine } = makeEngine();
    engine.recordTvl(1_000_000, 0.07);
    engine.recordTvl(900_000, 0.06);
    const snap = engine.getSnapshot();
    assert.strictEqual(snap.tvl, 900_000);
    assert.strictEqual(snap.currentApy, 0.06);
    assert.strictEqual(snap.peakTvl, 1_000_000);
    assert.strictEqual(snap.tvlDropFromPeakBps, 1_000);
    assert.ok(snap.alerts.some((a) => a.type === 'tvl_drop'));
  });

  it('computes allocation shares in bps for the pie chart', () => {
    const { engine } = makeEngine();
    engine.recordAllocations([
      { protocol: 'blend', amount: 600_000 },
      { protocol: 'phoenix', amount: 300_000 },
      { protocol: 'aquarius', amount: 100_000 },
    ]);
    const snap = engine.getSnapshot();
    assert.deepStrictEqual(
      snap.allocations.map((a) => `${a.protocol}:${a.shareBps}`),
      ['blend:6000', 'phoenix:3000', 'aquarius:1000'],
    );
  });

  it('records rebalance history with before/after APY and gas analysis', () => {
    const { engine } = makeEngine();
    engine.recordRebalance({
      fromProtocol: 'blend',
      toProtocol: 'phoenix',
      apyBefore: 0.06,
      apyAfter: 0.085,
      gasCostStroops: 10_000,
      success: true,
      trigger: 'scheduled',
    });
    engine.recordRebalance({
      fromProtocol: 'phoenix',
      toProtocol: 'aquarius',
      apyBefore: 0.085,
      apyAfter: 0.08,
      gasCostStroops: 30_000,
      success: true,
      trigger: 'tvl_change',
    });
    const snap = engine.getSnapshot();
    assert.strictEqual(snap.rebalanceHistory.length, 2);
    assert.strictEqual(snap.rebalanceHistory[1].apyAfter, 0.08);
    assert.strictEqual(snap.gasAnalysis.rebalances, 2);
    assert.strictEqual(snap.gasAnalysis.totalGasStroops, 40_000);
    assert.strictEqual(snap.gasAnalysis.avgGasStroops, 20_000);
    assert.strictEqual(snap.gasAnalysis.maxGasStroops, 30_000);
  });

  it('computes uptime from heartbeats and flags unhealthy agent', () => {
    const { engine, now } = makeEngine();
    engine.recordHeartbeat(true);
    now.t += 60_000;
    engine.recordHeartbeat(true);
    now.t += 60_000;
    engine.recordHeartbeat(true);
    assert.strictEqual(engine.getSnapshot().uptimePct, 100);
    assert.strictEqual(engine.getSnapshot().agentHealthy, true);

    // Heartbeat goes stale (> 5 min default threshold).
    now.t += 400_000;
    const snap = engine.getSnapshot();
    assert.strictEqual(snap.agentHealthy, false);
    assert.ok(snap.alerts.some((a) => a.type === 'heartbeat_stale'));
  });

  it('tracks error rate and fires the error-rate alert', () => {
    const { engine } = makeEngine();
    for (let i = 0; i < 90; i++) engine.recordOperation();
    for (let i = 0; i < 10; i++) engine.recordError('rpc_timeout');
    const snap = engine.getSnapshot();
    assert.strictEqual(snap.errorRate.total, 100);
    assert.strictEqual(snap.errorRate.errors, 10);
    assert.strictEqual(snap.errorRate.rateBps, 1_000);
    assert.ok(snap.alerts.some((a) => a.type === 'error_rate'));
  });

  it('aggregates user deposit/withdrawal trends', () => {
    const { engine, now } = makeEngine();
    engine.recordFlow('deposit', 'GUSER1', 100);
    now.t += 3_600_000; // next hour bucket
    engine.recordFlow('deposit', 'GUSER1', 200);
    engine.recordFlow('withdrawal', 'GUSER2', 50);
    const trends = engine.getSnapshot().userTrends;
    assert.strictEqual(trends.deposits, 300);
    assert.strictEqual(trends.withdrawals, 50);
    assert.strictEqual(trends.net, 250);
    assert.strictEqual(trends.uniqueUsers, 2);
    assert.strictEqual(trends.hourly.length, 2);
    assert.strictEqual(trends.hourly[1].deposits, 200);
  });

  it('alerts on consecutive failed rebalances', () => {
    const { engine } = makeEngine();
    engine.recordRebalance({ fromProtocol: 'blend', toProtocol: 'dex', apyBefore: 0.06, apyAfter: 0.06, success: true });
    engine.recordRebalance({ fromProtocol: 'dex', toProtocol: 'blend', apyBefore: 0.06, apyAfter: 0.06, success: false });
    engine.recordRebalance({ fromProtocol: 'blend', toProtocol: 'dex', apyBefore: 0.06, apyAfter: 0.06, success: false });
    assert.ok(!engine.checkAlerts().some((a) => a.type === 'consecutive_failures'));
    engine.recordRebalance({ fromProtocol: 'dex', toProtocol: 'blend', apyBefore: 0.06, apyAfter: 0.06, success: false });
    assert.ok(engine.checkAlerts().some((a) => a.type === 'consecutive_failures'));
  });
});

describe('Metrics API (Issue #652)', () => {
  const { engine, now } = makeEngine();
  engine.recordTvl(1_000_000, 0.07);
  engine.recordHeartbeat(true);
  const app = createMetricsApp(engine);
  let server: import('node:http').Server;
  let baseUrl = '';

  after(async () => {
    if (server) await new Promise<void>((resolve) => server.close(() => resolve()));
  });


  it('serves /api/metrics/snapshot with aggregated data', async () => {
    server = await startMetricsServer(engine, 0);
    const addr = server.address();
    const port = typeof addr === 'object' && addr ? addr.port : 0;

    const res = await fetch(`http://127.0.0.1:${port}/api/metrics/snapshot`);
    assert.strictEqual(res.status, 200);
    const body = (await res.json()) as MetricsSnapshotShape;
    assert.strictEqual(body.tvl, 1_000_000);
    assert.strictEqual(body.currentApy, 0.07);
    assert.strictEqual(typeof body.gasAnalysis.avgGasStroops, 'number');
    assert.ok(Array.isArray(body.alerts));
  });

  it('serves /health as healthy with a fresh heartbeat', async () => {
    server = server ?? (await startMetricsServer(engine, 0));
    const addr = server!.address();
    const port = typeof addr === 'object' && addr ? addr.port : 0;
    const res = await fetch(`http://127.0.0.1:${port}/health`);
    assert.strictEqual(res.status, 200);
    const body = (await res.json()) as { status: string; agentHealthy: boolean };
    assert.strictEqual(body.status, 'healthy');
    assert.strictEqual(body.agentHealthy, true);
  });

  it('serves /health as 503 when the heartbeat is stale', async () => {
    now.t += 400_000; // past the 5-minute stale threshold
    const addr = server!.address();
    const port = typeof addr === 'object' && addr ? addr.port : 0;
    const res = await fetch(`http://127.0.0.1:${port}/health`);
    assert.strictEqual(res.status, 503);
    const body = (await res.json()) as { status: string };
    assert.strictEqual(body.status, 'degraded');
  });

  it('serves /api/metrics/alerts', async () => {
    const addr = server!.address();
    const port = typeof addr === 'object' && addr ? addr.port : 0;
    const res = await fetch(`http://127.0.0.1:${port}/api/metrics/alerts`);
    assert.strictEqual(res.status, 200);
    const body = (await res.json()) as { alerts: unknown[] };
    assert.ok(Array.isArray(body.alerts));
  });
});

interface MetricsSnapshotShape {
  tvl: number;
  currentApy: number;
  gasAnalysis: { avgGasStroops: number };
  alerts: unknown[];
}
