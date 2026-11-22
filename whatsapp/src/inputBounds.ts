/**
 * Bounds for untrusted WhatsApp and provider input (#876).
 * Lengths are UTF-8 bytes. A value of exactly the limit is accepted.
 */

export const INPUT_LIMITS = {
  inboundBody: 1024,
  commandText: 256,
  strategy: 32,
  address: 56,
  providerCallback: 128,
  outboundTemplate: 1024,
} as const;

export const SAFE_OVERSIZE_REPLY = 'Message is too long. Send a shorter command.';
export const SAFE_MALFORMED_REPLY = 'That message could not be read. Please send it again as plain text.';
export const SAFE_STRATEGY_REPLY = 'That strategy name is too long. Use conservative, balanced, or growth.';
export const SAFE_OUTBOUND_REPLY = 'I could not send that reply. Please try the command again.';

export type BoundReason = 'oversize' | 'malformed' | 'type';

export type BoundResult =
  | { ok: true; value: string }
  | { ok: false; reason: BoundReason };

export function utf8ByteLength(value: string): number {
  return Buffer.byteLength(value, 'utf8');
}

export function isWellFormedText(value: string): boolean {
  if (value.includes('\0')) return false;
  for (let i = 0; i < value.length; i++) {
    const code = value.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      const next = value.charCodeAt(i + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return false;
      i += 1;
    } else if (code >= 0xdc00 && code <= 0xdfff) {
      return false;
    }
  }
  return true;
}

export function boundText(value: unknown, maxBytes: number): BoundResult {
  if (typeof value !== 'string') return { ok: false, reason: 'type' };
  if (!isWellFormedText(value)) return { ok: false, reason: 'malformed' };
  if (utf8ByteLength(value) > maxBytes) return { ok: false, reason: 'oversize' };
  return { ok: true, value };
}

/** Decodes a raw UTF-8 buffer. Invalid sequences and overlong encodings are rejected. */
export function decodeUtf8(bytes: Uint8Array, maxBytes: number): BoundResult {
  if (bytes.byteLength > maxBytes) return { ok: false, reason: 'oversize' };
  try {
    const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    return boundText(text, maxBytes);
  } catch {
    return { ok: false, reason: 'malformed' };
  }
}

/**
 * Decodes one application/x-www-form-urlencoded component.
 * Invalid percent-encoding is rejected before the decoded text is bounded.
 */
export function decodeFormComponent(raw: string, maxBytes: number): BoundResult {
  if (typeof raw !== 'string' || !isWellFormedText(raw)) return { ok: false, reason: 'malformed' };
  if (/%(?![0-9A-Fa-f]{2})/.test(raw)) return { ok: false, reason: 'malformed' };
  try {
    return boundText(decodeURIComponent(raw.replace(/\+/g, ' ')), maxBytes);
  } catch {
    return { ok: false, reason: 'malformed' };
  }
}

export function boundOutbound(template: string): string {
  const checked = boundText(template, INPUT_LIMITS.outboundTemplate);
  return checked.ok ? checked.value : SAFE_OUTBOUND_REPLY;
}

export interface ProviderCallbackFields {
  messageSid: string;
  status: string;
  to?: string;
  errorCode?: string;
  errorMessage?: string;
}

export type ProviderBound =
  | { ok: true; value: ProviderCallbackFields }
  | { ok: false; field: string; reason: BoundReason };

function boundPresent(value: unknown, maxBytes: number): BoundResult | { ok: true; value: undefined } {
  if (value === undefined || value === null) return { ok: true, value: undefined };
  return boundText(value, maxBytes);
}

/** Bounds every provider callback field. Empty strings are allowed; missing optionals stay unset. */
export function boundProviderCallback(input: {
  messageSid: unknown;
  status: unknown;
  to?: unknown;
  errorCode?: unknown;
  errorMessage?: unknown;
}): ProviderBound {
  const messageSid = boundText(input.messageSid ?? '', INPUT_LIMITS.providerCallback);
  if (!messageSid.ok) return { ok: false, field: 'messageSid', reason: messageSid.reason };

  const status = boundText(input.status ?? '', INPUT_LIMITS.providerCallback);
  if (!status.ok) return { ok: false, field: 'status', reason: status.reason };

  const optional: Array<['to' | 'errorCode' | 'errorMessage', unknown]> = [
    ['to', input.to],
    ['errorCode', input.errorCode],
    ['errorMessage', input.errorMessage],
  ];
  const value: ProviderCallbackFields = { messageSid: messageSid.value, status: status.value };

  for (const [field, raw] of optional) {
    const checked = boundPresent(raw, INPUT_LIMITS.providerCallback);
    if (!checked.ok) return { ok: false, field, reason: checked.reason };
    if (checked.value !== undefined) value[field] = checked.value;
  }

  return { ok: true, value };
}
