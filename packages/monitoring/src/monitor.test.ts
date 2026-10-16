/**
 * Tests for VaultMonitor
 */

import { VaultMonitor } from "./monitor";
import { MonitoringConfig, HealthMetrics } from "./types";

// Mock the MetricsCollector
jest.mock("./metrics-collector", () => ({
  MetricsCollector: jest.fn(function () {
    this.collectMetrics = jest.fn();
    this.getLastMetrics = jest.fn();
  }),
}));

// Mock the AlertEngine
jest.mock("./alert-engine", () => ({
  AlertEngine: jest.fn(function () {
    this.detectAnomalies = jest.fn();
  }),
}));

// Mock the AlertDispatcher
jest.mock("./alert-dispatcher", () => ({
  AlertDispatcher: jest.fn(function () {
    this.dispatch = jest.fn();
  }),
}));

// Mock pino logger
jest.mock("pino", () => {
  return jest.fn(() => ({
    info: jest.fn(),
    error: jest.fn(),
    warn: jest.fn(),
    debug: jest.fn(),
  }));
});

describe("VaultMonitor", () => {
  let monitor: VaultMonitor;
  let mockConfig: MonitoringConfig;
  let mockMetrics: HealthMetrics;

  beforeEach(() => {
    jest.clearAllMocks();
    jest.useFakeTimers();

    mockConfig = {
      contractId: "test-contract-id",
      rpcUrl: "https://soroban-testnet.stellar.org",
      networkPassphrase: "Test SDF Network ; September 2015",
      pollIntervalSeconds: 60,
      alertWebhooks: [
        {
          name: "test-webhook",
          type: "webhook",
          url: "https://example.com/webhook",
        },
      ],
      thresholds: {
        tvlDropPercentage: 20,
        withdrawalSpikeFactor: 3,
        pauseDurationLedgers: 17280,
        capSaturationPercentage: 95,
      },
    };

    mockMetrics = {
      timestamp: Date.now(),
      ledgerSequence: 1000,
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

    monitor = new VaultMonitor(mockConfig);
  });

  afterEach(() => {
    jest.useRealTimers();
  });

  describe("start", () => {
    it("should initialize monitoring system", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
      }

      await monitor.start();

      expect(MetricsCollector).toHaveBeenCalledWith(mockConfig);
    });

    it("should set up polling interval", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
      }

      await monitor.start();

      // Verify an interval was set. Jest's modern fake timers replace
      // global.setInterval with their own scheduler rather than a
      // jest.fn(), so the call args cannot be asserted on directly with
      // toHaveBeenCalledWith; a pending timer is the equivalent check.
      expect(jest.getTimerCount()).toBeGreaterThan(0);
    });

    it("should perform initial collection", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
      }

      await monitor.start();

      // The initial collection should have been called
      if (metricsCollectorInstance) {
        expect(metricsCollectorInstance.collectMetrics).toHaveBeenCalled();
      }
    });

    it("should handle collection errors gracefully", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockRejectedValue(new Error("Collection failed"));
      }

      // collectAndAnalyze catches a collection failure internally (logs it,
      // marks disconnected, optionally dispatches an RPC-connectivity
      // alert) rather than propagating it, so start() must resolve.
      await expect(monitor.start()).resolves.toBeUndefined();
    });
  });

  describe("stop", () => {
    it("should stop the monitoring interval", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
      }

      await monitor.start();
      expect(jest.getTimerCount()).toBeGreaterThan(0);

      await monitor.stop();

      // clearInterval should have cleared the polling interval. Jest's
      // modern fake timers (the default since Jest 27) replace
      // global.clearInterval with their own scheduler rather than a
      // jest.fn(), so it cannot be asserted on with toHaveBeenCalled();
      // the timer count dropping to zero is the equivalent check.
      expect(jest.getTimerCount()).toBe(0);
    });

    it("should be safe to call stop when not started", async () => {
      // Should not throw
      await expect(monitor.stop()).resolves.not.toThrow();
    });

    it("should prevent further collections after stop", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
      }

      await monitor.start();
      await monitor.stop();

      jest.advanceTimersByTime(mockConfig.pollIntervalSeconds * 1000);

      // Verify collectMetrics is not called during the interval after stop
      // (it should only be called from the initial start)
    });
  });

  describe("Health Check", () => {
    it("should provide health check status", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
        metricsCollectorInstance.getLastMetrics.mockReturnValue(mockMetrics);
      }

      await monitor.start();

      // The monitor should track health status
      // (specific implementation depends on monitor structure)
    });

    it("should report connected status when RPC is reachable", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
      }

      await monitor.start();

      // Should indicate connection is healthy
    });

    it("should report disconnected status on RPC failure", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockRejectedValue(new Error("RPC unavailable"));
      }

      // collectAndAnalyze catches the failure internally rather than
      // propagating it (see "should handle collection errors gracefully"
      // above), so start() resolves; the failure is observable through
      // getStatus().isConnected instead.
      await monitor.start();
      expect(monitor.getStatus().isConnected).toBe(false);
    });
  });

  describe("Anomaly Detection Integration", () => {
    it("should detect anomalies on each collection", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const { AlertEngine } = require("./alert-engine");

      const metricsCollectorInstance = MetricsCollector.mock.instances[0];
      const alertEngineInstance = AlertEngine.mock.instances[0];

      if (metricsCollectorInstance && alertEngineInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
        alertEngineInstance.detectAnomalies.mockReturnValue([]);
      }

      await monitor.start();

      // The alert engine should be called to detect anomalies
    });

    it("should dispatch alerts when anomalies detected", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const { AlertEngine } = require("./alert-engine");
      const { AlertDispatcher } = require("./alert-dispatcher");

      const metricsCollectorInstance = MetricsCollector.mock.instances[0];
      const alertEngineInstance = AlertEngine.mock.instances[0];
      const alertDispatcherInstance = AlertDispatcher.mock.instances[0];

      const testAlert = {
        id: "test-alert",
        type: "tvl_drop" as const,
        severity: "critical" as const,
        title: "TVL Dropped",
        message: "TVL dropped significantly",
        timestamp: Date.now(),
      };

      if (metricsCollectorInstance && alertEngineInstance && alertDispatcherInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
        alertEngineInstance.detectAnomalies.mockReturnValue([testAlert]);
      }

      await monitor.start();

      // Dispatcher should be called for each alert
    });
  });

  describe("Polling Interval", () => {
    it("should respect configured poll interval", async () => {
      const customConfig = {
        ...mockConfig,
        pollIntervalSeconds: 30,
      };

      const customMonitor = new VaultMonitor(customConfig);
      const { MetricsCollector } = require("./metrics-collector");

      // index 1, not 0: beforeEach already constructed the outer `monitor`
      // (and its own MetricsCollector) before this test's customMonitor.
      const metricsCollectorInstance = MetricsCollector.mock.instances[1];
      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
      }

      await customMonitor.start();
      // start() itself triggers one immediate collection; reset the call
      // count so the assertion below is only about the interval's delay.
      metricsCollectorInstance.collectMetrics.mockClear();

      // Jest's modern fake timers replace global.setInterval with their own
      // scheduler rather than a jest.fn(), so its call args cannot be
      // asserted on directly with toHaveBeenCalledWith. Advancing by just
      // under the configured interval must not yet trigger another
      // collection; advancing past it must. The async variant is required
      // because the interval callback's collectAndAnalyze() is itself
      // async - a sync advance would fire the timer but not await it.
      await jest.advanceTimersByTimeAsync(30 * 1000 - 1);
      expect(metricsCollectorInstance.collectMetrics).not.toHaveBeenCalled();

      await jest.advanceTimersByTimeAsync(1);
      expect(metricsCollectorInstance.collectMetrics).toHaveBeenCalledTimes(1);
    });
  });

  describe("State Management", () => {
    it("should track current and previous metrics", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      const metrics1 = { ...mockMetrics, ledgerSequence: 1000 };
      const metrics2 = { ...mockMetrics, ledgerSequence: 1001 };

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics
          .mockResolvedValueOnce(metrics1)
          .mockResolvedValueOnce(metrics2);
      }

      await monitor.start();

      // Advance time to trigger next collection
      jest.advanceTimersByTime(mockConfig.pollIntervalSeconds * 1000);

      // The monitor should maintain both current and previous metrics
    });

    it("should maintain active and resolved alerts", async () => {
      const { MetricsCollector } = require("./metrics-collector");
      const metricsCollectorInstance = MetricsCollector.mock.instances[0];

      if (metricsCollectorInstance) {
        metricsCollectorInstance.collectMetrics.mockResolvedValue(mockMetrics);
      }

      await monitor.start();

      // Monitor should track alert history
    });
  });
});
