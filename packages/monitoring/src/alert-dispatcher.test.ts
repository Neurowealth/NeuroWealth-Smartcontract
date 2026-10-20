/**
 * Tests for AlertDispatcher
 */

import { AlertDispatcher } from "./alert-dispatcher";
import { Alert, AlertWebhook } from "./types";
import axios from "axios";

// Mock axios
jest.mock("axios");
const mockedAxios = axios as jest.Mocked<typeof axios>;

// Mock pino logger
jest.mock("pino", () => {
  return jest.fn(() => ({
    info: jest.fn(),
    error: jest.fn(),
    warn: jest.fn(),
    debug: jest.fn(),
  }));
});

describe("AlertDispatcher", () => {
  let dispatcher: AlertDispatcher;
  let mockWebhooks: AlertWebhook[];
  let testAlert: Alert;

  beforeEach(() => {
    jest.clearAllMocks();

    mockWebhooks = [
      {
        name: "slack-webhook",
        type: "slack",
        url: "https://hooks.slack.com/test",
        severity: "critical",
      },
      {
        name: "discord-webhook",
        type: "discord",
        url: "https://discord.com/api/webhooks/test",
        severity: "warning",
      },
      {
        name: "generic-webhook",
        type: "webhook",
        url: "https://example.com/webhook",
      },
    ];

    dispatcher = new AlertDispatcher(mockWebhooks);

    testAlert = {
      id: "test-alert-123",
      type: "tvl_drop",
      severity: "critical",
      title: "TVL Dropped Significantly",
      message: "TVL dropped from $100M to $75M (25% loss)",
      metrics: {
        previous_tvl: 100000000,
        current_tvl: 75000000,
        drop_percentage: "25.00",
      },
      timestamp: Date.now(),
    };

    mockedAxios.post.mockResolvedValue({ status: 200, data: {} });
  });

  describe("dispatch", () => {
    it("should send alert to relevant webhooks", async () => {
      await dispatcher.dispatch(testAlert);

      expect(mockedAxios.post).toHaveBeenCalledTimes(3);
    });

    it("should only send to webhooks matching severity filter", async () => {
      const warningAlert: Alert = {
        ...testAlert,
        severity: "warning",
      };

      await dispatcher.dispatch(warningAlert);

      // Should send to discord (warning), generic (no filter), and skip slack (critical only)
      expect(mockedAxios.post).toHaveBeenCalledTimes(2);
    });

    it("should send info severity to info+ webhooks", async () => {
      const infoAlert: Alert = {
        ...testAlert,
        severity: "info",
      };

      await dispatcher.dispatch(infoAlert);

      // Only generic webhook (no severity filter) should receive info
      expect(mockedAxios.post).toHaveBeenCalledTimes(1);
    });

    it("should handle webhook failures gracefully", async () => {
      mockedAxios.post.mockRejectedValueOnce(new Error("Network error"));

      await dispatcher.dispatch(testAlert);

      // Should still attempt all webhooks despite one failure
      expect(mockedAxios.post).toHaveBeenCalledTimes(3);
    });

    it("should set correct timeout for requests", async () => {
      await dispatcher.dispatch(testAlert);

      mockedAxios.post.mock.calls.forEach((call) => {
        expect(call[2]).toEqual({ timeout: 10000 });
      });
    });
  });

  describe("Slack Format", () => {
    it("should format payload for Slack", async () => {
      const slackWebhook = mockWebhooks.find((w) => w.type === "slack");
      const slackDispatcher = new AlertDispatcher([slackWebhook!]);

      await slackDispatcher.dispatch(testAlert);

      const payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload).toHaveProperty("text");
      expect(payload).toHaveProperty("blocks");
      expect(payload.blocks).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ type: "header" }),
          expect.objectContaining({ type: "section" }),
          expect.objectContaining({ type: "context" }),
        ]),
      );
    });

    it("should include metrics in Slack format when present", async () => {
      const slackWebhook = mockWebhooks.find((w) => w.type === "slack");
      const slackDispatcher = new AlertDispatcher([slackWebhook!]);

      await slackDispatcher.dispatch(testAlert);

      const payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      const metricsBlock = payload.blocks.find((b: any) => b.type === "section" && b.text?.text?.includes("Metrics"));
      expect(metricsBlock).toBeDefined();
    });

    it("should use correct emoji for Slack severity", async () => {
      // Deliberately no `severity` filter: mockWebhooks' own slack entry is
      // pinned to "critical", which would filter out the warning-severity
      // dispatch below before it ever reaches sendAlert. This test is about
      // the emoji formatting, not severity filtering (covered separately
      // under "Severity Filtering").
      const slackWebhook = { ...mockWebhooks.find((w) => w.type === "slack")!, severity: undefined };
      const slackDispatcher = new AlertDispatcher([slackWebhook]);

      const criticalAlert = { ...testAlert, severity: "critical" as const };
      await slackDispatcher.dispatch(criticalAlert);

      let payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload.text).toContain("🚨");

      mockedAxios.post.mockClear();

      const warningSlackWebhook: AlertWebhook = {
        ...slackWebhook!,
        severity: "warning",
      };
      const warningSlackDispatcher = new AlertDispatcher([warningSlackWebhook]);
      const warningAlert = { ...testAlert, severity: "warning" as const };
      await warningSlackDispatcher.dispatch(warningAlert);

      payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload.text).toContain("⚠️");
    });
  });

  describe("Discord Format", () => {
    it("should format payload for Discord", async () => {
      const discordWebhook = mockWebhooks.find((w) => w.type === "discord");
      const discordDispatcher = new AlertDispatcher([discordWebhook!]);

      await discordDispatcher.dispatch(testAlert);

      const payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload).toHaveProperty("embeds");
      expect(payload.embeds).toHaveLength(1);
      expect(payload.embeds[0]).toHaveProperty("title");
      expect(payload.embeds[0]).toHaveProperty("description");
      expect(payload.embeds[0]).toHaveProperty("color");
      expect(payload.embeds[0]).toHaveProperty("footer");
      expect(payload.embeds[0]).toHaveProperty("timestamp");
    });

    it("should use correct color for Discord severity", async () => {
      const discordWebhook = mockWebhooks.find((w) => w.type === "discord");
      const discordDispatcher = new AlertDispatcher([discordWebhook!]);

      const criticalAlert = { ...testAlert, severity: "critical" as const };
      await discordDispatcher.dispatch(criticalAlert);

      let payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload.embeds[0].color).toBe(15158332); // Red for critical

      mockedAxios.post.mockClear();

      const warningAlert = { ...testAlert, severity: "warning" as const };
      await discordDispatcher.dispatch(warningAlert);

      payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload.embeds[0].color).toBe(15105570); // Orange for warning
    });

    it("should include metrics as fields in Discord", async () => {
      const discordWebhook = mockWebhooks.find((w) => w.type === "discord");
      const discordDispatcher = new AlertDispatcher([discordWebhook!]);

      await discordDispatcher.dispatch(testAlert);

      const payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload.embeds[0].fields).toBeDefined();
      expect(payload.embeds[0].fields.length).toBeGreaterThan(0);
      expect(payload.embeds[0].fields).toContainEqual(
        expect.objectContaining({
          name: "previous_tvl",
          value: "100000000",
        }),
      );
    });
  });

  describe("Telegram Format", () => {
    it("should format payload for Telegram", async () => {
      const telegramWebhook: AlertWebhook = {
        name: "telegram-webhook",
        type: "telegram",
        url: "https://api.telegram.org/test",
      };
      const telegramDispatcher = new AlertDispatcher([telegramWebhook]);

      await telegramDispatcher.dispatch(testAlert);

      const payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload).toHaveProperty("text");
      expect(payload).toHaveProperty("parse_mode");
      expect(payload.parse_mode).toBe("HTML");
    });

    it("should format message as HTML for Telegram", async () => {
      const telegramWebhook: AlertWebhook = {
        name: "telegram-webhook",
        type: "telegram",
        url: "https://api.telegram.org/test",
      };
      const telegramDispatcher = new AlertDispatcher([telegramWebhook]);

      await telegramDispatcher.dispatch(testAlert);

      const payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload.text).toContain("<b>");
      expect(payload.text).toContain("</b>");
      expect(payload.text).toContain("<code>");
      expect(payload.text).toContain("<i>");
    });
  });

  describe("Generic Webhook Format", () => {
    it("should format payload for generic webhook", async () => {
      const genericWebhook: AlertWebhook = {
        name: "generic-webhook",
        type: "webhook",
        url: "https://example.com/alerts",
      };
      const genericDispatcher = new AlertDispatcher([genericWebhook]);

      await genericDispatcher.dispatch(testAlert);

      const payload = mockedAxios.post.mock.calls[0][1] as Record<string, any>;
      expect(payload.alert_id).toBe(testAlert.id);
      expect(payload.type).toBe(testAlert.type);
      expect(payload.severity).toBe(testAlert.severity);
      expect(payload.title).toBe(testAlert.title);
      expect(payload.message).toBe(testAlert.message);
      expect(payload.metrics).toEqual(testAlert.metrics);
      expect(payload.timestamp).toBe(testAlert.timestamp);
    });
  });

  describe("Alert Without Metrics", () => {
    it("should handle alerts without metrics", async () => {
      const alertWithoutMetrics: Alert = {
        ...testAlert,
        metrics: undefined,
      };

      await dispatcher.dispatch(alertWithoutMetrics);

      // Should not throw and should send to all webhooks
      expect(mockedAxios.post).toHaveBeenCalledTimes(3);
    });
  });

  describe("Severity Filtering", () => {
    it("should send critical alerts to warning+ webhooks", async () => {
      const webhooksWithSeverity: AlertWebhook[] = [
        { name: "warning-only", type: "webhook", url: "https://warning.example.com", severity: "warning" },
        { name: "critical-only", type: "webhook", url: "https://critical.example.com", severity: "critical" },
      ];

      const testDispatcher = new AlertDispatcher(webhooksWithSeverity);
      const criticalAlert = { ...testAlert, severity: "critical" as const };

      await testDispatcher.dispatch(criticalAlert);

      // Both should receive the critical alert
      expect(mockedAxios.post).toHaveBeenCalledTimes(2);
    });

    it("should not send warning to critical-only webhooks", async () => {
      const webhooksWithSeverity: AlertWebhook[] = [
        { name: "critical-only", type: "webhook", url: "https://critical.example.com", severity: "critical" },
      ];

      const testDispatcher = new AlertDispatcher(webhooksWithSeverity);
      const warningAlert = { ...testAlert, severity: "warning" as const };

      await testDispatcher.dispatch(warningAlert);

      expect(mockedAxios.post).not.toHaveBeenCalled();
    });
  });
});
