/**
 * Bridge Monitor Configuration (#810)
 *
 * Environment-configurable timeout, poll, and backoff constants.
 * Falls back to sane defaults when env vars are not set or invalid.
 */

export interface BridgeMonitorConfig {
  /** Timeout in ms for Axelar confirmation checks */
  timeoutMs: number;
  /** Poll interval in ms for checking pending transfers */
  pollIntervalMs: number;
  /** Backoff delay in ms between retry attempts */
  retryBackoffMs: number;
}

const DEFAULTS: BridgeMonitorConfig = {
  timeoutMs: 30 * 60 * 1000, // 30 minutes
  pollIntervalMs: 60 * 1000, // 1 minute
  retryBackoffMs: 5 * 60 * 1000, // 5 minutes
};

/**
 * Parse a positive integer from env, falling back to default if invalid.
 */
function parsePositiveInt(value: string | undefined, fallback: number): number {
  if (value === undefined || value === "") return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0 || !Number.isInteger(parsed)) {
    return fallback;
  }
  return parsed;
}

/**
 * Load bridge monitor config from environment variables.
 *
 * Env vars:
 * - BRIDGE_TIMEOUT_MS: Timeout for Axelar confirmation checks (default: 1800000)
 * - BRIDGE_POLL_INTERVAL_MS: Poll interval (default: 60000)
 * - BRIDGE_RETRY_BACKOFF_MS: Retry backoff delay (default: 300000)
 */
export function loadBridgeMonitorConfig(
  env: Record<string, string | undefined> = process.env,
): BridgeMonitorConfig {
  return {
    timeoutMs: parsePositiveInt(env.BRIDGE_TIMEOUT_MS, DEFAULTS.timeoutMs),
    pollIntervalMs: parsePositiveInt(env.BRIDGE_POLL_INTERVAL_MS, DEFAULTS.pollIntervalMs),
    retryBackoffMs: parsePositiveInt(env.BRIDGE_RETRY_BACKOFF_MS, DEFAULTS.retryBackoffMs),
  };
}

/**
 * Log the effective bridge monitor configuration at startup.
 */
export function logBridgeMonitorConfig(config: BridgeMonitorConfig, logger: { info: (obj: Record<string, unknown>, msg: string) => void }): void {
  logger.info(
    {
      timeoutMs: config.timeoutMs,
      pollIntervalMs: config.pollIntervalMs,
      retryBackoffMs: config.retryBackoffMs,
    },
    "Bridge monitor configuration loaded",
  );
}
