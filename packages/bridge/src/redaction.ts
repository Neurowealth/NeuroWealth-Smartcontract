/**
 * Redaction helpers for persisted and logged bridge state (#848).
 *
 * Durable transfer records and log lines must never contain credentials or the
 * full cross-chain payload: a persisted record is read back by every operator
 * and shipped to log aggregators, so only the fields needed to resume a
 * workflow are kept, and free-form error text is scrubbed before it is stored.
 */

import { StoredBridgeTransfer } from "./types";

/** Longest error message persisted on a transfer record. */
export const MAX_PERSISTED_ERROR_LENGTH = 200;

/** Patterns that commonly carry credentials inside upstream error text. */
const SECRET_PATTERNS: ReadonlyArray<RegExp> = [
  // `secret: value`, `apiKey=...`, `token: ...`, `password=...`
  /\b(secret|api[-_ ]?key|token|password|passphrase|mnemonic|private[-_ ]?key)\b\s*[:=]\s*("[^"]*"|'[^']*'|\S+)/gi,
  // Bearer / Basic authorization headers
  /\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}/gi,
  // Raw 32-byte hex blobs (private keys, signatures)
  /\b0x[0-9a-f]{64,}\b/gi,
  // Long alphanumeric blobs that look like API tokens
  /\b[A-Za-z0-9_-]{40,}\b/g,
];

/**
 * Scrubs credential-looking substrings from free-form error text and truncates
 * it to a bounded length.
 */
export function sanitizeErrorMessage(
  error: unknown,
  maxLength: number = MAX_PERSISTED_ERROR_LENGTH,
): string {
  const raw =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : String(error);

  let scrubbed = raw;
  for (const pattern of SECRET_PATTERNS) {
    scrubbed = scrubbed.replace(pattern, "[redacted]");
  }

  // Collapse newlines so a multi-line upstream dump cannot forge log records.
  scrubbed = scrubbed.replace(/\s+/g, " ").trim();

  return scrubbed.length > maxLength
    ? `${scrubbed.slice(0, maxLength - 1)}…`
    : scrubbed;
}

/** Transfer fields safe to attach to a log line. */
export interface RedactedTransfer {
  id: string;
  status: string;
  stage: string;
  direction: string;
  sourceChain: string;
  destinationChain: string;
  amount: string;
  netAmount: string;
  sourceChainTxHash?: string;
  bridgeTxHash?: string;
  destinationTxHash?: string;
  attemptCount: number;
  retriesRemaining: number;
  createdAt: number;
  updatedAt: number;
  hasError: boolean;
}

/**
 * Projects a durable record onto the fields that are safe to log: identifiers,
 * amounts and workflow metadata. Free-form error text is reduced to a boolean
 * and the GMP payload is never included (it is not persisted at all).
 */
export function redactTransfer(
  transfer: StoredBridgeTransfer,
): RedactedTransfer {
  return {
    id: transfer.id,
    status: transfer.status,
    stage: transfer.stage,
    direction: transfer.direction,
    sourceChain: transfer.sourceChain,
    destinationChain: transfer.destinationChain,
    amount: transfer.amount.toString(),
    netAmount: transfer.netAmount.toString(),
    sourceChainTxHash: transfer.sourceChainTxHash,
    bridgeTxHash: transfer.bridgeTxHash,
    destinationTxHash: transfer.destinationTxHash,
    attemptCount: transfer.attemptCount,
    retriesRemaining: transfer.retriesRemaining,
    createdAt: transfer.createdAt,
    updatedAt: transfer.updatedAt,
    hasError: Boolean(transfer.lastError),
  };
}
