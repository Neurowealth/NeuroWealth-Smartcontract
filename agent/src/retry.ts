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

export interface DatabaseRetryMetrics {
  attempts: number;
  retries: number;
  recovered: number;
  permanentFailures: number;
  exhausted: number;
  deadlineExceeded: number;
}

export interface DatabaseRetryOptions {
  maxRetries?: number;
  baseDelayMs?: number;
  maxDelayMs?: number;
  totalDeadlineMs?: number;
  random?: () => number;
  now?: () => number;
  sleep?: (delayMs: number) => Promise<void>;
}

const DEFAULT_DATABASE_OPTIONS: Required<DatabaseRetryOptions> = {
  maxRetries: Number.parseInt(process.env.DB_RETRY_MAX_RETRIES || '3', 10),
  baseDelayMs: Number.parseInt(process.env.DB_RETRY_BASE_DELAY_MS || '100', 10),
  maxDelayMs: Number.parseInt(process.env.DB_RETRY_MAX_DELAY_MS || '2000', 10),
  totalDeadlineMs: Number.parseInt(process.env.DB_TRANSACTION_DEADLINE_MS || '10000', 10),
  random: Math.random,
  now: Date.now,
  sleep: (delayMs) => new Promise((resolve) => setTimeout(resolve, delayMs)),
};

const databaseRetryMetrics: DatabaseRetryMetrics = {
  attempts: 0,
  retries: 0,
  recovered: 0,
  permanentFailures: 0,
  exhausted: 0,
  deadlineExceeded: 0,
};

export class DatabaseRetryDeadlineError extends Error {
  constructor(label: string) {
    super(`Database operation exceeded its total deadline: ${label}`);
    this.name = 'DatabaseRetryDeadlineError';
  }
}

export function isRetryableDatabaseError(error: unknown): boolean {
  if (!error || typeof error !== 'object') return false;
  const candidate = error as { code?: unknown; cause?: unknown };
  const code = typeof candidate.code === 'string' ? candidate.code : '';
  if (
    code === '40001' ||
    code === '40P01' ||
    code.startsWith('08') ||
    ['57P01', '57P02', '57P03', 'ECONNRESET', 'ECONNREFUSED', 'ETIMEDOUT', 'EPIPE'].includes(code)
  ) return true;
  return candidate.cause !== undefined && isRetryableDatabaseError(candidate.cause);
}

export function getDatabaseRetryMetrics(): DatabaseRetryMetrics {
  return { ...databaseRetryMetrics };
}

export function resetDatabaseRetryMetrics(): void {
  Object.assign(databaseRetryMetrics, {
    attempts: 0,
    retries: 0,
    recovered: 0,
    permanentFailures: 0,
    exhausted: 0,
    deadlineExceeded: 0,
  });
}

export async function withDatabaseRetry<T>(
  operation: () => Promise<T>,
  label: string,
  options: DatabaseRetryOptions = {},
): Promise<T> {
  const opts = { ...DEFAULT_DATABASE_OPTIONS, ...options };
  const startedAt = opts.now();
  const deadlineAt = startedAt + opts.totalDeadlineMs;
  let retries = 0;
  let attempts = 0;

  while (true) {
    const remainingMs = deadlineAt - opts.now();
    if (remainingMs <= 0) {
      databaseRetryMetrics.deadlineExceeded += 1;
      const deadlineError = new DatabaseRetryDeadlineError(label);
      logger.error({ label, attempts, elapsedMs: opts.now() - startedAt }, 'Database operation deadline exceeded');
      throw deadlineError;
    }

    attempts += 1;
    databaseRetryMetrics.attempts += 1;
    const operationPromise = operation();
    // The deadline race can reject while the operation is still in flight and
    // the operation may still commit afterwards. Attach a handler now so its
    // eventual rejection is never unhandled; the timeout path also awaits it.
    void operationPromise.catch(() => undefined);
    try {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const result = await Promise.race([
        operationPromise,
        new Promise<never>((_resolve, reject) => {
          timer = setTimeout(() => reject(new DatabaseRetryDeadlineError(label)), remainingMs);
        }),
      ]).finally(() => {
        if (timer) clearTimeout(timer);
      });
      if (retries > 0) databaseRetryMetrics.recovered += 1;
      return result;
    } catch (error) {
      const elapsedMs = opts.now() - startedAt;
      if (error instanceof DatabaseRetryDeadlineError || elapsedMs >= opts.totalDeadlineMs) {
        if (error instanceof DatabaseRetryDeadlineError) {
          // Await the in-flight operation before propagating the timeout: the
          // DB work can still commit after the deadline fires, so let it
          // settle so its outcome is observed rather than leaving a wedged
          // row that nothing will ever reconcile. Rows that stay wedged
          // across a crash are swept by the stale-operation sweep in
          // rebalanceOperations.ts.
          await operationPromise.catch(() => undefined);
        }
        databaseRetryMetrics.deadlineExceeded += 1;
        logger.error({ label, attempts, elapsedMs, error }, 'Database operation deadline exceeded');
        throw error instanceof DatabaseRetryDeadlineError ? error : new DatabaseRetryDeadlineError(label);
      }
      if (!isRetryableDatabaseError(error)) {
        databaseRetryMetrics.permanentFailures += 1;
        logger.error({ label, attempts, elapsedMs, error }, 'Permanent database operation failure');
        throw error;
      }
      if (retries >= opts.maxRetries) {
        databaseRetryMetrics.exhausted += 1;
        logger.error({ label, attempts, elapsedMs, error }, 'Database operation retries exhausted');
        throw error;
      }

      const exponentialDelay = Math.min(opts.baseDelayMs * 2 ** retries, opts.maxDelayMs);
      const delayMs = Math.round(exponentialDelay * (0.5 + opts.random()));
      const boundedDelay = Math.min(delayMs, Math.max(0, deadlineAt - opts.now()));
      databaseRetryMetrics.retries += 1;
      logger.warn({ label, attempt: retries + 1, maxRetries: opts.maxRetries, delayMs: boundedDelay, elapsedMs, error }, 'Transient database failure; retrying');
      retries += 1;
      await opts.sleep(boundedDelay);
    }
  }
}

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
