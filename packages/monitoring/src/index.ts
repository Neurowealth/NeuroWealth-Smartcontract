/**
 * NeuroWealth Vault Monitoring System
 *
 * Exports all monitoring components for external use
 */

export { VaultMonitor } from "./monitor";
export { MetricsCollector } from "./metrics-collector";
export { AlertEngine } from "./alert-engine";
export { AlertDispatcher } from "./alert-dispatcher";
export {
  DEFAULT_ALERT_THRESHOLDS,
  loadAlertThresholds,
  loadMonitoringConfig,
  logMonitoringConfig,
} from "./config";

export type {
  MonitoringConfig,
  AlertWebhook,
  AlertThresholds,
  HealthMetrics,
  Alert,
  AlertType,
  MonitoringState,
  RebalanceEvent,
  AuditLogEntry,
} from "./types";
