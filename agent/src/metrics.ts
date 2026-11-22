/**
 * Agent monitoring metrics engine (Issue #652).
 *
 * Aggregates the performance metrics the dashboard needs into a single
 * snapshot, exposed over HTTP by `metricsApi.ts`:
 *
 *  - Real-time TVL and yield
 *  - Rebalance history with before/after APY attribution
 *  - Protocol allocation breakdown (for the dashboard pie chart)
 *  - Agent uptime and health status (heartbeat-based)
 *  - Error-rate tracking
 *  - Gas-cost analysis per rebalance
 *  - User growth / deposit-withdrawal trends
 *  - Configurable alert thresholds (TVL drop, failed rebalances, error rate)
 *
 * The engine keeps bounded in-memory history (oldest entries are dropped) and
 * accepts an injectable clock for deterministic tests.
 */

const MAX_HISTORY = 10_000;

export interface ProtocolAllocation {
  protocol: string;
  /** Deployed amount in stroops. */
  amount: number;
  /** Share of TVL in basis points. */
  shareBps: number;
  apy?: number;
}

export interface RebalancePerformanceRecord {
  at: number;
  fromProtocol: string;
  toProtocol: string;
  apyBefore: number;
  apyAfter: number;
  gasCostStroops?: number;
  success: boolean;
  trigger?: string;
  error?: string;
}

export interface VaultFlowEvent {
  at: number;
  kind: 'deposit' | 'withdrawal';
  user: string;
  amount: number;
}

export interface ErrorEvent {
  at: number;
  kind: string;
  message?: string;
}

export interface HeartbeatRecord {
  at: number;
  healthy: boolean;
}

export interface AlertThresholds {
  /** Alert when TVL dropped this many bps from its observed peak. */
  tvlDropBps: number;
  /** Alert after this many consecutive failed rebalances. */
  maxConsecutiveFailures: number;
  /** Alert when the error rate over the window exceeds this many bps. */
  maxErrorRateBps?: number;
  /** Observation window for the error rate (default 1h). */
  errorWindowMs?: number;
  /** Alert when no healthy heartbeat for this long (default 5m). */
  heartbeatStaleMs?: number;
}

export const DEFAULT_ALERT_THRESHOLDS: AlertThresholds = {
  tvlDropBps: 1_000,
  maxConsecutiveFailures: 3,
  maxErrorRateBps: 500,
  errorWindowMs: 3_600_000,
  heartbeatStaleMs: 300_000,
};

export interface MetricAlert {
  type: 'tvl_drop' | 'consecutive_failures' | 'error_rate' | 'heartbeat_stale';
  message: string;
  at: number;
  context?: Record<string, unknown>;
}

export interface UserTrends {
  deposits: number;
  withdrawals: number;
  net: number;
  uniqueUsers: number;
  /** Deposit/withdrawal totals bucketed per hour (oldest first). */
  hourly: { hour: number; deposits: number; withdrawals: number }[];
}

export interface GasAnalysis {
  rebalances: number;
  totalGasStroops: number;
  avgGasStroops: number;
  maxGasStroops: number;
}

export interface MetricsSnapshot {
  generatedAt: number;
  tvl: number;
  currentApy: number;
  peakTvl: number;
  tvlDropFromPeakBps: number;
  allocations: ProtocolAllocation[];
  rebalanceHistory: RebalancePerformanceRecord[];
  agentHealthy: boolean;
  uptimePct: number;
  lastHeartbeatAt: number | null;
  errorRate: { total: number; errors: number; rateBps: number; windowMs: number };
  gasAnalysis: GasAnalysis;
  userTrends: UserTrends;
  alerts: MetricAlert[];
}

/** In-memory metrics aggregation engine backing the dashboard API. */
export class MetricsEngine {
  private tvl = 0;
  private currentApy = 0;
  private peakTvl = 0;
  private allocations: ProtocolAllocation[] = [];
  private rebalances: RebalancePerformanceRecord[] = [];
  private flows: VaultFlowEvent[] = [];
  private errors: ErrorEvent[] = [];
  private operationCount = 0;
  private heartbeats: HeartbeatRecord[] = [];
  private readonly thresholds: AlertThresholds;

