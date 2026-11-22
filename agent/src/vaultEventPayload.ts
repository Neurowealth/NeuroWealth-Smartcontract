/**
 * Decodes vault Deposit/Withdraw events (Issue #748) so alerts are evaluated
 * against the real on-chain amount instead of a hardcoded placeholder.
 *
 * Contract shape (EVENTS.md): topics `("deposit" | "withdraw", user)`,
 * value `{ user, amount: i128, shares: i128 }` with 7-decimal amounts.
 */

import { scValToNative, xdr } from '@stellar/stellar-sdk';

/** 1 USDC = 10_000_000 raw units. */
export const USDC_DECIMALS = 7;

export interface VaultTransferPayload {
  user: string | null;
  /** Amount in whole USDC (alert thresholds are expressed in USDC). */
  amount: number;
  /** Raw 7-decimal on-chain amount, kept for lossless logging. */
  rawAmount: bigint;
  shares: bigint | null;
}

function toBigInt(value: unknown): bigint | null {
  if (typeof value === 'bigint') return value;
  if (typeof value === 'number' && Number.isInteger(value)) return BigInt(value);
  if (typeof value === 'string' && /^-?\d+$/.test(value)) return BigInt(value);
  return null;
}

export function rawToUsdc(raw: bigint): number {
  const scale = BigInt(10) ** BigInt(USDC_DECIMALS);
  return Number(raw / scale) + Number(raw % scale) / Number(scale);
}

/** Converts an ScVal (or an already-native value) to a JS value. */
function toNative(value: unknown): unknown {
  if (value instanceof xdr.ScVal) return scValToNative(value);
  return value;
}

/** Returns 'deposit' / 'withdraw' from the event's first topic, else null. */
export function vaultEventType(topic: unknown[]): 'deposit' | 'withdraw' | null {
  if (topic.length === 0) return null;
  let first: unknown;
  try {
    first = toNative(topic[0]);
  } catch {
    return null;
  }
  return first === 'deposit' || first === 'withdraw' ? first : null;
}

/**
 * Extracts user and amount from a deposit/withdraw event. Returns null when
 * the payload does not carry a readable amount.
 */
export function decodeVaultTransfer(event: { topic: unknown[]; value: unknown }): VaultTransferPayload | null {
  let native: unknown;
  try {
    native = toNative(event.value);
  } catch {
    return null;
  }
  if (!native || typeof native !== 'object') return null;

  const data = native as Record<string, unknown>;
  const rawAmount = toBigInt(data.amount);
  if (rawAmount === null) return null;

  let user: string | null = typeof data.user === 'string' ? data.user : null;
  if (!user && event.topic.length > 1) {
    try {
      const topicUser = toNative(event.topic[1]);
      if (typeof topicUser === 'string') user = topicUser;
    } catch {
      // user stays null
    }
  }

  return {
    user,
    amount: rawToUsdc(rawAmount),
    rawAmount,
    shares: toBigInt(data.shares),
  };
}
