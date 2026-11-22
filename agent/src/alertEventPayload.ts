export const ALERT_EVENT_TYPES = ['deposit', 'withdraw', 'auth_failure'] as const;

export type AlertEventType = (typeof ALERT_EVENT_TYPES)[number];

/** Canonical event shape shared by ingestion and alert evaluation. */
export interface AlertEventPayload {
  type: AlertEventType;
  amount: number;
  user: string | null;
  ledger: number;
}

export type AlertEventParseResult =
  | { ok: true; payload: AlertEventPayload }
  | { ok: false; reason: string };

export function parseAlertEventPayload(value: unknown): AlertEventParseResult {
  if (!value || typeof value !== 'object') {
    return { ok: false, reason: 'alert event payload must be an object' };
  }

  const candidate = value as Record<string, unknown>;
  if (typeof candidate.type !== 'string' || candidate.type.length === 0) {
    return { ok: false, reason: 'alert event payload requires a type' };
  }
  if (!(ALERT_EVENT_TYPES as readonly string[]).includes(candidate.type)) {
    return { ok: false, reason: `unknown alert event type: ${candidate.type}` };
  }
  if (typeof candidate.amount !== 'number' || !Number.isFinite(candidate.amount)) {
    return { ok: false, reason: 'alert event payload requires a finite amount' };
  }
  if (candidate.user !== null && typeof candidate.user !== 'string') {
    return { ok: false, reason: 'alert event payload user must be a string or null' };
  }
  if (!Number.isSafeInteger(candidate.ledger) || (candidate.ledger as number) < 0) {
    return { ok: false, reason: 'alert event payload requires a non-negative ledger' };
  }

  return {
    ok: true,
    payload: {
      type: candidate.type as AlertEventType,
      amount: candidate.amount,
      user: candidate.user,
      ledger: candidate.ledger as number,
    },
  };
}
