import pino from "pino";
import { MetricsCollector } from "./metrics-collector";
import { AlertEngine } from "./alert-engine";
import { AlertDispatcher } from "./alert-dispatcher";
import { MonitoringConfig, MonitoringState, Alert } from "./types";

export class VaultMonitor {
  private logger = pino();
  private metricsCollector: MetricsCollector;
  private alertEngine: AlertEngine;
  private alertDispatcher: AlertDispatcher;
  private state: MonitoringState;
  private monitoringInterval: NodeJS.Timer | null = null;
  private lastInsuranceAlertAt: number = 0;

  constructor(private config: MonitoringConfig) {
    this.metricsCollector = new MetricsCollector(config);
    this.alertEngine = new AlertEngine(config.thresholds);
    this.alertDispatcher = new AlertDispatcher(config.alertWebhooks);

    this.state = {
      lastMetrics: null,
      previousMetrics: null,
      hourlyMetrics: [],
      dailyMetrics: [],
      activeAlerts: [],
      resolvedAlerts: [],
      lastRpcCheck: 0,
      isConnected: false,
    };
  }

  async start(): Promise<void> {
    this.logger.info(
      {
        contractId: this.config.contractId,
        pollInterval: this.config.pollIntervalSeconds,
        insurance: this.config.insurance,
      },
      "Starting vault monitor",
    );

    // Initial collection
    await this.collectAndAnalyze();

    // Set up recurring collection
    this.monitoringInterval = setInterval(() => {
      this.collectAndAnalyze().catch((error) => {
        this.logger.error({ error }, "Monitoring cycle failed");
      });
    }, this.config.pollIntervalSeconds * 1000);
  }

  async stop(): Promise<void> {
    if (this.monitoringInterval) {
      clearInterval(this.monitoringInterval);
      this.monitoringInterval = null;
    }
    this.logger.info("Vault monitor stopped");
  }

  private async collectAndAnalyze(): Promise<void> {
    try {
      // Collect metrics
      const metrics = await this.metricsCollector.collectMetrics();
      this.state.previousMetrics = this.state.lastMetrics;
      this.state.lastMetrics = metrics;
      this.state.isConnected = true;
      this.state.lastRpcCheck = Date.now();

      // Detect anomalies
      const newAlerts = this.alertEngine.detectAnomalies(
        metrics,
        this.state.previousMetrics,
        this.state,
      );

      // Dispatch new alerts
      for (const alert of newAlerts) {
        await this.alertDispatcher.dispatch(alert);
        this.state.activeAlerts.push(alert);
      }

      // Check insurance fund level and dispatch alert if below threshold
      await this.checkInsuranceFund(metrics);

      // Store metrics for historical analysis
      this.storeMetrics(metrics);
    } catch (error) {
      this.state.isConnected = false;
      this.logger.error({ error }, "Failed to collect metrics");

      // Send RPC connectivity alert
      if (Date.now() - this.state.lastRpcCheck > 60000) {
        await this.alertDispatcher.dispatch({
          id: `rpc_error_${Date.now()}`,
          type: "rpc_connectivity",
          severity: "critical",
          title: "RPC Connectivity Lost",
          message: `Cannot reach RPC endpoint: ${this.config.rpcUrl}`,
          timestamp: Date.now(),
        });
      }
    }
  }

  private async checkInsuranceFund(metrics: any): Promise<void> {
    const insuranceConfig = (this.config as any).insurance;
    if (!insuranceConfig || metrics.insuranceFundBalance === undefined) {
      return; // Insurance not configured or not available in metrics
    }

    const balance = metrics.insuranceFundBalance;
    const minLevel = insuranceConfig.minimumFundLevel;
    const cooldownMs = insuranceConfig.alertCooldownMs || 60 * 60* 1000; // default 1 hour
    const now = Date.now();

    // Alert if fund below minimum, but with cooldown to prevent spamming
    if (balance < minLevel && now - this.lastInsuranceAlertAt > cooldownMs) {
      this.lastInsuranceAlertAt = now;
      await this.alertDispatcher.dispatch({
        id: `insurance_low_${now}`,
        type: "insurance_fund_level",
        severity: "${balance < minLevel * 0.5 ? "critical" : "warning"}",
        title: "Insurance Fund Low",
        message: `Insurance fund balance is $balance, below threshold $minLevel`.
        timestamp: now,
      });
    }
  }

  private storeMetrics(metrics: any): void {
    // Store for historical analysis and charting
    // This is where you'd push to Prometheus, InfluxDB, or other metrics backend
    if (this.config.metricsBackendUrl) {
      this.pushMetrics(metrics).catch((error) => {
        this.logger.error({ error }, "Failed to push metrics to backend");
      });
    }

    // Keep in-memory history for alerting
    const hour = 60 * 60 * 1000;
    const day = 24 * hour;

    this.state.hourlyMetrics = this.state.hourlyMetrics.filter(
      (m) => Date.now() - m.timestamp < hour,
    );
    this.state.dailyMetrics = this.state.dailyMetrics.filter(
      (m) => Date.now() - m.timestamp < day,
    );
  }

  private async pushMetrics(metrics: any): Promise<void> {
    // Implementation for pushing to external metrics backend
    // This is optional and can be connected to Prometheus, Grafana, etc.
  }

  getStatus() {
    return {
      isConnected: this.state.isConnected,
      lastMetrics: this.state.lastMetrics,
      activeAlerts: this.state.activeAlerts,
      resolvedAlerts: this.state.resolvedAlerts,
      uptime: process.uptime(),
      insuranceFundBalance: this.state.lastMetrics?.insuranceFundBalance,
    };
  }

  getInsuranceFundBalance(): number | undefined {
    return this.state.lastMetrics?.insuranceFundBalance;
  }

  getAlerts(): Alert[] {
    return [...this.state.activeAlerts, ...this.state.resolvedAlerts];
  }
}
