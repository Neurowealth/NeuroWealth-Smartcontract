import logger from './logger';

export interface RetryOptions {
  maxRetries?: number;
  baseDelayMs?: number;
  maxDelayMs?: number;
}

const DEFAULT_OPTIONS: Required<RetryOptions> = {
  maxRetries: parseInt(process.env.MAX_RETRIES || '3', 10),
  baseDelayMs: 1000,
  maxDelayMs: 30000,
};

export async function withRetry<T>(
  fn: () => Promise<T>,
  label: string,
  options: RetryOptions = {},
): Promise<T> {
  const opts = { ...DEFAULT_OPTIONS, ...options };
  let lastError: Error | undefined;

  for (let attempt = 0; attempt <= opts.maxRetries; attempt++) {
    try {
      return await fn();
    } catch (err) {
      lastError = err instanceof Error ? err : new Error(String(err));
      if (attempt === opts.maxRetries) break;

      const delay = Math.min(
        opts.baseDelayMs * Math.pow(2, attempt) + Math.random() * 500,
        opts.maxDelayMs,
      );
      logger.warn({ attempt: attempt + 1, maxRetries: opts.maxRetries, delayMs: Math.round(delay), label, error: lastError.message }, 'Retryable error, retrying...');
      await new Promise((r) => setTimeout(r, delay));
    }
  }

  logger.error({ label, error: lastError?.message }, 'All retries exhausted');
  throw lastError;
}