  constructor(
    thresholds: Partial<AlertThresholds> = {},
    private readonly now: () => number = () => Date.now(),
  ) {
    this.thresholds = { ...DEFAULT_ALERT_THRESHOLDS, ...thresholds };
  }

  /** Updates the real-time TVL and current blended APY. */
  recordTvl(tvl: number, apy: number): void {
    this.tvl = tvl;
    this.currentApy = apy;
    if (tvl > this.peakTvl) this.peakTvl = tvl;
  }

  /** Replaces the protocol allocation breakdown. */
  recordAllocations(allocations: Omit<ProtocolAllocation, 'shareBps'>[]): void {
    const total = allocations.reduce((sum, a) => sum + a.amount, 0);
    this.allocations = allocations.map((a) => ({
      ...a,
      shareBps: total > 0 ? Math.round((a.amount / total) * 10_000) : 0,
    }));
  }

  recordRebalance(record: Omit<RebalancePerformanceRecord, 'at'>): void {
    this.push(this.rebalances, { ...record, at: this.now() });
  }

  recordFlow(kind: 'deposit' | 'withdrawal', user: string, amount: number): void {
    this.operationCount += 1;
    this.push(this.flows, { at: this.now(), kind, user, amount });
  }

  /** Records an operational error (agent, venue, RPC...). */
  recordError(kind: string, message?: string): void {
    this.operationCount += 1;
    this.push(this.errors, { at: this.now(), kind, message });
  }

  /** Manual or automatic operation counter (no error). */
  recordOperation(): void {
    this.operationCount += 1;
  }

  recordHeartbeat(healthy = true): void {
    this.push(this.heartbeats, { at: this.now(), healthy });
  }

  /** Runs every threshold check and returns the currently firing alerts. */
  checkAlerts(): MetricAlert[] {
    const now = this.now();
    const alerts: MetricAlert[] = [];

    if (this.peakTvl > 0) {
      const dropBps = Math.round(((this.peakTvl - this.tvl) / this.peakTvl) * 10_000);
      if (dropBps >= this.thresholds.tvlDropBps) {
        alerts.push({
          type: 'tvl_drop',
          message: `TVL dropped ${dropBps}bps from peak`,
          at: now,
          context: { dropBps, tvl: this.tvl, peakTvl: this.peakTvl },
        });
      }
    }

    const consecutive = this.countConsecutiveFailures();
    if (consecutive >= this.thresholds.maxConsecutiveFailures) {
      alerts.push({
        type: 'consecutive_failures',
        message: `${consecutive} consecutive failed rebalances`,
        at: now,
        context: { consecutive, threshold: this.thresholds.maxConsecutiveFailures },
      });
    }

    const windowMs = this.thresholds.errorWindowMs ?? 3_600_000;
    const windowErrors = this.errors.filter((e) => now - e.at <= windowMs).length;
    const rateBps = this.operationCount > 0
      ? Math.round((windowErrors / this.operationCount) * 10_000)
      : 0;
    const maxErrorRateBps = this.thresholds.maxErrorRateBps ?? 500;
    if (rateBps >= maxErrorRateBps) {
      alerts.push({
        type: 'error_rate',
        message: `Error rate ${rateBps}bps exceeds ${maxErrorRateBps}bps`,
        at: now,
        context: { rateBps, errors: windowErrors, operations: this.operationCount },
      });
    }

    const staleMs = this.thresholds.heartbeatStaleMs ?? 300_000;
    const last = this.heartbeats[this.heartbeats.length - 1];
    if (last && (now - last.at > staleMs || !last.healthy)) {
      alerts.push({
        type: 'heartbeat_stale',
        message: last.healthy
          ? `No heartbeat for ${now - last.at}ms`
          : 'Last heartbeat reported unhealthy',
        at: now,
        context: { lastHeartbeatAt: last.at, healthy: last.healthy },
      });
    }

    return alerts;
  }

