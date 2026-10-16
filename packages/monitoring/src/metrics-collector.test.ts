/**
 * Tests for MetricsCollector
 */

import { MetricsCollector } from "./metrics-collector";
import { MonitoringConfig, HealthMetrics } from "./types";
import * as StellarSdk from "@stellar/stellar-sdk";

// Mock the VaultClient
jest.mock("@neurowealth/vault-client", () => ({
  VaultClient: jest.fn(function (config) {
    this.get_total_assets = jest.fn();
    this.get_total_shares = jest.fn();
    this.get_total_deposits = jest.fn();
    this.is_paused = jest.fn();
    this.get_current_protocol = jest.fn();
    this.get_owner = jest.fn();
    this.get_agent = jest.fn();
    this.get_tvl_cap = jest.fn();
    this.get_user_deposit_cap = jest.fn();
    this.get_pending_upgrade = jest.fn();
    this.get_pending_agent_update = jest.fn();
  }),
  DECIMAL_PLACES: 7,
}));

// Mock Stellar SDK
jest.mock("@stellar/stellar-sdk", () => ({
  ...jest.requireActual("@stellar/stellar-sdk"),
  Keypair: {
    random: jest.fn(() => ({
      publicKey: jest.fn(() => "GTEST_PUBLIC_KEY"),
    })),
  },
  SorobanRpc: {
    Server: jest.fn(function () {
      this.getLatestLedger = jest.fn();
    }),
  },
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

describe("MetricsCollector", () => {
  let collector: MetricsCollector;
  let mockConfig: MonitoringConfig;
  let mockVaultClient: any;

  beforeEach(() => {
    jest.clearAllMocks();

    mockConfig = {
      contractId: "test-contract-id",
      rpcUrl: "https://soroban-testnet.stellar.org",
      networkPassphrase: "Test SDF Network ; September 2015",
      pollIntervalSeconds: 60,
      alertWebhooks: [],
      thresholds: {
        tvlDropPercentage: 20,
        withdrawalSpikeFactor: 3,
        pauseDurationLedgers: 17280,
        capSaturationPercentage: 95,
      },
    };

    collector = new MetricsCollector(mockConfig);
    // A jest.fn() constructor mock records the `this`-populated instance in
    // mock.instances, not mock.results[0].value: a constructor with no
    // explicit `return` implicitly returns undefined, which is what
    // mock.results records. Reading .value here always yielded undefined,
    // so every test below silently never exercised the mocked VaultClient.
    mockVaultClient = (require("@neurowealth/vault-client").VaultClient as jest.Mock)
      .mock.instances[0];
  });

  describe("collectMetrics", () => {
    it("should collect all metrics successfully", async () => {
      // Setup mock server
      const mockServer = new (StellarSdk.SorobanRpc.Server as jest.Mock)();
      mockServer.getLatestLedger.mockResolvedValue({ sequence: 1000 });
      (StellarSdk.SorobanRpc.Server as jest.Mock).mockImplementation(() => mockServer);

      // Setup vault client mocks
      mockVaultClient.get_total_assets.mockResolvedValue(100000000000n);
      mockVaultClient.get_total_shares.mockResolvedValue(50000000000n);
      mockVaultClient.get_total_deposits.mockResolvedValue(80000000000n);
      mockVaultClient.is_paused.mockResolvedValue(false);
      mockVaultClient.get_current_protocol.mockResolvedValue("blend");
      mockVaultClient.get_owner.mockResolvedValue("GOWNER_ADDRESS");
      mockVaultClient.get_agent.mockResolvedValue("GAGENT_ADDRESS");
      mockVaultClient.get_tvl_cap.mockResolvedValue(200000000000n);
      mockVaultClient.get_user_deposit_cap.mockResolvedValue(5000000000n);
      mockVaultClient.get_pending_upgrade.mockRejectedValue(new Error("Not found"));
      mockVaultClient.get_pending_agent_update.mockRejectedValue(new Error("Not found"));

      const metrics = await collector.collectMetrics();

      expect(metrics).toBeDefined();
      expect(metrics.ledgerSequence).toBe(1000);
      expect(metrics.tvl).toBe(100000000000n);
      expect(metrics.totalShares).toBe(50000000000n);
      expect(metrics.totalDeposits).toBe(80000000000n);
      expect(metrics.isPaused).toBe(false);
      expect(metrics.currentProtocol).toBe("blend");
      expect(metrics.owner).toBe("GOWNER_ADDRESS");
      expect(metrics.agent).toBe("GAGENT_ADDRESS");
      expect(metrics.sharePrice).toBe(2);
      expect(metrics.tvlCap).toBe(200000000000n);
      expect(metrics.userDepositCap).toBe(5000000000n);
      expect(metrics.pendingUpgrade).toBeUndefined();
      expect(metrics.pendingAgent).toBeUndefined();
    });

    it("should calculate share price correctly", async () => {
      const mockServer = new (StellarSdk.SorobanRpc.Server as jest.Mock)();
      mockServer.getLatestLedger.mockResolvedValue({ sequence: 1000 });
      (StellarSdk.SorobanRpc.Server as jest.Mock).mockImplementation(() => mockServer);

      mockVaultClient.get_total_assets.mockResolvedValue(150000000000n);
      mockVaultClient.get_total_shares.mockResolvedValue(100000000000n);
      mockVaultClient.get_total_deposits.mockResolvedValue(100000000000n);
      mockVaultClient.is_paused.mockResolvedValue(false);
      mockVaultClient.get_current_protocol.mockResolvedValue("blend");
      mockVaultClient.get_owner.mockResolvedValue("GOWNER");
      mockVaultClient.get_agent.mockResolvedValue("GAGENT");
      mockVaultClient.get_tvl_cap.mockResolvedValue(300000000000n);
      mockVaultClient.get_user_deposit_cap.mockResolvedValue(10000000000n);
      mockVaultClient.get_pending_upgrade.mockRejectedValue(new Error("Not found"));
      mockVaultClient.get_pending_agent_update.mockRejectedValue(new Error("Not found"));

      const metrics = await collector.collectMetrics();

      // 150000000000 / 100000000000 = 1.5
      expect(metrics.sharePrice).toBe(1.5);
    });

    it("should handle zero shares for share price calculation", async () => {
      const mockServer = new (StellarSdk.SorobanRpc.Server as jest.Mock)();
      mockServer.getLatestLedger.mockResolvedValue({ sequence: 1000 });
      (StellarSdk.SorobanRpc.Server as jest.Mock).mockImplementation(() => mockServer);

      mockVaultClient.get_total_assets.mockResolvedValue(100000000000n);
      mockVaultClient.get_total_shares.mockResolvedValue(0n);
      mockVaultClient.get_total_deposits.mockResolvedValue(100000000000n);
      mockVaultClient.is_paused.mockResolvedValue(false);
      mockVaultClient.get_current_protocol.mockResolvedValue("blend");
      mockVaultClient.get_owner.mockResolvedValue("GOWNER");
      mockVaultClient.get_agent.mockResolvedValue("GAGENT");
      mockVaultClient.get_tvl_cap.mockResolvedValue(200000000000n);
      mockVaultClient.get_user_deposit_cap.mockResolvedValue(5000000000n);
      mockVaultClient.get_pending_upgrade.mockRejectedValue(new Error("Not found"));
      mockVaultClient.get_pending_agent_update.mockRejectedValue(new Error("Not found"));

      const metrics = await collector.collectMetrics();

      expect(metrics.sharePrice).toBe(0);
    });

    it("should include pending upgrade when present", async () => {
      const mockServer = new (StellarSdk.SorobanRpc.Server as jest.Mock)();
      mockServer.getLatestLedger.mockResolvedValue({ sequence: 1000 });
      (StellarSdk.SorobanRpc.Server as jest.Mock).mockImplementation(() => mockServer);

      const pendingUpgrade = {
        wasm_hash: "abc123def456",
        expiry: 2000,
      };

      mockVaultClient.get_total_assets.mockResolvedValue(100000000000n);
      mockVaultClient.get_total_shares.mockResolvedValue(50000000000n);
      mockVaultClient.get_total_deposits.mockResolvedValue(80000000000n);
      mockVaultClient.is_paused.mockResolvedValue(false);
      mockVaultClient.get_current_protocol.mockResolvedValue("blend");
      mockVaultClient.get_owner.mockResolvedValue("GOWNER");
      mockVaultClient.get_agent.mockResolvedValue("GAGENT");
      mockVaultClient.get_tvl_cap.mockResolvedValue(200000000000n);
      mockVaultClient.get_user_deposit_cap.mockResolvedValue(5000000000n);
      mockVaultClient.get_pending_upgrade.mockResolvedValue(pendingUpgrade);
      mockVaultClient.get_pending_agent_update.mockRejectedValue(new Error("Not found"));

      const metrics = await collector.collectMetrics();

      expect(metrics.pendingUpgrade).toBeDefined();
      expect(metrics.pendingUpgrade?.hash).toBe("abc123def456");
      expect(metrics.pendingUpgrade?.expiryLedger).toBe(2000);
    });

    it("should handle RPC errors gracefully", async () => {
      const mockServer = new (StellarSdk.SorobanRpc.Server as jest.Mock)();
      mockServer.getLatestLedger.mockRejectedValue(new Error("RPC connection failed"));
      (StellarSdk.SorobanRpc.Server as jest.Mock).mockImplementation(() => mockServer);

      await expect(collector.collectMetrics()).rejects.toThrow("RPC connection failed");
    });

    it("should store metrics in lastMetrics", async () => {
      const mockServer = new (StellarSdk.SorobanRpc.Server as jest.Mock)();
      mockServer.getLatestLedger.mockResolvedValue({ sequence: 1000 });
      (StellarSdk.SorobanRpc.Server as jest.Mock).mockImplementation(() => mockServer);

      mockVaultClient.get_total_assets.mockResolvedValue(100000000000n);
      mockVaultClient.get_total_shares.mockResolvedValue(50000000000n);
      mockVaultClient.get_total_deposits.mockResolvedValue(80000000000n);
      mockVaultClient.is_paused.mockResolvedValue(false);
      mockVaultClient.get_current_protocol.mockResolvedValue("blend");
      mockVaultClient.get_owner.mockResolvedValue("GOWNER");
      mockVaultClient.get_agent.mockResolvedValue("GAGENT");
      mockVaultClient.get_tvl_cap.mockResolvedValue(200000000000n);
      mockVaultClient.get_user_deposit_cap.mockResolvedValue(5000000000n);
      mockVaultClient.get_pending_upgrade.mockRejectedValue(new Error("Not found"));
      mockVaultClient.get_pending_agent_update.mockRejectedValue(new Error("Not found"));

      await collector.collectMetrics();

      const lastMetrics = collector.getLastMetrics();
      expect(lastMetrics).toBeDefined();
      expect(lastMetrics?.ledgerSequence).toBe(1000);
    });
  });

  describe("getLastMetrics", () => {
    it("should return null when no metrics collected", () => {
      const metrics = collector.getLastMetrics();
      expect(metrics).toBeNull();
    });
  });
});
