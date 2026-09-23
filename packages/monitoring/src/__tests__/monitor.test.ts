import { VaultMonitor } from "../monitor";
import { HealthMetrics, MonitoringConfig } from "../types";

// Monitor health surface (#701). The monitor is what an operator actually
// looks at, so these tests pin the status it exposes, the two alerts it raises
// outside the engine (RPC connectivity and the insurance fund), and the
// cooldown that stops the insurance alert from repeating every cycle.

jest.mock("axios");
jest.mock("../metrics-collector", () => ({ MetricsCollector: jest.fn() }));

import axios from "axios";
import { MetricsCollector } from "../metrics-collector";

const mockedPost = axios.post as jest.MockedFunction<typeof axios.post>;
const MockCollector = MetricsCollector as unknown as jest.Mock;

function metrics(overrides: Partial<HealthMetrics> = {}): HealthMetrics {
  return {
    timestamp: Date.now(),
    ledgerSequence: 1000,
    tvl: 1_000_0000000n,
    totalShares: 1_000_0000000n,
    totalDeposits: 1_000_0000000n,
    isPaused: false,
    currentProtocol: "blend",
    owner: "GOWNERAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    agent: "GAGENTAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    sharePrice: 1,
    tvlCap: 10_000_0000000n,
    userDepositCap: 100_0000000n,
    ...overrides,
  };
}

function config(overrides: Partial<MonitoringConfig> = {}): MonitoringConfig {
  return {
    contractId: "CVAULT",
    rpcUrl: "https://rpc.testnet.stellar.org",
    networkPassphrase: "Test SDF Network ; September 2015",
    pollIntervalSeconds: 60,
    alertWebhooks: [
      { name: "ops", type: "webhook", url: "https://hooks.example.test/all" },
    ],
    thresholds: {
      tvlDropPercentage: 20,
      withdrawalSpikeFactor: 3,
      pauseDurationLedgers: 17280,
      capSaturationPercentage: 95,
    },
    ...overrides,
  };
}

function collectorReturning(...readings: Array<HealthMetrics | Error>) {
  const collectMetrics = jest.fn();
  for (const reading of readings) {
    if (reading instanceof Error) collectMetrics.mockRejectedValueOnce(reading);
    else collectMetrics.mockResolvedValueOnce(reading);
  }
  MockCollector.mockImplementation(() => ({ collectMetrics }));
  return collectMetrics;
}

beforeEach(() => {
  mockedPost.mockResolvedValue({ status: 200 } as never);
});

afterEach(() => {
  jest.useRealTimers();
});

describe("VaultMonitor health surface", () => {
  it("reports a disconnected, empty state before it has run", () => {
    collectorReturning(metrics());
    const monitor = new VaultMonitor(config());

    const status = monitor.getStatus();
    expect(status.isConnected).toBe(false);
    expect(status.lastMetrics).toBeNull();
    expect(status.activeAlerts).toEqual([]);
    expect(typeof status.uptime).toBe("number");
  });

  it("marks itself connected and exposes the last reading once polling starts", async () => {
    const reading = metrics({ ledgerSequence: 4321 });
    collectorReturning(reading);
    const monitor = new VaultMonitor(config());

    await monitor.start();
    await monitor.stop();

    const status = monitor.getStatus();
    expect(status.isConnected).toBe(true);
    expect(status.lastMetrics).toEqual(reading);
  });

  it("exposes the insurance fund balance from the last reading", async () => {
    collectorReturning(metrics({ insuranceFundBalance: 250 }));
    const monitor = new VaultMonitor(config());

    await monitor.start();
    await monitor.stop();

    expect(monitor.getInsuranceFundBalance()).toBe(250);
    expect(monitor.getStatus().insuranceFundBalance).toBe(250);
  });

  it("keeps polling on the configured interval and stops when asked", async () => {
    jest.useFakeTimers();
    const collectMetrics = collectorReturning(metrics(), metrics(), metrics());
    const monitor = new VaultMonitor(config());

    await monitor.start();
    expect(collectMetrics).toHaveBeenCalledTimes(1);

    await jest.advanceTimersByTimeAsync(60_000);
    expect(collectMetrics).toHaveBeenCalledTimes(2);

    await monitor.stop();
    await jest.advanceTimersByTimeAsync(180_000);
    expect(collectMetrics).toHaveBeenCalledTimes(2);
  });
});

