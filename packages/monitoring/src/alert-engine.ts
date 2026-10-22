/**
 * Detects anomalies and generates alerts
 */

import pino from "pino";
import {
  Alert,
  AlertType,
  HealthMetrics,
  MonitoringState,
  AlertThresholds,
} from "./types";

export class AlertEngine {
  private logger = pino();

  constructor(private thresholds: AlertThresholds) {}

  detectAnomalies(
    current: HealthMetrics,
    previous: HealthMetrics | null,
    state: MonitoringState,
  ): Alert[] {
    const alerts: Alert[] = [];

    if (previous) {
      // TVL drop detection
      const tvlDropAlert = this.checkTvlDrop(current, previous);
      if (tvlDropAlert) alerts.push(tvlDropAlert);

      // Share price decrease
      const sharePriceAlert = this.checkSharePriceDecrease(current, previous);
      if (sharePriceAlert) alerts.push(sharePriceAlert);

      // Withdrawal spike (basic check)
      if (state.hourlyMetrics.length > 0) {
        const withdrawalAlert = this.checkWithdrawalSpike(current, previous);
        if (withdrawalAlert) alerts.push(withdrawalAlert);
      }
    }

    // Cap saturation
    const capAlert = this.checkCapSaturation(current);
    if (capAlert) alerts.push(capAlert);

    // Pause duration
    const pauseAlert = this.checkPauseDuration(current);
    if (pauseAlert) alerts.push(pauseAlert);

    // Timelock monitoring
    const timelockAlerts = this.checkTimelocksMonitoring(current);
    alerts.push(...timelockAlerts);

    return alerts;
  }

  private checkTvlDrop(
    current: HealthMetrics,
    previous: HealthMetrics,
  ): Alert | null {
    const tvlDropThreshold = this.thresholds.tvlDropPercentage / 100;
    const tvlDropFactor = Number(current.tvl) / Number(previous.tvl);

    if (tvlDropFactor < 1 - tvlDropThreshold) {
      return this.createAlert(
        "tvl_drop",
        "critical",
        "Critical: TVL Dropped Significantly",
        `TVL dropped from ${this.formatUsdc(previous.tvl)} to ${this.formatUsdc(current.tvl)} ` +
          `(${((1 - tvlDropFactor) * 100).toFixed(2)}% loss)`,
        {
          previous_tvl: Number(previous.tvl),
          current_tvl: Number(current.tvl),
          drop_percentage: ((1 - tvlDropFactor) * 100).toFixed(2),
        },
      );
    }

    return null;
  }

  private checkSharePriceDecrease(
    current: HealthMetrics,
    previous: HealthMetrics,
  ): Alert | null {
    if (previous.sharePrice <= 0) {
      return null;
    }

    if (current.sharePrice <= previous.sharePrice * 0.99) {
      return this.createAlert(
        "share_price_decrease",
        "critical",
        "Critical: Share Price Decreased",
        `Share price decreased from ${previous.sharePrice.toFixed(4)} to ${current.sharePrice.toFixed(4)}. ` +
          `This should never happen. Investigate immediately.`,
        {
          previous_price: previous.sharePrice,
          current_price: current.sharePrice,
          change_percent: (
            ((current.sharePrice - previous.sharePrice) / previous.sharePrice) *
            100
          ).toFixed(2),
        },
      );
    }

    return null;
  }

  private checkWithdrawalSpike(
    current: HealthMetrics,
    previous: HealthMetrics,
  ): Alert | null {
    const withdrawalDelta = Number(
      previous.totalDeposits - current.totalDeposits,
    );

    if (withdrawalDelta > 0) {
      const withdrawalPercent =
        (withdrawalDelta / Number(previous.totalDeposits)) * 100;

      if (withdrawalPercent > 10) {
        // More than 10% withdrawn
        return this.createAlert(
          "withdrawal_spike",
          "warning",
          "Withdrawal Spike Detected",
          `Unusual withdrawal activity: ${this.formatUsdc(BigInt(Math.floor(withdrawalDelta)))} withdrawn ` +
            `(${withdrawalPercent.toFixed(2)}% of previous deposits)`,
          {
            withdrawn_amount: withdrawalDelta,
            percentage_of_deposits: withdrawalPercent.toFixed(2),
          },
        );
      }
    }

    return null;
  }

  private checkCapSaturation(metrics: HealthMetrics): Alert | null {
    const saturation = Number(metrics.tvl) / Number(metrics.tvlCap);

    if (saturation > this.thresholds.capSaturationPercentage / 100) {
      return this.createAlert(
        "cap_saturation",
        "warning",
        "TVL Cap Approaching Saturation",
        `Current TVL is ${(saturation * 100).toFixed(2)}% of the TVL cap. ` +
          `Consider raising the cap to avoid blocking new deposits.`,
        {
          current_tvl: Number(metrics.tvl),
          tvl_cap: Number(metrics.tvlCap),
          saturation_percent: (saturation * 100).toFixed(2),
        },
      );
    }

    return null;
  }

  private checkPauseDuration(metrics: HealthMetrics): Alert | null {
    if (metrics.isPaused) {
      return this.createAlert(
        "pause_duration_exceeded",
        "warning",
        "Vault is Paused",
        `The vault has been paused. Verify this is intentional and normal operations will resume soon.`,
        {
          is_paused: true,
          ledger: metrics.ledgerSequence,
        },
      );
    }

    return null;
  }

  private checkTimelocksMonitoring(metrics: HealthMetrics): Alert[] {
    const alerts: Alert[] = [];

    if (metrics.pendingUpgrade) {
      alerts.push(
        this.createAlert(
          "upgrade_scheduled",
          "warning",
          "Contract Upgrade Scheduled",
          `A contract upgrade is pending. It will become executable at ledger ${metrics.pendingUpgrade.expiryLedger}. ` +
            `Current ledger: ${metrics.ledgerSequence}. Ledgers remaining: ${Math.max(0, metrics.pendingUpgrade.expiryLedger - metrics.ledgerSequence)}`,
          {
            wasm_hash: metrics.pendingUpgrade.hash.slice(0, 16) + "...",
            expiry_ledger: metrics.pendingUpgrade.expiryLedger,
            current_ledger: metrics.ledgerSequence,
            ledgers_remaining: Math.max(
              0,
              metrics.pendingUpgrade.expiryLedger - metrics.ledgerSequence,
            ),
          },
        ),
      );
    }

    if (metrics.pendingAgent) {
      alerts.push(
        this.createAlert(
          "agent_update_proposed",
          "warning",
          "Agent Update Proposed",
          `An agent update is pending. It will become confirmable at ledger ${metrics.pendingAgent.expiryLedger}. ` +
            `Verify this change is authorized.`,
          {
            new_agent: metrics.pendingAgent.hash.slice(0, 16) + "...",
            expiry_ledger: metrics.pendingAgent.expiryLedger,
            current_ledger: metrics.ledgerSequence,
            ledgers_remaining: Math.max(
              0,
              metrics.pendingAgent.expiryLedger - metrics.ledgerSequence,
            ),
          },
        ),
      );
    }

    return alerts;
  }

  private createAlert(
    type: AlertType,
    severity: "info" | "warning" | "critical",
    title: string,
    message: string,
    metrics?: Record<string, any>,
  ): Alert {
    return {
      id: `${type}_${Date.now()}`,
      type,
      severity,
      title,
      message,
      metrics,
      timestamp: Date.now(),
    };
  }

  private formatUsdc(amount: bigint): string {
    const usdc = Number(amount) / 10 ** 7;
    return `$${usdc.toLocaleString(undefined, { maximumFractionDigits: 2 })}`;
  }
}
