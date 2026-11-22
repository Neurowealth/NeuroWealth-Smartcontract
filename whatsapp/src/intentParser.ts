export type IntentType =
  | 'GREETING'
  | 'OTP_CODE'
  | 'DEPOSIT'
  | 'WITHDRAW'
  | 'BALANCE'
  | 'EARNINGS'
  | 'STRATEGY'
  | 'APY'
  | 'UNKNOWN';

export interface ParsedIntent {
  type: IntentType;
  amount?: number;
  withdrawAll?: boolean;
  /**
   * Strategy name as typed by the user (lower-cased). Not guaranteed to be a
   * valid vault strategy; run `validateIntent` before acting on it.
   */
  strategy?: string;
  otpCode?: string;
  rawText: string;
}

const KNOWN_STRATEGIES = ['conservative', 'balanced', 'growth'];

/**
 * Extracts a signed decimal amount, tolerating thousands separators
 * ("1,000.50"). Returns undefined when no amount is present.
 */
function parseAmount(clean: string): number | undefined {
  const match = clean.match(/(-?\d[\d,]*(?:\.\d+)?)/);
  if (!match) return undefined;
  const value = parseFloat(match[1].replace(/,/g, ''));
  return Number.isNaN(value) ? undefined : value;
}

function findKnownStrategy(clean: string): string | undefined {
  return KNOWN_STRATEGIES.find((s) => clean.includes(s));
}

/**
 * Parses user natural language message into structured intent object.
 */
export function parseIntent(message: string): ParsedIntent {
  const clean = message.trim().toLowerCase();
  const rawText = message.trim();

  // Check 6-digit OTP code pattern
  if (/^\d{6}$/.test(clean)) {
    return { type: 'OTP_CODE', otpCode: clean, rawText };
  }

  // Greeting
  if (/^(hi|hello|hey|start|menu|help)$/.test(clean)) {
    return { type: 'GREETING', rawText };
  }

  // Balance query
  if (clean.includes('balance') || clean.includes('how much do i have') || clean.includes('my funds')) {
    return { type: 'BALANCE', rawText };
  }

  // Earnings query
  if (clean.includes('earnings') || clean.includes('how much have i made') || clean.includes('profit') || clean.includes('yield')) {
    return { type: 'EARNINGS', rawText };
  }

  // APY query
  // Word-boundary match: a bare includes('rate') would also match "strategy".
  if (/\b(apy|rates?|interest rate)\b/.test(clean)) {
    return { type: 'APY', rawText };
  }

  // Strategy change
  if (clean.includes('switch to') || clean.includes('change strategy')) {
    const known = findKnownStrategy(clean);
    if (known) return { type: 'STRATEGY', strategy: known, rawText };
    // Surface the unrecognised name so the validator can reject it explicitly.
    const named = clean.match(/(?:switch to|change strategy(?: to)?)\s+([a-z]+)/);
    return { type: 'STRATEGY', strategy: named ? named[1] : undefined, rawText };
  }

  // Deposit intent
  if (clean.startsWith('deposit') || clean.includes('add money') || clean.includes('put in')) {
    const amount = parseAmount(clean);
    const strategy = findKnownStrategy(clean);
    return { type: 'DEPOSIT', amount, strategy, rawText };
  }

  // Withdraw intent
  if (clean.startsWith('withdraw') || clean.includes('take out') || clean.includes('cash out')) {
    if (clean.includes('all') || clean.includes('everything')) {
      return { type: 'WITHDRAW', withdrawAll: true, rawText };
    }
    const amount = parseAmount(clean);
    return { type: 'WITHDRAW', amount, withdrawAll: false, rawText };
  }

  return { type: 'UNKNOWN', rawText };
}
