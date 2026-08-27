import express from 'express';
import { Pool } from 'pg';
import { SorobanRpc } from '@stellar/stellar-sdk';
import { getPoolMetrics } from './db';
import { openAiKeyManager } from './openAiKeyManager';

const router = express.Router();
let dbPool: Pool | null = null;
let rpcServer: SorobanRpc.Server | null = null;

export function configureHealthChecks(pool: Pool, server: SorobanRpc.Server) {
  dbPool = pool;
  rpcServer = server;
}

router.get('/health', async (_req, res) => {
  const checks: Record<string, string> = {};

  // Database check
  try {
    if (dbPool) {
      await dbPool.query('SELECT 1');
      checks.database = 'ok';
    } else {
      checks.database = 'not_configured';
    }
  } catch {
    checks.database = 'error';
  }

  // Stellar RPC check
  try {
    if (rpcServer) {
      await rpcServer.getLatestLedger();
      checks.stellar_rpc = 'ok';
    } else {
      checks.stellar_rpc = 'not_configured';
    }
  } catch {
    checks.stellar_rpc = 'error';
  }

  // OpenAI keys check
  const keyHealth = openAiKeyManager.getHealthStatus();
  const healthyKeysCount = keyHealth.filter((k) => k.isHealthy).length;
  if (keyHealth.length > 0) {
    checks.openai_api_keys = healthyKeysCount > 0 ? 'ok' : 'degraded';
  } else {
    checks.openai_api_keys = 'not_configured';
  }

  const isHealthy = Object.values(checks).every((s) => s === 'ok' || s === 'not_configured');
  const status = isHealthy ? 200 : 503;

  res.status(status).json({
    status: isHealthy ? 'ok' : 'degraded',
    checks,
    metrics: {
      dbPool: getPoolMetrics(),
      openAiKeys: {
        totalConfigured: openAiKeyManager.keyCount,
        healthyCount: healthyKeysCount,
        keys: keyHealth,
      },
    },
    uptime: process.uptime(),
    timestamp: new Date().toISOString(),
  });
});

router.get('/ready', async (_req, res) => {
  try {
    if (rpcServer) {
      await rpcServer.getLatestLedger();
    }
    res.json({ ready: true });
  } catch {
    res.status(503).json({ ready: false, error: 'Stellar RPC unreachable' });
  }
});

export default router;
