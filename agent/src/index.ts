import 'dotenv/config';
import express from 'express';
import { startEventListener, stopEventListener, server, pool } from './eventListener';
import { closePool } from './db';
import { evaluateYield } from './yieldComparison';
import { runRebalanceCycle } from './userStrategies';
import healthRouter, { configureHealthChecks } from './health';
import logger from './logger';
import { initializeTracing } from './tracing';
import { submitAutoCompoundTx, submitRebalanceTx } from './sorobanTx';
import { getCurrentAllocation } from './userStrategies';

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

/**
 * Invokes the vault contract's `auto_compound(min_out)` function to harvest
 * accrued yield and immediately reinvest it in the same protocol, maximizing
 * compound growth without user intervention.
 *
 * @param minOut - Minimum amount of yield that must be compounded; reverts if
 *                 the available yield is below this threshold.
 */
async function autoCompound(minOut: number = 0): Promise<void> {
  const vaultAddress = process.env.VAULT_ADDRESS || process.env.VAULT_CONTRACT_ID;
  if (!vaultAddress) {
    throw new Error("VAULT_ADDRESS environment variable is not set");
  }

  console.log(`Auto-compounding yield on vault ${vaultAddress} with min_out=${minOut}`);

  await submitAutoCompoundTx(minOut);
}

function startDecisionLoop() {
  logger.info('Initializing hourly decision loop');

  decisionInterval = setInterval(async () => {
    try {
      logger.info('Running hourly yield evaluation');
      if (!process.env.DATABASE_URL) {
        logger.warn('DATABASE_URL is not set; cannot load user strategies, skipping hourly evaluation');
        return;
      }

      // Each user's strategy comes from users.strategy_preference and the
      // current position from the latest rebalances row (#749).
      const { usersEvaluated, decisions } = await runRebalanceCycle(pool, evaluateYield);
      const rebalanceNeeded = [...decisions.values()].some((d) => d.shouldRebalance);
      logger.info(
        { usersEvaluated, strategies: Object.fromEntries(decisions) },
        'Hourly yield evaluation complete',
      );

      if (usersEvaluated > 0 && !rebalanceNeeded) {
        console.log(`Hourly check: Yield is optimal. No action needed.`);
        // Yield is already in the best protocol; compound it for maximum growth
        await autoCompound(0);
      } else if (usersEvaluated > 0 && rebalanceNeeded) {
        console.log(`Hourly check: Rebalance needed.`);
        const currentAllocation = await getCurrentAllocation(pool);
        
        // Batch rebalances: iterate through decisions and trigger rebalance for target protocols.
        // Since it's a single vault, we just pick the first valid target protocol to rebalance to.
        const decisionToRebalance = [...decisions.values()].find((d) => d.shouldRebalance && d.targetProtocol);
        
        if (decisionToRebalance?.targetProtocol) {
          logger.info({ targetProtocol: decisionToRebalance.targetProtocol }, 'Submitting batch rebalance transaction');
          await submitRebalanceTx(decisionToRebalance.targetProtocol, currentAllocation.apy);
        }
      }
    } catch (error) {
      logger.error({ error: error instanceof Error ? error.message : error }, 'Decision loop error');
    }
  }, 60 * 60 * 1000);
}

let isShuttingDown = false;

export async function gracefulShutdown(
  signal: string,
  serverInstance?: import('http').Server,
  options = { timeoutMs: 10000, exitProcess: true }
): Promise<void> {
  if (isShuttingDown) {
    logger.warn(`${signal} received while shutdown already in progress; ignoring duplicate signal`);
    return;
  }
  isShuttingDown = true;
  logger.info(`${signal} received, initiating graceful shutdown`);

  // Force exit safeguard timer
  let forceTimer: NodeJS.Timeout | null = null;
  if (options.exitProcess) {
    forceTimer = setTimeout(() => {
      logger.fatal('Graceful shutdown timed out; forcing process exit');
      process.exit(1);
    }, options.timeoutMs);
    forceTimer.unref();
  }

  // 1. Stop hourly decision loop
  if (decisionInterval) {
    clearInterval(decisionInterval);
    decisionInterval = null;
    logger.info('Decision loop stopped');
  }

  // 2. Stop accepting incoming HTTP connections and drain in-flight requests
  if (serverInstance) {
    await new Promise<void>((resolve) => {
      serverInstance.close((err) => {
        if (err) {
          logger.warn({ error: err.message }, 'Error closing HTTP server');
        } else {
          logger.info('HTTP server closed');
        }
        resolve();
      });
    });
  }

  // 3. Gracefully stop event listener and await completion of in-flight polls
  try {
    await stopEventListener();
  } catch (err) {
    logger.error({ error: err instanceof Error ? err.message : String(err) }, 'Error stopping event listener');
  }

  // 4. Drain and close PostgreSQL connection pool
  try {
    await closePool();
  } catch (err) {
    logger.error({ error: err instanceof Error ? err.message : String(err) }, 'Error closing database pool');
  }

  if (forceTimer) clearTimeout(forceTimer);
  logger.info('Graceful shutdown completed successfully');

  if (options.exitProcess) {
    process.exit(0);
  }
}

async function main() {
  logger.info('Starting NeuroWealth AI Agent');

  configureHealthChecks(pool, server);
  await startEventListener();
  startDecisionLoop();

  const serverInstance = app.listen(PORT, () => {
    logger.info({ port: PORT }, 'Agent HTTP server listening');
  });

  process.on('SIGTERM', () => {
    gracefulShutdown('SIGTERM', serverInstance).catch((err) => {
      logger.fatal({ error: err instanceof Error ? err.message : String(err) }, 'Shutdown error');
      process.exit(1);
    });
  });
  process.on('SIGINT', () => {
    gracefulShutdown('SIGINT', serverInstance).catch((err) => {
      logger.fatal({ error: err instanceof Error ? err.message : String(err) }, 'Shutdown error');
      process.exit(1);
    });
  });
}

if (require.main === module) {
  main().catch((err) => {
    logger.fatal({ error: err.message }, 'Startup failed');
    process.exit(1);
  });
}