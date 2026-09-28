/**
 * Cross-chain bridge types
 */

export type BridgeChain = "stellar" | "ethereum";
export type BridgeDirection = "deposit" | "withdraw";
export type BridgeStatus =
  | "pending"
  | "confirming"
  | "confirmed"
  | "failed"
  | "cancelled";

/**
 * Durable workflow stage of a transfer (#848).
 *
 * The stage is persisted alongside the transfer so a restarted process can
 * resume from the exact point it stopped at instead of re-deriving it from
 * process memory:
 *
 * - `observed`  - source-chain transfer seen, nothing submitted to the bridge
 * - `submitted` - the bridge message is in flight (`bridgeTxHash` recorded)
 * - `confirmed` - the destination chain confirmed the transfer
 * - `completed` - terminal, user-visible settlement finished
 */
export type TransferStage = "observed" | "submitted" | "confirmed" | "completed";

/** Stage a transfer starts in. */
export const INITIAL_STAGE: TransferStage = "observed";

/** Stages from which a transfer must never be submitted again. */
export const SUBMITTED_STAGES: readonly TransferStage[] = [
  "submitted",
  "confirmed",
  "completed",
];

export interface BridgeConfig {
  // Stellar
  stellarRpcUrl: string;
  stellarNetworkPassphrase: string;
  stellarVaultContractId: string;
  stellarUsdcTokenId: string;

  // Ethereum
  ethereumRpcUrl: string;
  ethereumChainId: number;
  ethereumVaultContractAddress: string;
  ethereumUsdcTokenAddress: string;

  // Axelar
  axelarApiUrl: string;
  axelarChainName: string;
  axelarGasServiceAddress: string;

  // Bridge settings
  bridgeFeePercentage: number; // 0.5 = 0.5%
  minBridgeAmount: bigint;
  maxBridgeAmount: bigint;

  // #851 - Confirmation depth settings per destination chain
  confirmationDepths: Record<BridgeChain, number>; // Number of blocks/ledgers to wait before confirming
}

export interface BridgeTransfer {
  id: string;
  status: BridgeStatus;
  direction: BridgeDirection;
  sourceChain: BridgeChain;
  destinationChain: BridgeChain;
  user: string;
  amount: bigint;
  bridgeFee: bigint;
  netAmount: bigint;
  sourceChainTxHash?: string;
  bridgeTxHash?: string;
  destinationTxHash?: string;
  createdAt: number;
  updatedAt: number;
  errorMessage?: string;
  estimatedArrivalTime?: number;
  idempotencyKey?: string; // #849 - Idempotency key for safe retries
  currentConfirmationDepth?: number; // #851 - Current confirmation depth for monitoring
  requiredConfirmationDepth?: number; // #851 - Required confirmation depth for this transfer
}

export interface BridgeQuote {
  amount: bigint;
  bridgeFee: bigint;
  netAmount: bigint;
  estimatedTime: number; // seconds
  slippagePercentage: number;
}

export interface AxelarGMPMessage {
  messageId: string;
  sourceChain: string;
  destinationChain: string;
  sourceAddress: string;
  destinationAddress: string;
  payload: string;
  status: "pending" | "approved" | "executed" | "failed";
}

export interface BridgeEvent {
  type:
    | "deposit_initiated"
    | "withdraw_initiated"
    | "transfer_confirmed"
    | "transfer_failed"
    | "transfer_completed";
  bridgeTransferId: string;
  timestamp: number;
  details: Record<string, any>;
}

export interface StoredBridgeTransfer extends BridgeTransfer {
  retriesRemaining: number;
  lastRetryTime?: number;

  // ── #848 durable workflow state ────────────────────────────────────────
  /** Workflow stage the transfer had reached when it was last persisted. */
  stage: TransferStage;
  /** How many times an external submission/reconciliation has been attempted. */
  attemptCount: number;
  /** Sanitised description of the last failure, or `undefined` when healthy. */
  lastError?: string;
  /** Earliest timestamp (ms) at which an unknown external state may be retried. */
  nextAttemptAt?: number;
  /** Last time a reconciliation pass inspected this record (ms). */
  lastReconciledAt?: number;
}

/** Fields a reconciliation pass may write back to a durable record. */
export type BridgeTransferPatch = Partial<
  Omit<StoredBridgeTransfer, "id">
>;
