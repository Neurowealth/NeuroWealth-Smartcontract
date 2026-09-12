/**
 * Core types for monitoring system
 */

export interface MonitoringConfig {
  contractId: string;
  rpcUrl: string;
  networkPassphrase: string;
  pollIntervalSeconds: number;
  alertWebhooks: AlertWebhook[];
  thresholds: AlertThresholds;
  metricsBackendUrl?: string;
  enablePauseDrill?: boolean;
  /** Insurance fund alerting. When omitted, no insurance alert is raised. */
  insurance?: InsuranceConfig;
}

export interface InsuranceConfig {
  /** Balance below this level raises an insurance-fund alert. */
  minimumFundLevel: number;
  /** Minimum time between insurance alerts, defaults to one hour. */
  alertCooldownMs?: number;
}

export interface AlertWebhook {
  name: string;
  type: "slack" | "discord" | "telegram" | "webhook";
  url: string;
  severity?: "info" | "warning" | "critical";
}

export interface AlertThresholds {
  tvlDropPercentage: number; // 20 = 20% drop
  withdrawalSpikeFactor: number; // 3 = 3x avg
  pauseDurationLedgers: number; // ~24h = 17280
  capSaturationPercentage: number; // 95 = 95%
}

export interface HealthMetrics {
  timestamp: number;
  ledgerSequence: number;
  tvl: bigint;
  totalShares: bigint;
  totalDeposits: bigint;
  isPaused: boolean;
  currentProtocol: string;
  owner: string;
  agent: string;
  sharePrice: number;
  tvlCap: bigint;
  userDepositCap: bigint;
  pendingUpgrade?: PendingTimelock;
  pendingAgent?: PendingTimelock;
  /** Insurance fund balance in USDC, when the contract exposes it. */
  insuranceFundBalance?: number;
}

export interface PendingTimelock {
  hash: string;
  expiryLedger: number;
}

export interface Alert {
  id: string;
  type: AlertType;
  severity: "info" | "warning" | "critical";
  title: string;
  message: string;
  metrics?: Record<string, any>;
  timestamp: number;
  resolved?: boolean;
  resolutionTime?: number;
}

export type AlertType =
  | "tvl_drop"
  | "pause_duration_exceeded"
  | "withdrawal_spike"
  | "cap_saturation"
  | "share_price_decrease"
  | "failed_rebalance"
  | "upgrade_scheduled"
  | "agent_update_proposed"
  | "owner_transfer_initiated"
  | "anomalous_activity"
  | "rpc_connectivity"
  // Raised by the monitor itself when the insurance fund drops below its minimum.
  | "insurance_fund_level";

export interface MetricRecord {
  timestamp: number;
  ledger: number;
  value: number | bigint;
  unit: string;
}

export interface MonitoringState {
  lastMetrics: HealthMetrics | null;
  previousMetrics: HealthMetrics | null;
  hourlyMetrics: MetricRecord[];
  dailyMetrics: MetricRecord[];
  activeAlerts: Alert[];
  resolvedAlerts: Alert[];
  lastRpcCheck: number;
  isConnected: boolean;
}

export interface RebalanceEvent {
  ledger: number;
  agent: string;
  source: string;
  destination: string;
  timestamp: number;
  success: boolean;
}

export interface AuditLogEntry {
  timestamp: number;
  ledger: number;
  type: string;
  actor: string;
  action: string;
  details: Record<string, any>;
}