  /** Builds the full dashboard snapshot. */
  getSnapshot(): MetricsSnapshot {
    const now = this.now();
    const recent = this.recentRebalances(now);
    const gasRecords = recent.filter((r) => typeof r.gasCostStroops === 'number');
    const totalGas = gasRecords.reduce((s, r) => s + (r.gasCostStroops ?? 0), 0);
    const recentHeartbeats = this.heartbeats.filter(
      (h) => now - h.at <= 24 * 3_600_000,
    );
    const healthyCount = recentHeartbeats.filter((h) => h.healthy).length;
    const last = this.heartbeats[this.heartbeats.length - 1] ?? null;
    const windowMs = this.thresholds.errorWindowMs ?? 3_600_000;
    const windowErrors = this.errors.filter((e) => now - e.at <= windowMs).length;

    return {
      generatedAt: now,
      tvl: this.tvl,
      currentApy: this.currentApy,
      peakTvl: this.peakTvl,
      tvlDropFromPeakBps:
        this.peakTvl > 0
          ? Math.round(((this.peakTvl - this.tvl) / this.peakTvl) * 10_000)
          : 0,
      allocations: [...this.allocations],
      rebalanceHistory: [...recent],
      agentHealthy:
        !!last && last.healthy && now - last.at <= (this.thresholds.heartbeatStaleMs ?? 300_000),
      uptimePct:
        recentHeartbeats.length > 0
          ? (healthyCount / recentHeartbeats.length) * 100
          : 0,
      lastHeartbeatAt: last ? last.at : null,
      errorRate: {
        total: this.operationCount,
        errors: windowErrors,
        rateBps:
          this.operationCount > 0
            ? Math.round((windowErrors / this.operationCount) * 10_000)
            : 0,
        windowMs,
      },
      gasAnalysis: {
        rebalances: gasRecords.length,
        totalGasStroops: totalGas,
        avgGasStroops:
          gasRecords.length > 0 ? Math.round(totalGas / gasRecords.length) : 0,
        maxGasStroops: gasRecords.reduce((m, r) => Math.max(m, r.gasCostStroops ?? 0), 0),
      },
      userTrends: this.buildUserTrends(),
      alerts: this.checkAlerts(),
    };
  }

  private recentRebalances(now: number): RebalancePerformanceRecord[] {
    const windowMs = 7 * 24 * 3_600_000;
    return this.rebalances.filter((r) => now - r.at <= windowMs);
  }

  private countConsecutiveFailures(): number {
    let consecutive = 0;
    for (let i = this.rebalances.length - 1; i >= 0; i--) {
      if (this.rebalances[i].success) break;
      consecutive += 1;
    }
    return consecutive;
  }

  private buildUserTrends(): UserTrends {
    const deposits = this.flows.filter((f) => f.kind === 'deposit');
    const withdrawals = this.flows.filter((f) => f.kind === 'withdrawal');
    const sum = (xs: VaultFlowEvent[]) => xs.reduce((s, f) => s + f.amount, 0);
    const users = new Set(this.flows.map((f) => f.user));

    const hourlyMap = new Map<number, { deposits: number; withdrawals: number }>();
    for (const f of this.flows) {
      const hour = Math.floor(f.at / 3_600_000) * 3_600_000;
      const bucket = hourlyMap.get(hour) ?? { deposits: 0, withdrawals: 0 };
      if (f.kind === 'deposit') bucket.deposits += f.amount;
      else bucket.withdrawals += f.amount;
      hourlyMap.set(hour, bucket);
    }
    const hourly = [...hourlyMap.entries()]
      .sort(([a], [b]) => a - b)
      .map(([hour, b]) => ({ hour, ...b }));

    return {
      deposits: sum(deposits),
      withdrawals: sum(withdrawals),
      net: sum(deposits) - sum(withdrawals),
      uniqueUsers: users.size,
      hourly,
    };
  }

  private push<T extends { at: number }>(arr: T[], item: T): void {
    arr.push(item);
    if (arr.length > MAX_HISTORY) arr.splice(0, arr.length - MAX_HISTORY);
  }
}

