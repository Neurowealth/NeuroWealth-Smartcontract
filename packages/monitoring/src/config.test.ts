/**
 * Tests for monitoring configuration and alert threshold loading (#758)
 */

import {
  DEFAULT_ALERT_THRESHOLDS,
  loadAlertThresholds,
  loadMonitoringConfig,
} from "./config";

describe("loadAlertThresholds", () => {
  it("uses defaults when no environment variables are set", () => {
    const thresholds = loadAlertThresholds({});
    expect(thresholds).toEqual(DEFAULT_ALERT_THRESHOLDS);
  });

  it("loads configured alert thresholds from environment variables", () => {
    const env = {
      TVL_DROP_PERCENTAGE: "15",
      WITHDRAWAL_SPIKE_FACTOR: "5",
      PAUSE_DURATION_LEDGERS: "8640",
      CAP_SATURATION_PERCENTAGE: "90",
    };

    const thresholds = loadAlertThresholds(env);
    expect(thresholds).toEqual({
      tvlDropPercentage: 15,
      withdrawalSpikeFactor: 5,
      pauseDurationLedgers: 8640,
      capSaturationPercentage: 90,
    });
  });

  it("falls back to defaults when environment variables are invalid or non-positive", () => {
    const env = {
      TVL_DROP_PERCENTAGE: "-5",
      WITHDRAWAL_SPIKE_FACTOR: "not_a_number",
      PAUSE_DURATION_LEDGERS: "0",
      CAP_SATURATION_PERCENTAGE: "",
    };

    const thresholds = loadAlertThresholds(env);
    expect(thresholds).toEqual(DEFAULT_ALERT_THRESHOLDS);
  });
});

describe("loadMonitoringConfig", () => {
  it("loads full monitoring config with default values", () => {
    const config = loadMonitoringConfig({});
    expect(config.contractId).toBe("");
    expect(config.rpcUrl).toBe("https://soroban.stellar.org");
    expect(config.pollIntervalSeconds).toBe(30);
    expect(config.thresholds).toEqual(DEFAULT_ALERT_THRESHOLDS);
    expect(config.alertWebhooks).toEqual([]);
    expect(config.enablePauseDrill).toBe(false);
  });

  it("loads full monitoring config from environment variables", () => {
    const env = {
      VAULT_CONTRACT_ID: "CCONTRACT123",
      SOROBAN_RPC_URL: "https://soroban-testnet.stellar.org",
      SOROBAN_NETWORK_PASSPHRASE: "Test SDF Network ; September 2015",
      POLL_INTERVAL_SECONDS: "60",
      TVL_DROP_PERCENTAGE: "25",
      WITHDRAWAL_SPIKE_FACTOR: "4",
      PAUSE_DURATION_LEDGERS: "10000",
      CAP_SATURATION_PERCENTAGE: "80",
      SLACK_WEBHOOK_URL: "https://hooks.slack.com/services/abc",
      DISCORD_WEBHOOK_URL: "https://discord.com/api/webhooks/xyz",
      TELEGRAM_WEBHOOK_URL: "https://api.telegram.org/bot123/sendMessage",
      METRICS_BACKEND_URL: "http://localhost:9090",
      ENABLE_PAUSE_DRILL: "true",
    };

    const config = loadMonitoringConfig(env);
    expect(config.contractId).toBe("CCONTRACT123");
    expect(config.rpcUrl).toBe("https://soroban-testnet.stellar.org");
    expect(config.networkPassphrase).toBe("Test SDF Network ; September 2015");
    expect(config.pollIntervalSeconds).toBe(60);
    expect(config.thresholds).toEqual({
      tvlDropPercentage: 25,
      withdrawalSpikeFactor: 4,
      pauseDurationLedgers: 10000,
      capSaturationPercentage: 80,
    });
    expect(config.alertWebhooks).toHaveLength(3);
    expect(config.metricsBackendUrl).toBe("http://localhost:9090");
    expect(config.enablePauseDrill).toBe(true);
  });
});
