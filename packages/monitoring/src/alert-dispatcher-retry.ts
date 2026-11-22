/**
 * Alert Dispatcher with Retry/Backoff (#811)
 *
 * Adds exponential backoff + jitter retry for failed webhook deliveries.
 * Queues undeliverable alerts and flushes them on the next monitor tick.
 */

import axios from "axios";
import pino from "pino";
import { Alert, AlertWebhook } from "./types";

export interface AlertDispatcherRetryConfig {
  /** Maximum number of retry attempts (default: 3) */
  maxRetries?: number;
  /** Base delay in ms for exponential backoff (default: 1000) */
  baseDelayMs?: number;
  /** Maximum delay in ms (default: 30000) */
  maxDelayMs?: number;
  /** Maximum queued alerts before dropping (default: 100) */
  maxQueueSize?: number;
}

interface QueuedAlert {
  alert: Alert;
  webhook: AlertWebhook;
  attempts: number;
  nextRetryAt: number;
}

export class AlertDispatcherRetry {
  private logger = pino();
  private config: Required<AlertDispatcherRetryConfig>;
  private queue: QueuedAlert[] = [];
  private flushTimer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private webhooks: AlertWebhook[],
    config: AlertDispatcherRetryConfig = {},
  ) {
    this.config = {
      maxRetries: config.maxRetries ?? 3,
      baseDelayMs: config.baseDelayMs ?? 1000,
      maxDelayMs: config.maxDelayMs ?? 30000,
      maxQueueSize: config.maxQueueSize ?? 100,
    };
  }

  async dispatch(alert: Alert): Promise<void> {
    const relevantWebhooks = this.webhooks.filter(
      (w) =>
        !w.severity ||
        alert.severity === w.severity ||
        this.isSeverityHigher(alert.severity, w.severity || "info"),
    );

    for (const webhook of relevantWebhooks) {
      await this.sendWithRetry(alert, webhook);
    }
  }

  private async sendWithRetry(alert: Alert, webhook: AlertWebhook): Promise<void> {
    for (let attempt = 0; attempt <= this.config.maxRetries; attempt++) {
      try {
        await this.sendAlert(alert, webhook);
        this.logger.info(
          { webhook: webhook.name, alert_id: alert.id, attempt },
          "Alert sent successfully",
        );
        return;
      } catch (error) {
        const isLastAttempt = attempt === this.config.maxRetries;

        if (isLastAttempt) {
          this.logger.error(
            { error, webhook: webhook.name, alert_id: alert.id, attempts: attempt + 1 },
            "Alert delivery failed after all retries",
          );
          this.queueAlert(alert, webhook, attempt + 1);
          return;
        }

        // Calculate delay with exponential backoff + jitter
        const delay = this.calculateDelay(attempt);
        this.logger.warn(
          { error, webhook: webhook.name, alert_id: alert.id, attempt: attempt + 1, delayMs: delay },
          "Alert delivery failed, retrying",
        );
        await this.sleep(delay);
      }
    }
  }

  private calculateDelay(attempt: number): number {
    const exponentialDelay = this.config.baseDelayMs * Math.pow(2, attempt);
    const jitter = Math.random() * this.config.baseDelayMs;
    return Math.min(exponentialDelay + jitter, this.config.maxDelayMs);
  }

  private queueAlert(alert: Alert, webhook: AlertWebhook, attempts: number): void {
    if (this.queue.length >= this.config.maxQueueSize) {
      this.logger.warn(
        { queueSize: this.queue.length, alert_id: alert.id },
        "Alert queue full, dropping oldest alert",
      );
      this.queue.shift();
    }

    this.queue.push({
      alert,
      webhook,
      attempts,
      // Already exhausted the exponential-backoff schedule in sendWithRetry,
      // so the next attempt (a later flushQueue pass) waits the longest
      // configured tier rather than restarting the schedule from attempt 0.
      nextRetryAt: Date.now() + this.config.maxDelayMs,
    });

    this.logger.info(
      { queueSize: this.queue.length, alert_id: alert.id },
      "Alert queued for retry",
    );
  }

  startFlushTimer(intervalMs: number = 60000): void {
    if (this.flushTimer) return;

    this.flushTimer = setInterval(() => {
      this.flushQueue();
    }, intervalMs);
  }

  stopFlushTimer(): void {
    if (this.flushTimer) {
      clearInterval(this.flushTimer);
      this.flushTimer = null;
    }
  }

  private async flushQueue(): Promise<void> {
    const now = Date.now();
    const readyAlerts = this.queue.filter((q) => q.nextRetryAt <= now);
    this.queue = this.queue.filter((q) => q.nextRetryAt > now);

    for (const queued of readyAlerts) {
      this.logger.info(
        { alert_id: queued.alert.id, attempts: queued.attempts },
        "Retrying queued alert",
      );
      await this.sendWithRetry(queued.alert, queued.webhook);
    }
  }

  getQueueSize(): number {
    return this.queue.length;
  }

  private async sendAlert(alert: Alert, webhook: AlertWebhook): Promise<void> {
    const payload = this.formatPayload(alert, webhook.type);

    const response = await axios.post(webhook.url, payload, {
      timeout: 10000,
    });

    // Honor Retry-After header if present
    const retryAfter = response.headers["retry-after"];
    if (retryAfter) {
      const retryAfterMs = Number(retryAfter) * 1000;
      if (Number.isFinite(retryAfterMs) && retryAfterMs > 0) {
        await this.sleep(retryAfterMs);
      }
    }
  }

  private formatPayload(alert: Alert, type: string) {
    const severity_emoji = {
      info: "ℹ️",
      warning: "⚠️",
      critical: "🚨",
    };

    const emoji = severity_emoji[alert.severity];
    const timestamp = new Date(alert.timestamp).toISOString();

    if (type === "slack") {
      return {
        text: `${emoji} ${alert.title}`,
        blocks: [
          {
            type: "header",
            text: {
              type: "plain_text",
              text: `${emoji} ${alert.title}`,
            },
          },
          {
            type: "section",
            text: {
              type: "mrkdwn",
              text: alert.message,
            },
          },
          {
            type: "context",
            elements: [
              {
                type: "mrkdwn",
                text: `*Type:* ${alert.type} | *Severity:* ${alert.severity} | *Time:* ${timestamp}`,
              },
            ],
          },
          ...(alert.metrics
            ? [
                {
                  type: "section",
                  text: {
                    type: "mrkdwn",
                    text: `*Metrics:*\n${Object.entries(alert.metrics)
                      .map(([k, v]) => `• ${k}: ${v}`)
                      .join("\n")}`,
                  },
                },
              ]
            : []),
        ],
      };
    }

    if (type === "discord") {
      const color = { info: 3447003, warning: 15105570, critical: 15158332 }[
        alert.severity
      ];

      return {
        embeds: [
          {
            title: `${emoji} ${alert.title}`,
            description: alert.message,
            color,
            fields: alert.metrics
              ? Object.entries(alert.metrics).map(([k, v]) => ({
                  name: k,
                  value: String(v),
                  inline: true,
                }))
              : [],
            footer: {
              text: `Type: ${alert.type} | Severity: ${alert.severity}`,
            },
            timestamp: new Date(alert.timestamp).toISOString(),
          },
        ],
      };
    }

    if (type === "telegram") {
      let message = `${emoji} <b>${alert.title}</b>\n\n${alert.message}\n\n`;
      if (alert.metrics) {
        message += "<b>Metrics:</b>\n";
        message += Object.entries(alert.metrics)
          .map(([k, v]) => `<code>${k}</code>: ${v}`)
          .join("\n");
      }
      message += `\n\n<i>Type: ${alert.type} | Severity: ${alert.severity} | ${timestamp}</i>`;

      return {
        text: message,
        parse_mode: "HTML",
      };
    }

    return {
      alert_id: alert.id,
      type: alert.type,
      severity: alert.severity,
      title: alert.title,
      message: alert.message,
      metrics: alert.metrics,
      timestamp: alert.timestamp,
    };
  }

  private isSeverityHigher(current: string, minimum: string): boolean {
    const levels = { info: 0, warning: 1, critical: 2 };
    return (
      levels[current as keyof typeof levels] >=
      levels[minimum as keyof typeof levels]
    );
  }

  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}
