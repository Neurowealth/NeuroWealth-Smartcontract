/**
 * Error Sanitization Utility
 *
 * Ensures errors rendered in user-facing fallbacks or sent to observability
 * never leak raw server responses, secrets (Stellar secret keys, tokens, passwords),
 * file paths, or production stack traces.
 */

// Matches Stellar secret seed keys (starts with S, followed by 55 base32 chars)
const STELLAR_SECRET_KEY_REGEX = /\bS[A-Z2-7]{55}\b/g;

// Matches Bearer tokens, auth tokens, passwords, and API key assignments
const CREDENTIAL_PATTERN_REGEX =
  /(bearer\s+[A-Za-z0-9_\-\.]+)|((?:token|apikey|api_key|password|secret|authorization|auth)[=:\s]+["']?[A-Za-z0-9_\-\.]{8,}["']?)/gi;

// Matches sensitive URL query parameters
const SENSITIVE_QUERY_PARAM_REGEX = /[?&](?:token|key|secret|password|auth|api_key)=[^&\s]+/gi;

// Matches local filesystem absolute paths or webpack internal paths
const FILE_PATH_REGEX = /(?:\/[\w\.\-]+)+(?:\.[\w]+)?(?::\d+:\d+)?/g;

export interface SanitizedErrorInfo {
  title: string;
  message: string;
  digest?: string;
  sanitizedStack?: string;
  isFatal?: boolean;
}

/**
 * Strips all known secret and credential patterns from an arbitrary string.
 */
export function redactSensitiveData(raw: string): string {
  if (!raw || typeof raw !== 'string') return '';

  return raw
    .replace(STELLAR_SECRET_KEY_REGEX, '[REDACTED_SECRET_KEY]')
    .replace(SENSITIVE_QUERY_PARAM_REGEX, '[REDACTED_PARAM]')
    .replace(CREDENTIAL_PATTERN_REGEX, '[REDACTED_CREDENTIAL]');
}

/**
 * Extracts and sanitizes an error message from any error input.
 */
export function sanitizeErrorMessage(error: unknown): string {
  if (!error) return 'An unexpected error occurred.';

  let rawMessage = '';

  if (error instanceof Error) {
    rawMessage = error.message;
  } else if (typeof error === 'string') {
    rawMessage = error;
  } else if (typeof error === 'object') {
    try {
      const obj = error as Record<string, unknown>;
      if (typeof obj.message === 'string') {
        rawMessage = obj.message;
      } else {
        rawMessage = JSON.stringify(error);
      }
    } catch {
      rawMessage = String(error);
    }
  } else {
    rawMessage = String(error);
  }

  // Redact any secrets immediately
  let sanitized = redactSensitiveData(rawMessage);

  // In production, prevent leaking internal file paths or raw stack dumps
  const isProd = process.env.NODE_ENV === 'production';
  if (isProd) {
    // If it looks like an unhandled internal exception with path references or raw JSON RPC errors
    if (sanitized.includes('webpack-internal') || sanitized.includes('node_modules') || FILE_PATH_REGEX.test(sanitized)) {
      return 'An unexpected system error occurred. Please try again.';
    }
  }

  return sanitized.trim() || 'An unexpected error occurred.';
}

/**
 * Returns comprehensive sanitized error information suitable for display and logging.
 */
export function getSanitizedErrorInfo(error: unknown, surface?: string): SanitizedErrorInfo {
  const message = sanitizeErrorMessage(error);
  const isProd = process.env.NODE_ENV === 'production';

  const digest =
    error && typeof error === 'object' && 'digest' in error && typeof (error as any).digest === 'string'
      ? (error as any).digest
      : undefined;

  let sanitizedStack: string | undefined = undefined;

  if (!isProd && error instanceof Error && error.stack) {
    sanitizedStack = redactSensitiveData(error.stack);
  }

  let title = 'Something went wrong';
  if (surface === 'portfolio') {
    title = 'Unable to load portfolio data';
  } else if (surface === 'transactions') {
    title = 'Unable to load transactions';
  } else if (surface === 'strategies') {
    title = 'Unable to load strategy chart';
  } else if (surface === 'transaction-modal') {
    title = 'Transaction Action Error';
  }

  return {
    title,
    message,
    digest,
    sanitizedStack,
    isFatal: false,
  };
}
