import { MetricsCollector } from "../metrics-collector";
import { MonitoringConfig } from "../types";

// Metrics collection and log formatting (#701). The collector is the only place
// that turns raw contract reads into the HealthMetrics every alert depends on,
// so the tests pin the derived values (share price, timelock mapping, bigint
// passthrough) and the shape of the line it logs.

jest.mock("@neurowealth/vault-client", () => ({
  DECIMAL_PLACES: 7,
  VaultClient: jest.fn(),
}));

jest.mock("@stellar/stellar-sdk", () => ({
  SorobanRpc: {
    Server: jest.fn().mockImplementation(() => ({
      getLatestLedger: jest.fn().mockResolvedValue({ sequence: 4242 }),
    })),
  },
  Keypair: {
    random: () => ({ publicKey: () => "GDUMMYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA" }),
  },
}));

jest.mock("pino", () =>
  jest.fn(() => ({ info: jest.fn(), error: jest.fn(), warn: jest.fn() })),
);

import { VaultClient } from "@neurowealth/vault-client";
import pino from "pino";

const CONFIG: MonitoringConfig = {
  contractId: "CVAULT",
  rpcUrl: "https://rpc.testnet.stellar.org",
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

type ClientReads = Record<string, jest.Mock>;

function reads(overrides: Partial<Record<string, unknown>> = {}): ClientReads {
  const base: Record<string, unknown> = {
    get_total_assets: 2_000_0000000n,
    get_total_shares: 1_000_0000000n,
    get_total_deposits: 1_500_0000000n,
    is_paused: false,
    get_current_protocol: "blend",
    get_owner: "GOWNERAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    get_agent: "GAGENTAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    get_tvl_cap: 10_000_0000000n,
    get_user_deposit_cap: 100_0000000n,
    get_pending_upgrade: null,
    get_pending_agent_update: null,
    ...overrides,
  };
  const client: ClientReads = {};
  for (const [key, value] of Object.entries(base)) {
    client[key] =
      value instanceof Error
        ? jest.fn().mockRejectedValue(value)
        : jest.fn().mockResolvedValue(value);
  }
  return client;
}

function collectorWith(client: ClientReads) {
  (VaultClient as unknown as jest.Mock).mockImplementation(() => client);
  return new MetricsCollector(CONFIG);
}

const logger = () => (pino as unknown as jest.Mock).mock.results[0].value;

describe("MetricsCollector.collectMetrics", () => {
  it("maps every contract read onto the health metrics", async () => {
    const metrics = await collectorWith(reads()).collectMetrics();

    expect(metrics.ledgerSequence).toBe(4242);
    expect(metrics.tvl).toBe(2_000_0000000n);
    expect(metrics.totalShares).toBe(1_000_0000000n);
    expect(metrics.totalDeposits).toBe(1_500_0000000n);
    expect(metrics.isPaused).toBe(false);
    expect(metrics.currentProtocol).toBe("blend");
    expect(metrics.tvlCap).toBe(10_000_0000000n);
    expect(metrics.userDepositCap).toBe(100_0000000n);
    expect(typeof metrics.timestamp).toBe("number");
  });

  it("derives the share price from assets over shares", async () => {
    const metrics = await collectorWith(reads()).collectMetrics();
    expect(metrics.sharePrice).toBe(2);
  });

  it("reports a share price of zero rather than dividing by zero", async () => {
    const metrics = await collectorWith(
      reads({ get_total_shares: 0n, get_total_assets: 0n }),
    ).collectMetrics();
    expect(metrics.sharePrice).toBe(0);
  });

  it("keeps bigint precision instead of coercing to number", async () => {
    const huge = 123_456_789_012_345_678_901n;
    const metrics = await collectorWith(reads({ get_total_assets: huge })).collectMetrics();
    expect(metrics.tvl).toBe(huge);
    expect(typeof metrics.tvl).toBe("bigint");
  });

  it("maps a pending upgrade onto the timelock shape the alert engine reads", async () => {
    const metrics = await collectorWith(
      reads({ get_pending_upgrade: { wasm_hash: "deadbeef", expiry: 5000 } }),
    ).collectMetrics();
    expect(metrics.pendingUpgrade).toEqual({ hash: "deadbeef", expiryLedger: 5000 });
  });

  it("maps a pending agent update separately", async () => {
    const metrics = await collectorWith(
      reads({ get_pending_agent_update: { agent: "GNEWAGENT", expiry: 6000 } }),
    ).collectMetrics();
    expect(metrics.pendingAgent).toEqual({ hash: "GNEWAGENT", expiryLedger: 6000 });
  });

  it("treats a missing timelock as absent rather than failing the whole collection", async () => {
    const metrics = await collectorWith(
      reads({
        get_pending_upgrade: new Error("no pending upgrade"),
        get_pending_agent_update: new Error("no pending agent update"),
      }),
    ).collectMetrics();

    expect(metrics.pendingUpgrade).toBeUndefined();
    expect(metrics.pendingAgent).toBeUndefined();
    expect(metrics.tvl).toBe(2_000_0000000n);
  });

  it("remembers the last reading and serves it back", async () => {
    const collector = collectorWith(reads());
    expect(collector.getLastMetrics()).toBeNull();

    const metrics = await collector.collectMetrics();
    expect(collector.getLastMetrics()).toBe(metrics);
  });

  it("rethrows a failed collection and does not overwrite the last good reading", async () => {
    // The client is captured in the constructor, so the same instance has to
    // succeed once and then fail for the retained-reading assertion to mean
    // anything.
    const flaky = reads();
    flaky.get_total_assets
      .mockResolvedValueOnce(2_000_0000000n)
      .mockRejectedValueOnce(new Error("rpc unreachable"));
    const collector = collectorWith(flaky);

    const first = await collector.collectMetrics();
    await expect(collector.collectMetrics()).rejects.toThrow("rpc unreachable");

    expect(collector.getLastMetrics()).toBe(first);
    expect(logger().error).toHaveBeenCalled();
  });
});

describe("MetricsCollector log formatting", () => {
  it("logs a compact, human-scaled record of the reading", async () => {
    await collectorWith(reads()).collectMetrics();

    const [payload, message] = logger().info.mock.calls[0];
    expect(message).toBe("Metrics collected");
    expect(payload.metrics).toEqual({
      ledger: 4242,
      tvl_usdc: 2000,
      total_shares: "10000000000",
      share_price: "2.0000",
      is_paused: false,
      protocol: "blend",
      owner: "GOWNERAAAA...",
      agent: "GAGENTAAAA...",
    });
  });

  it("shortens owner and agent so a log line cannot leak a full key", async () => {
    await collectorWith(reads()).collectMetrics();
    const { owner, agent } = logger().info.mock.calls[0][0].metrics;
    expect(owner).toHaveLength(13);
    expect(agent).toHaveLength(13);
    expect(owner.endsWith("...")).toBe(true);
  });

  it("renders the share price at four decimal places", async () => {
    await collectorWith(
      reads({ get_total_assets: 1_000_0000001n, get_total_shares: 1_000_0000000n }),
    ).collectMetrics();
    expect(logger().info.mock.calls[0][0].metrics.share_price).toBe("1.0000");
  });
});
