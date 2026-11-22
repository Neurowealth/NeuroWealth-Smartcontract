/**
 * Tests for AlertEngine
 */

import { AlertEngine } from "./alert-engine";
import { AlertThresholds, HealthMetrics, MonitoringState } from "./types";

// Mock pino logger
jest.mock("pino", () => {
  return jest.fn(() => ({
    info: jest.fn(),
    error: jest.fn(),
    warn: jest.fn(),
    debug: jest.fn(),
  }));
});

describe("AlertEngine", () => {
  let engine: AlertEngine;
  let mockThresholds: AlertThresholds;
  let currentMetrics: HealthMetrics;
  let previousMetrics: HealthMetrics;
  let mockState: MonitoringState;

  beforeEach(() => {
    jest.clearAllMocks();

    mockThresholds = {
      tvlDropPercentage: 20,
      withdrawalSpikeFactor: 3,
      pauseDurationLedgers: 17280,
      capSaturationPercentage: 95,
    };

    engine = new AlertEngine(mockThresholds);

    // Base metrics
    currentMetrics = {
      timestamp: Date.now(),
      ledgerSequence: 2000,
      tvl: 100000000000n,
      totalShares: 50000000000n,
      totalDeposits: 80000000000n,
      isPaused: false,
      currentProtocol: "blend",
      owner: "GOWNER_ADDRESS",
      agent: "GAGENT_ADDRESS",
      sharePrice: 2.0,
      tvlCap: 200000000000n,
      userDepositCap: 5000000000n,
    };

    previousMetrics = {
      ...currentMetrics,
      ledgerSequence: 1000,
      timestamp: Date.now() - 60000,
    };

    mockState = {
      lastMetrics: currentMetrics,
      previousMetrics: previousMetrics,
      hourlyMetrics: [],
      dailyMetrics: [],
      activeAlerts: [],
      resolvedAlerts: [],
      lastRpcCheck: Date.now(),
      isConnected: true,
    };
  });

  describe("TVL Drop Detection", () => {
    it("should detect significant TVL drop", () => {
      currentMetrics.tvl = 75000000000n; // 25% drop
      previousMetrics.tvl = 100000000000n;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      expect(alerts.length).toBeGreaterThan(0);
      const tvlAlert = alerts.find((a) => a.type === "tvl_drop");
      expect(tvlAlert).toBeDefined();
      expect(tvlAlert?.severity).toBe("critical");
      expect(tvlAlert?.metrics).toHaveProperty("drop_percentage");
    });

    it("should not alert for small TVL drop below threshold", () => {
      currentMetrics.tvl = 85000000000n; // 15% drop, below 20% threshold
      previousMetrics.tvl = 100000000000n;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const tvlAlert = alerts.find((a) => a.type === "tvl_drop");
      expect(tvlAlert).toBeUndefined();
    });

    it("should not alert for TVL increase", () => {
      currentMetrics.tvl = 120000000000n;
      previousMetrics.tvl = 100000000000n;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const tvlAlert = alerts.find((a) => a.type === "tvl_drop");
      expect(tvlAlert).toBeUndefined();
    });
  });

  describe("Share Price Decrease Detection", () => {
    it("should detect share price decrease", () => {
      currentMetrics.sharePrice = 1.97; // Less than 99% of previous (1.98 is exactly 99%, not less)
      previousMetrics.sharePrice = 2.0;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const priceAlert = alerts.find((a) => a.type === "share_price_decrease");
      expect(priceAlert).toBeDefined();
      expect(priceAlert?.severity).toBe("critical");
    });

    it("should not alert for small share price changes", () => {
      currentMetrics.sharePrice = 1.99; // 99.5% of previous, within tolerance
      previousMetrics.sharePrice = 2.0;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const priceAlert = alerts.find((a) => a.type === "share_price_decrease");
      expect(priceAlert).toBeUndefined();
    });

    it("should not alert for share price increase", () => {
      currentMetrics.sharePrice = 2.1;
      previousMetrics.sharePrice = 2.0;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const priceAlert = alerts.find((a) => a.type === "share_price_decrease");
      expect(priceAlert).toBeUndefined();
    });
  });

  describe("Withdrawal Spike Detection", () => {
    it("should detect significant withdrawal spike", () => {
      currentMetrics.totalDeposits = 70000000000n; // 10+ USDC withdrawn (more than 10%)
      previousMetrics.totalDeposits = 80000000000n;
      mockState.hourlyMetrics = [{ timestamp: Date.now(), ledger: 1000, value: 80000000000n, unit: "usdc" }];

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const withdrawalAlert = alerts.find((a) => a.type === "withdrawal_spike");
      expect(withdrawalAlert).toBeDefined();
      expect(withdrawalAlert?.severity).toBe("warning");
    });

    it("should not alert for small withdrawals", () => {
      currentMetrics.totalDeposits = 79000000000n; // 1 USDC withdrawn, ~1.25%
      previousMetrics.totalDeposits = 80000000000n;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const withdrawalAlert = alerts.find((a) => a.type === "withdrawal_spike");
      expect(withdrawalAlert).toBeUndefined();
    });
  });

  describe("Cap Saturation Detection", () => {
    it("should detect cap saturation above threshold", () => {
      currentMetrics.tvl = 195000000000n; // 97.5% of 200B cap
      currentMetrics.tvlCap = 200000000000n;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const capAlert = alerts.find((a) => a.type === "cap_saturation");
      expect(capAlert).toBeDefined();
      expect(capAlert?.severity).toBe("warning");
      expect(capAlert?.metrics).toHaveProperty("saturation_percent");
    });

    it("should not alert when below saturation threshold", () => {
      currentMetrics.tvl = 185000000000n; // 92.5% of cap, below 95%
      currentMetrics.tvlCap = 200000000000n;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const capAlert = alerts.find((a) => a.type === "cap_saturation");
      expect(capAlert).toBeUndefined();
    });
  });

  describe("Pause Detection", () => {
    it("should detect when vault is paused", () => {
      currentMetrics.isPaused = true;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const pauseAlert = alerts.find((a) => a.type === "pause_duration_exceeded");
      expect(pauseAlert).toBeDefined();
      expect(pauseAlert?.severity).toBe("warning");
    });

    it("should not alert when vault is not paused", () => {
      currentMetrics.isPaused = false;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const pauseAlert = alerts.find((a) => a.type === "pause_duration_exceeded");
      expect(pauseAlert).toBeUndefined();
    });
  });

  describe("Pending Upgrade Monitoring", () => {
    it("should detect pending upgrade", () => {
      currentMetrics.pendingUpgrade = {
        hash: "abc123def456",
        expiryLedger: 3000,
      };

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const upgradeAlert = alerts.find((a) => a.type === "upgrade_scheduled");
      expect(upgradeAlert).toBeDefined();
      expect(upgradeAlert?.severity).toBe("warning");
      expect(upgradeAlert?.metrics).toHaveProperty("ledgers_remaining");
    });

    it("should calculate remaining ledgers correctly", () => {
      currentMetrics.ledgerSequence = 1000;
      currentMetrics.pendingUpgrade = {
        hash: "abc123def456",
        expiryLedger: 2000,
      };

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const upgradeAlert = alerts.find((a) => a.type === "upgrade_scheduled");
      expect(upgradeAlert?.metrics?.ledgers_remaining).toBe(1000);
    });
  });

  describe("Pending Agent Update Monitoring", () => {
    it("should detect pending agent update", () => {
      currentMetrics.pendingAgent = {
        hash: "GNEWAGENT_ADDRESS",
        expiryLedger: 3000,
      };

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      const agentAlert = alerts.find((a) => a.type === "agent_update_proposed");
      expect(agentAlert).toBeDefined();
      expect(agentAlert?.severity).toBe("warning");
    });
  });

  describe("No Previous Metrics", () => {
    it("should handle null previous metrics", () => {
      const alerts = engine.detectAnomalies(currentMetrics, null, mockState);

      // Should not alert on TVL drop, share price, or withdrawal spike without previous data
      expect(alerts.find((a) => a.type === "tvl_drop")).toBeUndefined();
      expect(alerts.find((a) => a.type === "share_price_decrease")).toBeUndefined();
      expect(alerts.find((a) => a.type === "withdrawal_spike")).toBeUndefined();
    });

    it("should still check cap saturation and pause without previous metrics", () => {
      currentMetrics.isPaused = true;

      const alerts = engine.detectAnomalies(currentMetrics, null, mockState);

      // Should still alert on pause and cap saturation
      expect(alerts.find((a) => a.type === "pause_duration_exceeded")).toBeDefined();
    });
  });

  describe("Multiple Alerts", () => {
    it("should detect multiple anomalies simultaneously", () => {
      // Trigger multiple alerts
      currentMetrics.tvl = 75000000000n; // 25% TVL drop
      currentMetrics.sharePrice = 1.97; // Share price drop (below 99% of previous)
      currentMetrics.isPaused = true; // Vault paused
      previousMetrics.tvl = 100000000000n;
      previousMetrics.sharePrice = 2.0;

      const alerts = engine.detectAnomalies(currentMetrics, previousMetrics, mockState);

      expect(alerts.length).toBeGreaterThanOrEqual(3);
      expect(alerts.find((a) => a.type === "tvl_drop")).toBeDefined();
      expect(alerts.find((a) => a.type === "share_price_decrease")).toBeDefined();
      expect(alerts.find((a) => a.type === "pause_duration_exceeded")).toBeDefined();
    });
  });
});
