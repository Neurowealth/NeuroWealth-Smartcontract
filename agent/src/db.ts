import { Pool, PoolConfig } from 'pg';
import logger from './logger';

export type PoolLifecycleStatus = 'active' | 'closing' | 'closed';

const maxPoolSize = parseInt(process.env.MAX_DB_POOL_SIZE || '10', 10);
const minPoolSize = parseInt(process.env.MIN_DB_POOL_SIZE || '2', 10);
const idleTimeout = parseInt(process.env.DB_IDLE_TIMEOUT_MS || '30000', 10);
const connTimeout = parseInt(process.env.DB_CONN_TIMEOUT_MS || '5000', 10);

export function getDefaultPoolConfig(): PoolConfig {
  return {
    connectionString: process.env.DATABASE_URL,
    max: maxPoolSize,
    min: minPoolSize,
    idleTimeoutMillis: idleTimeout,
    connectionTimeoutMillis: connTimeout,
  };
}

let activePool: Pool = createPoolInstance(getDefaultPoolConfig());
let poolStatus: PoolLifecycleStatus = 'active';
let closePromise: Promise<void> | null = null;

function createPoolInstance(config: PoolConfig): Pool {
  const p = new Pool(config);

  p.on('error', (err) => {
    logger.error({ error: err.message }, 'Unexpected PostgreSQL connection pool error');
  });

  p.on('connect', () => {
    logger.debug('New client connected to PostgreSQL pool');
  });

  return p;
}

/**
 * Initializes or re-initializes the PostgreSQL connection pool.
 */
export function initPool(config?: PoolConfig): Pool {
  if (poolStatus === 'active') {
    // If a pool is already active, close it first without throwing
    activePool.end().catch(() => {});
  }
  activePool = createPoolInstance(config || getDefaultPoolConfig());
  poolStatus = 'active';
  closePromise = null;
  return activePool;
}

/**
 * Returns the currently active PostgreSQL connection pool instance.
 */
export function getPool(): Pool {
  return activePool;
}

/**
 * Checks whether the pool is in the process of closing.
 */
export function isPoolClosing(): boolean {
  return poolStatus === 'closing';
}

/**
 * Checks whether the pool is fully closed.
 */
export function isPoolClosed(): boolean {
  return poolStatus === 'closed';
}

/**
 * Returns the current lifecycle status of the pool.
 */
export function getPoolStatus(): PoolLifecycleStatus {
  return poolStatus;
}

/**
 * Gracefully shuts down and drains the PostgreSQL pool with an optional timeout.
 * Idempotent: safe to call multiple times concurrently.
 */
export function closePool(timeoutMs = 5000): Promise<void> {
  if (poolStatus === 'closed') {
    return Promise.resolve();
  }
  if (closePromise) {
    return closePromise;
  }

  poolStatus = 'closing';
  logger.info({ timeoutMs }, 'Draining and closing PostgreSQL connection pool');

  closePromise = (async () => {
    let timer: NodeJS.Timeout | null = null;
    const timeoutPromise = new Promise<void>((resolve) => {
      timer = setTimeout(() => {
        logger.warn({ timeoutMs }, 'PostgreSQL pool drain timed out; forcing shutdown');
        resolve();
      }, timeoutMs);
    });

    const endPromise = (async () => {
      try {
        await activePool.end();
        logger.info('PostgreSQL connection pool closed successfully');
      } catch (err: unknown) {
        logger.error({ error: err instanceof Error ? err.message : String(err) }, 'Error during PostgreSQL pool closure');
      }
    })();

    try {
      await Promise.race([endPromise, timeoutPromise]);
    } finally {
      if (timer) clearTimeout(timer);
      poolStatus = 'closed';
    }
  })();

  return closePromise;
}

/**
 * Exported pool proxy maintaining full backwards compatibility with all imports.
 */
export const pool = new Proxy({} as Pool, {
  get(_target, prop) {
    const val = Reflect.get(activePool, prop);
    if (typeof val === 'function') {
      return (val as (...args: unknown[]) => unknown).bind(activePool);
    }
    return val;
  },
});

export function getPoolMetrics() {
  if (poolStatus === 'closed') {
    return {
      totalCount: 0,
      idleCount: 0,
      waitingCount: 0,
      maxPoolSize,
      minPoolSize,
      status: poolStatus,
    };
  }

  return {
    totalCount: activePool.totalCount,
    idleCount: activePool.idleCount,
    waitingCount: activePool.waitingCount,
    maxPoolSize,
    minPoolSize,
    status: poolStatus,
  };
}
