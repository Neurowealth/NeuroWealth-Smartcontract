import { AlertDispatcher } from "../alert-dispatcher";
import { Alert, AlertWebhook } from "../types";

// Webhook fan-out (#701). The dispatcher decides who hears about an alert, so
// the tests pin the severity filter, the per-transport payload shape, and the
// promise that one broken webhook cannot stop the others.

jest.mock("axios");
import axios from "axios";

const mockedPost = axios.post as jest.MockedFunction<typeof axios.post>;

function alert(overrides: Partial<Alert> = {}): Alert {
  return {
    id: "tvl_drop_1700000000000",
    type: "tvl_drop",
    severity: "critical",
    title: "Critical: TVL Dropped Significantly",
    message: "TVL dropped from $1,000 to $700",
    metrics: { drop_percentage: "30.00" },
    timestamp: 1_700_000_000_000,
    ...overrides,
  };
}

function webhook(overrides: Partial<AlertWebhook> = {}): AlertWebhook {
  return {
    name: "ops-slack",
    type: "slack",
    url: "https://hooks.example.test/slack",
    ...overrides,
  };
}

beforeEach(() => {
  mockedPost.mockResolvedValue({ status: 200 } as never);
});

describe("AlertDispatcher severity filtering", () => {
  it("sends everything to a webhook with no severity floor", async () => {
    const dispatcher = new AlertDispatcher([webhook()]);
    await dispatcher.dispatch(alert({ severity: "info" }));
    expect(mockedPost).toHaveBeenCalledTimes(1);
  });

  it("sends a critical alert to a webhook configured for critical", async () => {
    const dispatcher = new AlertDispatcher([webhook({ severity: "critical" })]);
    await dispatcher.dispatch(alert({ severity: "critical" }));
    expect(mockedPost).toHaveBeenCalledTimes(1);
  });

  it("sends a critical alert to a webhook configured for warnings, since it is worse", async () => {
    const dispatcher = new AlertDispatcher([webhook({ severity: "warning" })]);
    await dispatcher.dispatch(alert({ severity: "critical" }));
    expect(mockedPost).toHaveBeenCalledTimes(1);
  });

  it("withholds a warning from a webhook that only wants critical alerts", async () => {
    const dispatcher = new AlertDispatcher([webhook({ severity: "critical" })]);
    await dispatcher.dispatch(alert({ severity: "warning" }));
    expect(mockedPost).not.toHaveBeenCalled();
  });

  it("withholds an info alert from a webhook that wants warnings", async () => {
    const dispatcher = new AlertDispatcher([webhook({ severity: "warning" })]);
    await dispatcher.dispatch(alert({ severity: "info" }));
    expect(mockedPost).not.toHaveBeenCalled();
  });

  it("reaches every matching webhook and skips the rest", async () => {
    const dispatcher = new AlertDispatcher([
      webhook({ name: "all" }),
      webhook({ name: "critical-only", severity: "critical", url: "https://hooks.example.test/crit" }),
      webhook({ name: "warning-floor", severity: "warning", url: "https://hooks.example.test/warn" }),
    ]);

    await dispatcher.dispatch(alert({ severity: "warning" }));

    const urls = mockedPost.mock.calls.map((call) => call[0]);
    expect(urls).toEqual([
      "https://hooks.example.test/slack",
      "https://hooks.example.test/warn",
    ]);
  });
});

describe("AlertDispatcher payloads", () => {
  it("builds a Slack message with a header, the message and a metrics block", async () => {
    const dispatcher = new AlertDispatcher([webhook({ type: "slack" })]);
    await dispatcher.dispatch(alert());

    const payload = mockedPost.mock.calls[0][1] as Record<string, any>;
    expect(payload.text).toContain("Critical: TVL Dropped Significantly");
    expect(payload.blocks[0].type).toBe("header");
    expect(payload.blocks[1].text.text).toBe("TVL dropped from $1,000 to $700");
    expect(payload.blocks[2].elements[0].text).toContain("tvl_drop");
    expect(payload.blocks[3].text.text).toContain("drop_percentage");
  });

  it("omits the metrics block when an alert carries none", async () => {
    const dispatcher = new AlertDispatcher([webhook({ type: "slack" })]);
    await dispatcher.dispatch(alert({ metrics: undefined }));

    const payload = mockedPost.mock.calls[0][1] as Record<string, any>;
    expect(payload.blocks).toHaveLength(3);
  });

  it("builds a Discord embed whose colour matches the severity", async () => {
    const dispatcher = new AlertDispatcher([webhook({ type: "discord" })]);
    await dispatcher.dispatch(alert({ severity: "warning" }));

    const payload = mockedPost.mock.calls[0][1] as Record<string, any>;
    expect(payload.embeds[0].color).toBe(15105570);
    expect(payload.embeds[0].fields[0]).toEqual({
      name: "drop_percentage",
      value: "30.00",
      inline: true,
    });
    expect(payload.embeds[0].footer.text).toContain("warning");
  });

  it("builds a Telegram message in HTML mode with code-formatted metrics", async () => {
    const dispatcher = new AlertDispatcher([webhook({ type: "telegram" })]);
    await dispatcher.dispatch(alert());

    const payload = mockedPost.mock.calls[0][1] as Record<string, any>;
    expect(payload.parse_mode).toBe("HTML");
    expect(payload.text).toContain("<b>");
    expect(payload.text).toContain("<code>drop_percentage</code>");
  });

  it("falls back to a plain object for a generic webhook", async () => {
    const dispatcher = new AlertDispatcher([webhook({ type: "webhook" })]);
    await dispatcher.dispatch(alert());

    const payload = mockedPost.mock.calls[0][1] as Record<string, any>;
    expect(payload).toMatchObject({
      alert_id: "tvl_drop_1700000000000",
      type: "tvl_drop",
      severity: "critical",
    });
  });

  it("posts with a bounded timeout so a hanging webhook cannot wedge the monitor", async () => {
    const dispatcher = new AlertDispatcher([webhook()]);
    await dispatcher.dispatch(alert());
    expect(mockedPost.mock.calls[0][2]).toMatchObject({ timeout: 10000 });
  });
});

describe("AlertDispatcher failure isolation", () => {
  it("keeps delivering to the remaining webhooks when one rejects", async () => {
    mockedPost
      .mockRejectedValueOnce(new Error("slack is down"))
      .mockResolvedValueOnce({ status: 200 } as never);

    const dispatcher = new AlertDispatcher([
      webhook({ name: "flaky", url: "https://hooks.example.test/flaky" }),
      webhook({ name: "healthy", url: "https://hooks.example.test/healthy" }),
    ]);

    await expect(dispatcher.dispatch(alert())).resolves.toBeUndefined();
    expect(mockedPost).toHaveBeenCalledTimes(2);
    expect(mockedPost.mock.calls[1][0]).toBe("https://hooks.example.test/healthy");
  });

  it("does nothing when no webhooks are configured", async () => {
    const dispatcher = new AlertDispatcher([]);
    await dispatcher.dispatch(alert());
    expect(mockedPost).not.toHaveBeenCalled();
  });
});
