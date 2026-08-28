import { Pool, PoolConfig } from 'pg';
import logger from './logger';

const maxPoolSize = parseInt(process.env.MAX_DB_POOL_SIZE || '10', 10);
const minPoolSize = parseInt(process.env.MIN_DB_POOL_SIZE || '2', 10);
const idleTimeout = parseInt(process.env.DB_IDLE_TIMEOUT_MS || '30000', 10);
const connTimeout = parseInt(process.env.DB_CONN_TIMEOUT_MS || '5000', 10);

const poolConfig: PoolConfig = {
  connectionString: process.env.DATABASE_URL,
  max: maxPoolSize,
  min: minPoolSize,
  idleTimeoutMillis: idleTimeout,
  connectionTimeoutMillis: connTimeout,
};

export const pool = new Pool(poolConfig);

// Pool error handling & monitoring
pool.on('error', (err) => {
  logger.error({ error: err.message }, 'Unexpected PostgreSQL connection pool error');
});

pool.on('connect', () => {
  logger.debug('New client connected to PostgreSQL pool');
});

export function getPoolMetrics() {
  return {
    totalCount: pool.totalCount,
    idleCount: pool.idleCount,
    waitingCount: pool.waitingCount,
    maxPoolSize,
    minPoolSize,
  };
}
