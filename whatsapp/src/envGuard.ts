/**
 * Boot-time environment guard for the WhatsApp service (#945).
 *
 * Mirrors scripts/check-no-production-defaults.py, which statically forbids
 * hardcoded fallbacks for these variables in CI; this module is the runtime
 * half and refuses to start when a variable is missing, a known placeholder,
 * or an unsafe default. All problems are reported together so an operator
 * can fix a bad deployment in one pass.
 */
import { loadKeyring, MIN_KEY_LENGTH } from './keyring';

const MIN_SALT_LENGTH = 16;

const PLACEHOLDER = /^(changeme|change[-_ ]?me|secret|password|passwd|default|insecure|placeholder|example|dummy|todo|your[-_ ].*|.*[-_ ]here|x{3,}|\*{3,}|<.*>)$/i;

function isPlaceholder(value: string): boolean {
  return PLACEHOLDER.test(value.trim()) || new Set(value).size < 8;
}

function missing(env: NodeJS.ProcessEnv, name: string): boolean {
  return env[name] === undefined || env[name]?.trim() === '';
}

function checkSecret(env: NodeJS.ProcessEnv, name: string, minLength: number, problems: string[]): void {
  if (missing(env, name)) {
    problems.push(`${name} must be set; refusing to use an unsafe default`);
    return;
  }
  const value = env[name] as string;
  if (value.length < minLength) {
    problems.push(`${name} must be at least ${minLength} characters`);
  } else if (isPlaceholder(value)) {
    problems.push(`${name} looks like a placeholder or low-entropy value; generate one with "openssl rand -hex 32"`);
  }
}

function checkPositiveInts(env: NodeJS.ProcessEnv, names: string[], problems: string[]): void {
  for (const name of names) {
    const raw = env[name];
    if (raw === undefined || raw.trim() === '') continue;
    if (!/^\d+$/.test(raw.trim()) || Number(raw) <= 0) {
      problems.push(`${name} must be a positive integer when set (got "${raw}")`);
    }
  }
}

/** Returns every problem found; an empty array means the environment is acceptable. */
export function validateEnv(env: NodeJS.ProcessEnv = process.env): string[] {
  const problems: string[] = [];
  const production = env.NODE_ENV === 'production';

  // Custody secrets.
  checkSecret(env, 'ENCRYPTION_KEY', MIN_KEY_LENGTH, problems);
  checkSecret(env, 'PHONE_HASH_SALT', MIN_SALT_LENGTH, problems);
  if (!missing(env, 'ENCRYPTION_KEY')) {
    try {
      loadKeyring(env);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      // Length problems for ENCRYPTION_KEY were already reported above.
      if (!message.startsWith('ENCRYPTION_KEY must be')) problems.push(message);
    }
  }

  // Database: no implicit localhost credentials.
  if (missing(env, 'DATABASE_URL')) {
    problems.push('DATABASE_URL must be set; refusing to use an unsafe database default');
  } else if (/\/\/postgres:postgres@/i.test(env.DATABASE_URL as string)) {
    problems.push('DATABASE_URL uses the default postgres:postgres credentials');
  }

  // Twilio signature validation must stay on unless explicitly disabled for local dev.
  const skipSignature = env.TWILIO_SKIP_SIGNATURE_VALIDATION === 'true';
  if (skipSignature && production) {
    problems.push('TWILIO_SKIP_SIGNATURE_VALIDATION=true is not allowed when NODE_ENV=production');
  } else if (!skipSignature) {
    checkSecret(env, 'TWILIO_AUTH_TOKEN', 16, problems);
  }

  // Vault wiring is read at module load by vaultRouter.
  for (const name of ['SOROBAN_RPC_URL', 'VAULT_CONTRACT_ID']) {
    if (missing(env, name)) problems.push(`${name} must be set; refusing to use an unsafe vault default`);
  }

  checkPositiveInts(
    env,
    [
      'WHATSAPP_OTP_TTL_MS',
      'WHATSAPP_OTP_MAX_ATTEMPTS',
      'WHATSAPP_SESSION_TIMEOUT_MS',
      'WHATSAPP_RATE_LIMIT_MAX_MESSAGES',
      'WHATSAPP_RATE_LIMIT_WINDOW_MS',
      'WHATSAPP_WEBHOOK_LEASE_MS',
    ],
    problems
  );

  return problems;
}

/** Throws one error listing every problem. Variable names only - values are never echoed. */
export function assertEnv(env: NodeJS.ProcessEnv = process.env): void {
  const problems = validateEnv(env);
  if (problems.length > 0) {
    throw new Error(
      `Refusing to start: insecure or incomplete WhatsApp service configuration\n${problems.map((p) => `  - ${p}`).join('\n')}`
    );
  }
}
