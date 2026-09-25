/**
 * Cross-chain bridge module exports
 */

export {
  BridgeManager,
  ALLOWED_TRANSITIONS,
  canTransition,
  backoffDelayMs,
  RECONCILE_BACKOFF_BASE_MS,
  RECONCILE_BACKOFF_MAX_MS,
} from "./bridge-manager";
export { InMemoryBridgeStore, SqlBridgeStore, isTerminalStatus, TERMINAL_STATUSES } from "./bridge-store";
export { BridgeMonitor } from "./bridge-monitor";
export { TransferReconciler } from "./bridge-recovery";
export { redactTransfer, sanitizeErrorMessage, MAX_PERSISTED_ERROR_LENGTH } from "./redaction";

export type {
  BridgeConfig,
  BridgeTransfer,
  BridgeQuote,
  BridgeStatus,
  BridgeDirection,
  BridgeChain,
  StoredBridgeTransfer,
  BridgeTransferPatch,
  TransferStage,
  AxelarGMPMessage,
  BridgeEvent,
} from "./types";

export { INITIAL_STAGE, SUBMITTED_STAGES } from "./types";

export type { ExternalChainState, ExternalTransferState } from "./bridge-manager";
export type { BridgeStore } from "./bridge-store";
export type {
  ReconciliationAction,
  ReconciliationResult,
  ReconciliationSummary,
  TransferReconcilerOptions,
} from "./bridge-recovery";
export type { RedactedTransfer } from "./redaction";
