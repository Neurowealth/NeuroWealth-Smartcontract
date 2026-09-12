import { AlertEngine } from "../alert-engine";
import { AlertThresholds, HealthMetrics, MonitoringState } from "../types";

// Alert generation (#701). These tests pin the thresholds the engine claims to
// enforce, including the boundary cases a refactor could quietly move: a drop
// that is exactly at the threshold must NOT alert, and one just past it must.

const THRESHOLDS: AlertThresholds = {
  tvlDropPercentage: 20,
  withdrawalSpikeFactor: 3,
  pauseDurationLedgers: 17280,
  capSaturationPercentage: 95,
};

function metrics(overrides: Partial<HealthMetrics> = {}): HealthMetrics {
  return {
    timestamp: 1_700_000_000_000,
    ledgerSequence: 1000,
    tvl: 1_000_0000000n, // 1,000 USDC at 7 decimals
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

function state(overrides: Partial<MonitoringState> = {}): MonitoringState {
  return {
    lastMetrics: null,
    previousMetrics: null,
    hourlyMetrics: [],
    dailyMetrics: [],
    activeAlerts: [],
    resolvedAlerts: [],
    lastRpcCheck: 0,
    isConnected: true,
    ...overrides,
  };
}

const engine = () => new AlertEngine(THRESHOLDS);
const typesOf = (alerts: ReturnType<AlertEngine["detectAnomalies"]>) =>
  alerts.map((a) => a.type).sort();

describe("AlertEngine TVL drop", () => {
  it("raises a critical alert for a drop past the threshold", () => {
    const alerts = engine().detectAnomalies(
      metrics({ tvl: 700_0000000n }),
      metrics({ tvl: 1_000_0000000n }),
      state(),
    );
    const alert = alerts.find((a) => a.type === "tvl_drop");

    expect(alert).toBeDefined();
    expect(alert!.severity).toBe("critical");
    expect(alert!.message).toMatch(/30.00% loss/);
    expect(alert!.metrics?.drop_percentage).toBe("30.00");
  });

  it("does not alert when the drop equals the threshold exactly", () => {
    // A 20% drop against a 20% threshold is the boundary the comparison excludes.
    const alerts = engine().detectAnomalies(
      metrics({ tvl: 800_0000000n }),
      metrics({ tvl: 1_000_0000000n }),
      state(),
    );
    expect(typesOf(alerts)).not.toContain("tvl_drop");
  });

  it("does not alert on growth", () => {
    const alerts = engine().detectAnomalies(
      metrics({ tvl: 2_000_0000000n }),
      metrics({ tvl: 1_000_0000000n }),
      state(),
    );
    expect(typesOf(alerts)).not.toContain("tvl_drop");
  });

  it("needs a previous reading before it can compare anything", () => {
    const alerts = engine().detectAnomalies(metrics({ tvl: 1n }), null, state());
    expect(typesOf(alerts)).not.toContain("tvl_drop");
    expect(typesOf(alerts)).not.toContain("share_price_decrease");
  });
});

describe("AlertEngine share price", () => {
  it("flags any decrease past one percent", () => {
    const alerts = engine().detectAnomalies(
      metrics({ sharePrice: 0.98 }),
      metrics({ sharePrice: 1 }),
      state(),
    );
    const alert = alerts.find((a) => a.type === "share_price_decrease");
    expect(alert).toBeDefined();
    expect(alert!.severity).toBe("critical");
    expect(alert!.metrics?.change_percent).toBe("-2.00");
  });

  it("treats exactly one percent as acceptable", () => {
    const alerts = engine().detectAnomalies(
      metrics({ sharePrice: 0.99 }),
      metrics({ sharePrice: 1 }),
      state(),
    );
    expect(typesOf(alerts)).not.toContain("share_price_decrease");
  });
});

describe("AlertEngine withdrawal spike", () => {
  const withHistory = state({
    hourlyMetrics: [{ timestamp: Date.now(), ledger: 1, value: 1, unit: "usdc" }],
  });

  it("warns when more than ten percent of deposits left", () => {
    const alerts = engine().detectAnomalies(
      metrics({ totalDeposits: 800_0000000n }),
      metrics({ totalDeposits: 1_000_0000000n }),
      withHistory,
    );
    const alert = alerts.find((a) => a.type === "withdrawal_spike");
    expect(alert).toBeDefined();
    expect(alert!.severity).toBe("warning");
    expect(alert!.metrics?.percentage_of_deposits).toBe("20.00");
  });

  it("stays quiet for ordinary withdrawals", () => {
    const alerts = engine().detectAnomalies(
      metrics({ totalDeposits: 950_0000000n }),
      metrics({ totalDeposits: 1_000_0000000n }),
      withHistory,
    );
    expect(typesOf(alerts)).not.toContain("withdrawal_spike");
  });

  it("stays quiet when deposits grew", () => {
    const alerts = engine().detectAnomalies(
      metrics({ totalDeposits: 1_200_0000000n }),
      metrics({ totalDeposits: 1_000_0000000n }),
      withHistory,
    );
    expect(typesOf(alerts)).not.toContain("withdrawal_spike");
  });

  it("is skipped entirely when no history has been collected yet", () => {
    const alerts = engine().detectAnomalies(
      metrics({ totalDeposits: 100_0000000n }),
      metrics({ totalDeposits: 1_000_0000000n }),
      state(),
    );
    expect(typesOf(alerts)).not.toContain("withdrawal_spike");
  });
});

describe("AlertEngine caps and pause", () => {
  it("warns when TVL approaches the cap", () => {
    const alerts = engine().detectAnomalies(
      metrics({ tvl: 9_600_0000000n, tvlCap: 10_000_0000000n }),
      null,
      state(),
    );
    const alert = alerts.find((a) => a.type === "cap_saturation");
    expect(alert).toBeDefined();
    expect(alert!.metrics?.saturation_percent).toBe("96.00");
  });

  it("does not warn at or below the saturation threshold", () => {
    const alerts = engine().detectAnomalies(
      metrics({ tvl: 9_500_0000000n, tvlCap: 10_000_0000000n }),
      null,
      state(),
    );
    expect(typesOf(alerts)).not.toContain("cap_saturation");
  });

  it("warns for the whole time the vault is paused", () => {
    const alerts = engine().detectAnomalies(metrics({ isPaused: true }), null, state());
    const alert = alerts.find((a) => a.type === "pause_duration_exceeded");
    expect(alert).toBeDefined();
    expect(alert!.metrics?.ledger).toBe(1000);
  });

  it("does not warn when the vault is running", () => {
    expect(typesOf(engine().detectAnomalies(metrics(), null, state()))).not.toContain(
      "pause_duration_exceeded",
    );
  });
});

describe("AlertEngine timelocks", () => {
  it("reports a pending upgrade with the ledgers remaining", () => {
    const alerts = engine().detectAnomalies(
      metrics({ pendingUpgrade: { hash: "abcdef0123456789", expiryLedger: 1500 } }),
      null,
      state(),
    );
    const alert = alerts.find((a) => a.type === "upgrade_scheduled");
    expect(alert).toBeDefined();
    expect(alert!.severity).toBe("warning");
    expect(alert!.metrics?.ledgers_remaining).toBe(500);
    expect(alert!.metrics?.wasm_hash).toBe("abcdef0123456789...");
  });

  it("clamps ledgers remaining at zero for an expired timelock", () => {
    const alerts = engine().detectAnomalies(
      metrics({ pendingUpgrade: { hash: "aa", expiryLedger: 10 } }),
      null,
      state(),
    );
    const alert = alerts.find((a) => a.type === "upgrade_scheduled");
    expect(alert!.metrics?.ledgers_remaining).toBe(0);
  });

  it("reports a proposed agent update separately", () => {
    const alerts = engine().detectAnomalies(
      metrics({ pendingAgent: { hash: "bb", expiryLedger: 2000 } }),
      null,
      state(),
    );
    const alert = alerts.find((a) => a.type === "agent_update_proposed");
    expect(alert).toBeDefined();
    expect(alert!.message).toMatch(/authorized/i);
  });

  it("emits nothing for timelocks when none are pending", () => {
    const types = typesOf(engine().detectAnomalies(metrics(), null, state()));
    expect(types).not.toContain("upgrade_scheduled");
    expect(types).not.toContain("agent_update_proposed");
  });
});

describe("AlertEngine alert shape", () => {
  it("stamps every alert with an id, a timestamp and a readable message", () => {
    const alerts = engine().detectAnomalies(metrics({ isPaused: true }), null, state());
    expect(alerts).toHaveLength(1);
    const alert = alerts[0];
    expect(alert.id).toMatch(new RegExp("^pause_duration_exceeded_" + "\\d+$"));
    expect(typeof alert.timestamp).toBe("number");
    expect(alert.title.length).toBeGreaterThan(0);
    expect(alert.message.length).toBeGreaterThan(0);
  });
});
