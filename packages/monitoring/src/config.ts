/**
 * Monitoring Alert Thresholds and Runtime Configuration (#758)
 *
 * Provides environment-configurable thresholds for TVL drop, withdrawal spikes,
 * pause duration, and TVL cap saturation, with fallback to safe defaults.
 */

import { AlertThresholds, AlertWebhook, MonitoringConfig } from "./types";

export const DEFAULT_ALERT_THRESHOLDS: AlertThresholds = {
  tvlDropPercentage: 20, // 20% drop
  withdrawalSpikeFactor: 3, // 3x average withdrawal rate
  pauseDurationLedgers: 17280, // ~24 hours in ledgers (at ~5s per ledger)
  capSaturationPercentage: 95, // 95% cap saturation
};

/**
 * Parses a positive finite number from an env string, returning fallback if absent or invalid.
 */
function parsePositiveNumber(value: string | undefined, fallback: number): number {
  if (value === undefined || value === "") return fallback;
  const num = Number(value);
  if (!Number.isFinite(num) || num <= 0) return fallback;
  return num;
}

/**
 * Parses a positive integer from an env string, returning fallback if absent or invalid.
 */
function parsePositiveInteger(value: string | undefined, fallback: number): number {
  if (value === undefined || value === "") return fallback;
  const num = Number(value);
  if (!Number.isFinite(num) || num <= 0 || !Number.isInteger(num)) return fallback;
  return num;
}

/**
 * Load alert thresholds from environment variables.
 *
 * Env vars:
 * - TVL_DROP_PERCENTAGE: Percentage drop in TVL to trigger alert (default: 20)
 * - WITHDRAWAL_SPIKE_FACTOR: Multiplier over average withdrawals (default: 3)
 * - PAUSE_DURATION_LEDGERS: Max ledger duration before alert on paused vault (default: 17280)
 * - CAP_SATURATION_PERCENTAGE: Deposit cap utilization percentage to alert (default: 95)
 */
export function loadAlertThresholds(
  env: Record<string, string | undefined> = process.env,
): AlertThresholds {
  return {
    tvlDropPercentage: parsePositiveNumber(
      env.TVL_DROP_PERCENTAGE,
      DEFAULT_ALERT_THRESHOLDS.tvlDropPercentage,
    ),
    withdrawalSpikeFactor: parsePositiveNumber(
      env.WITHDRAWAL_SPIKE_FACTOR,
      DEFAULT_ALERT_THRESHOLDS.withdrawalSpikeFactor,
    ),
    pauseDurationLedgers: parsePositiveInteger(
      env.PAUSE_DURATION_LEDGERS,
      DEFAULT_ALERT_THRESHOLDS.pauseDurationLedgers,
    ),
    capSaturationPercentage: parsePositiveNumber(
      env.CAP_SATURATION_PERCENTAGE,
      DEFAULT_ALERT_THRESHOLDS.capSaturationPercentage,
    ),
  };
}

/**
 * Load complete monitoring configuration from environment variables.
 */
export function loadMonitoringConfig(
  env: Record<string, string | undefined> = process.env,
): MonitoringConfig {
  const alertWebhooks: AlertWebhook[] = [];

  if (env.SLACK_WEBHOOK_URL) {
    alertWebhooks.push({
      name: "slack",
      type: "slack",
      url: env.SLACK_WEBHOOK_URL,
      severity: "warning",
    });
  }
  if (env.DISCORD_WEBHOOK_URL) {
    alertWebhooks.push({
      name: "discord",
      type: "discord",
      url: env.DISCORD_WEBHOOK_URL,
      severity: "warning",
    });
  }
  if (env.TELEGRAM_WEBHOOK_URL) {
    alertWebhooks.push({
      name: "telegram",
      type: "telegram",
      url: env.TELEGRAM_WEBHOOK_URL,
      severity: "warning",
    });
  }

  return {
    contractId: env.VAULT_CONTRACT_ID || "",
    rpcUrl: env.SOROBAN_RPC_URL || "https://soroban.stellar.org",
    networkPassphrase:
      env.SOROBAN_NETWORK_PASSPHRASE ||
      "Public Global Stellar Network ; Stellar Development Foundation",
    pollIntervalSeconds: parsePositiveInteger(env.POLL_INTERVAL_SECONDS, 30),
    alertWebhooks,
    thresholds: loadAlertThresholds(env),
    metricsBackendUrl: env.METRICS_BACKEND_URL || undefined,
    enablePauseDrill: env.ENABLE_PAUSE_DRILL === "true",
  };
}

/**
 * Log the effective monitoring configuration at startup.
 */
export function logMonitoringConfig(
  config: MonitoringConfig,
  logger: { info: (obj: Record<string, unknown>, msg: string) => void },
): void {
  logger.info(
    {
      contractId: config.contractId,
      rpcUrl: config.rpcUrl,
      pollIntervalSeconds: config.pollIntervalSeconds,
      thresholds: config.thresholds,
      webhooksConfigured: config.alertWebhooks.length,
      metricsBackendUrl: config.metricsBackendUrl ? "configured" : "none",
    },
    "Monitoring configuration loaded",
  );
}
