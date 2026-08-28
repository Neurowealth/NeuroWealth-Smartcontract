import 'dotenv/config';
import express from 'express';
import { startEventListener, stopEventListener, server, pool } from './eventListener';
import { evaluateYield } from './yieldComparison';
import healthRouter, { configureHealthChecks } from './health';
import logger from './logger';
import { initializeTracing } from './tracing';

import { ipRateLimiter, userRateLimiter } from './rateLimiter';

// Initialize OpenTelemetry tracing
initializeTracing();

const app = express();
const PORT = parseInt(process.env.PORT || '3001', 10);

app.use(express.json());
app.use(ipRateLimiter);
app.use(userRateLimiter);
app.use(healthRouter);

let decisionInterval: ReturnType<typeof setInterval> | null = null;

function startDecisionLoop() {
  logger.info('Initializing hourly decision loop');

  decisionInterval = setInterval(async () => {
    try {
      logger.info('Running hourly yield evaluation');
      const decision = await evaluateYield('balanced', 'blend', 6.5);

      if (decision.shouldRebalance) {
        logger.info({ targetProtocol: decision.targetProtocol }, 'Rebalance needed');
      } else {
        logger.info('Yield is optimal, no action needed');
      }
    } catch (error) {
      logger.error({ error: error instanceof Error ? error.message : error }, 'Decision loop error');
    }
  }, 60 * 60 * 1000);
}

async function main() {
  logger.info('Starting NeuroWealth AI Agent');

  configureHealthChecks(pool, server);
  await startEventListener();
  startDecisionLoop();

  const serverInstance = app.listen(PORT, () => {
    logger.info({ port: PORT }, 'Agent HTTP server listening');
  });

  // Graceful shutdown
  async function shutdown(signal: string) {
    logger.info(`${signal} received, shutting down`);

    if (decisionInterval) {
      clearInterval(decisionInterval);
      decisionInterval = null;
    }

    stopEventListener();

    serverInstance.close(() => {
      logger.info('HTTP server closed');
    });

    try {
      await pool.end();
      logger.info('Database pool closed');
    } catch {
      // ignore
    }

    process.exit(0);
  }

  process.on('SIGTERM', () => shutdown('SIGTERM'));
  process.on('SIGINT', () => shutdown('SIGINT'));
}

main().catch((err) => {
  logger.fatal({ error: err.message }, 'Startup failed');
  process.exit(1);
});
