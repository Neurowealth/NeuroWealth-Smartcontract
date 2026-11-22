import { ParsedIntent } from './intentParser';

/**
 * Strategy symbols accepted by the vault's `set_user_strategy`.
 * Mirrors the `InvalidStrategy` check in neurowealth-vault/contracts/vault/src/lib.rs.
 */
export const VALID_STRATEGIES = ['conservative', 'balanced', 'growth'] as const;
export type Strategy = (typeof VALID_STRATEGIES)[number];

/** USDC on Stellar uses 7 decimal places (1 USDC = 10_000_000 stroops). */
export const USDC_DECIMALS = 7;
const STROOPS_PER_USDC = 10 ** USDC_DECIMALS;

/**
 * Contract defaults written by `initialize` (all values in USDC stroops).
 * Keep in sync with DEFAULT_MIN_DEPOSIT / DEFAULT_MAX_DEPOSIT /
 * DEFAULT_USER_DEPOSIT_CAP / DEFAULT_TVL_CAP in lib.rs.
 */
export const CONTRACT_DEFAULT_LIMITS_STROOPS = {
  minDeposit: 1_000_000,          // 0.1 USDC
  maxDeposit: 10_000_000_000,     // 1,000 USDC
  userDepositCap: 10_000_000_000, // 1,000 USDC
  tvlCap: 100_000_000_000,        // 10,000 USDC
};

/** Deposit limits in whole USDC, as enforced by the vault contract. */
export interface ContractLimits {
  minDeposit: number;
  maxDeposit: number;
  userDepositCap: number;
  tvlCap: number;
}

function readStroopsEnv(name: string, fallback: number): number {
  const raw = process.env[name];
  if (raw === undefined || raw.trim() === '') return fallback;
  const value = Number(raw);
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${name} must be a positive integer amount in stroops (got "${raw}")`);
  }
  return value;
}

/**
 * Returns the vault's deposit limits in USDC.
 *
 * Defaults mirror the contract's `initialize` values. When the owner changes
 * limits on-chain (`set_deposit_limits`, `set_user_deposit_cap`,
 * `set_tvl_cap`), set the matching env vars (in stroops, the same unit as the
 * contract getters `get_min_deposit`, `get_max_deposit`,
 * `get_user_deposit_cap`, `get_tvl_cap`) so the bot rejects out-of-range
 * requests before submitting a transaction.
 */
export function getContractLimits(): ContractLimits {
  const d = CONTRACT_DEFAULT_LIMITS_STROOPS;
  return {
    minDeposit: readStroopsEnv('VAULT_MIN_DEPOSIT', d.minDeposit) / STROOPS_PER_USDC,
    maxDeposit: readStroopsEnv('VAULT_MAX_DEPOSIT', d.maxDeposit) / STROOPS_PER_USDC,
    userDepositCap: readStroopsEnv('VAULT_USER_DEPOSIT_CAP', d.userDepositCap) / STROOPS_PER_USDC,
    tvlCap: readStroopsEnv('VAULT_TVL_CAP', d.tvlCap) / STROOPS_PER_USDC,
  };
}

export function isValidStrategy(name: string | undefined): name is Strategy {
  return name !== undefined && (VALID_STRATEGIES as readonly string[]).includes(name);
}

export type ValidationResult = { ok: true } | { ok: false; error: string };

function formatUsdc(value: number): string {
  return value.toLocaleString('en-US', { maximumFractionDigits: USDC_DECIMALS });
}

function validateAmount(amount: number | undefined, action: 'deposit' | 'withdraw'): ValidationResult {
  if (amount === undefined) {
    return { ok: false, error: `Please include an amount, e.g. "${action} 50 USDC".` };
  }
  if (!Number.isFinite(amount) || amount <= 0) {
    return { ok: false, error: `The ${action} amount must be greater than 0 USDC.` };
  }
  // Reject precision the contract cannot represent (sub-stroop amounts).
  const stroops = amount * STROOPS_PER_USDC;
  if (Math.abs(stroops - Math.round(stroops)) > 1e-6) {
    return { ok: false, error: `USDC supports at most ${USDC_DECIMALS} decimal places.` };
  }
  return { ok: true };
}

/**
 * Validates a parsed intent against the vault contract's caps and accepted
 * strategy names so invalid requests are rejected in chat instead of failing
 * on-chain. Intents that carry no amount or strategy always pass.
 */
export function validateIntent(
  intent: ParsedIntent,
  limits: ContractLimits = getContractLimits()
): ValidationResult {
  switch (intent.type) {
    case 'DEPOSIT': {
      const amountCheck = validateAmount(intent.amount, 'deposit');
      if (!amountCheck.ok) return amountCheck;
      const amount = intent.amount as number;
      if (amount < limits.minDeposit) {
        return { ok: false, error: `Minimum deposit is ${formatUsdc(limits.minDeposit)} USDC.` };
      }
      if (amount > limits.maxDeposit) {
        return { ok: false, error: `Maximum single deposit is ${formatUsdc(limits.maxDeposit)} USDC.` };
      }
      if (amount > limits.userDepositCap) {
        return { ok: false, error: `Deposits are capped at ${formatUsdc(limits.userDepositCap)} USDC per user.` };
      }
      if (amount > limits.tvlCap) {
        return { ok: false, error: `This exceeds the vault's total deposit cap of ${formatUsdc(limits.tvlCap)} USDC.` };
      }
      if (intent.strategy !== undefined && !isValidStrategy(intent.strategy)) {
        return { ok: false, error: invalidStrategyMessage(intent.strategy) };
      }
      return { ok: true };
    }
    case 'WITHDRAW': {
      if (intent.withdrawAll) return { ok: true };
      return validateAmount(intent.amount, 'withdraw');
    }
    case 'STRATEGY': {
      if (!isValidStrategy(intent.strategy)) {
        return { ok: false, error: invalidStrategyMessage(intent.strategy) };
      }
      return { ok: true };
    }
    default:
      return { ok: true };
  }
}

function invalidStrategyMessage(name: string | undefined): string {
  const options = VALID_STRATEGIES.map((s) => `"${s}"`).join(', ');
  return name
    ? `"${name}" is not a valid strategy. Choose one of ${options}.`
    : `Please name a strategy: ${options}.`;
}