describe("VaultMonitor alerts", () => {
  it("dispatches what the engine detects and remembers it", async () => {
    collectorReturning(metrics({ isPaused: true }));
    const monitor = new VaultMonitor(config());

    await monitor.start();
    await monitor.stop();

    const alerts = monitor.getAlerts();
    expect(alerts.map((a) => a.type)).toContain("pause_duration_exceeded");
    expect(mockedPost).toHaveBeenCalledTimes(1);
    const payload = mockedPost.mock.calls[0][1] as Record<string, any>;
    expect(payload.severity).toBe("warning");
  });

  it("reports a lost RPC connection as a critical alert", async () => {
    collectorReturning(new Error("connect ECONNREFUSED"));
    const monitor = new VaultMonitor(config());

    await monitor.start();
    await monitor.stop();

    expect(monitor.getStatus().isConnected).toBe(false);
    const payload = mockedPost.mock.calls[0][1] as Record<string, any>;
    expect(payload.type).toBe("rpc_connectivity");
    expect(payload.severity).toBe("critical");
    expect(payload.message).toContain("https://rpc.testnet.stellar.org");
  });

  it("raises a critical insurance alert when the fund is under half the minimum", async () => {
    collectorReturning(metrics({ insuranceFundBalance: 400 }));
    const monitor = new VaultMonitor(
      config({ insurance: { minimumFundLevel: 1000 } }),
    );

    await monitor.start();
    await monitor.stop();

    const payload = mockedPost.mock.calls[0][1] as Record<string, any>;
    expect(payload.title).toBe("Insurance Fund Low");
    expect(payload.severity).toBe("critical");
    expect(payload.message).toContain("400");
    expect(payload.message).toContain("1000");
  });

  it("raises a warning between half the minimum and the minimum", async () => {
    collectorReturning(metrics({ insuranceFundBalance: 800 }));
    const monitor = new VaultMonitor(
      config({ insurance: { minimumFundLevel: 1000 } }),
    );

    await monitor.start();
    await monitor.stop();

    expect((mockedPost.mock.calls[0][1] as Record<string, any>).severity).toBe("warning");
  });

  it("stays quiet while the fund is healthy, and while no insurance config exists", async () => {
    collectorReturning(metrics({ insuranceFundBalance: 5000 }));
    const monitor = new VaultMonitor(
      config({ insurance: { minimumFundLevel: 1000 } }),
    );
    await monitor.start();
    await monitor.stop();
    expect(mockedPost).not.toHaveBeenCalled();

    mockedPost.mockClear();
    collectorReturning(metrics({ insuranceFundBalance: 1 }));
    const unconfigured = new VaultMonitor(config());
    await unconfigured.start();
    await unconfigured.stop();
    expect(mockedPost).not.toHaveBeenCalled();
  });

  it("does not repeat the insurance alert inside the cooldown window", async () => {
    const collectMetrics = collectorReturning(metrics({ insuranceFundBalance: 100 }), metrics({ insuranceFundBalance: 100 }));
    const monitor = new VaultMonitor(
      config({
        pollIntervalSeconds: 1,
        insurance: { minimumFundLevel: 1000, alertCooldownMs: 60 * 60 * 1000 },
      }),
    );

    await monitor.start();
    await monitor.stop();

    // A second cycle inside the cooldown: the collector is called again but the
    // insurance alert is not re-dispatched.
    await monitor.start();
    await monitor.stop();

    expect(collectMetrics).toHaveBeenCalled();
    const insuranceAlerts = mockedPost.mock.calls.filter(
      (call) => (call[1] as Record<string, any>).title === "Insurance Fund Low",
    );
    expect(insuranceAlerts).toHaveLength(1);
  });
});
