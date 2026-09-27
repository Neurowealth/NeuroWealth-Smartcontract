import 'dotenv/config';
import express from 'express';
import { startEventListener, stopEventListener, server, pool } from './eventListener';
import { evaluateYield } from './yieldComparison';
import { runRebalanceCycle } from './userStrategies';
import healthRouter, { configureHealthChecks } from './health';
import logger from './logger';
import { initializeTracing } from './tracing';
import { submitAutoCompoundTx, submitRebalanceTx } from './sorobanTx';
import { createDecisionId } from './rebalanceOperations';
import { resolveExecutionMode } from './executionMode';
import { RollupScheduler } from './dataRollup';

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
const rollupScheduler = new RollupScheduler(pool);

/**
 * Invokes the vault contract's `auto_compound(min_out)` function to harvest
 * accrued yield and immediately reinvest it in the same protocol, maximizing
 * compound growth without user intervention.
 *
 * @param minOut - Minimum amount of yield that must be compounded; reverts if
 *                 the available yield is below this threshold.
 */
async function autoCompound(minOut: number = 0, mode: 'live' | 'dry-run'): Promise<void> {
  const vaultAddress = process.env.VAULT_ADDRESS || process.env.VAULT_CONTRACT_ID;
  if (!vaultAddress) {
    throw new Error("VAULT_ADDRESS environment variable is not set");
  }

  console.log(`Auto-compounding yield on vault ${vaultAddress} with min_out=${minOut}`);

  await submitAutoCompoundTx(minOut, mode);
}

function startDecisionLoop(executionMode: 'live' | 'dry-run') {
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
      const cycle = await runRebalanceCycle(pool, evaluateYield);
      const { usersEvaluated, decisions } = cycle;
      const rebalanceNeeded = [...decisions.values()].some((d) => d.shouldRebalance);
      const cycleSnapshot = {
        currentAllocation: cycle.currentAllocation,
        strategySnapshot: cycle.strategySnapshot,
        decisions: [...decisions.entries()].sort(([a], [b]) => a.localeCompare(b)),
      };
      const decisionId = createDecisionId(
        'hourly-rebalance',
        new Date().toISOString().slice(0, 13),
        cycleSnapshot,
      );
      logger.info(
        { usersEvaluated, strategies: Object.fromEntries(decisions) },
        'Hourly yield evaluation complete',
      );

      if (usersEvaluated > 0 && !rebalanceNeeded) {
        console.log(`Hourly check: Yield is optimal. No action needed.`);
        // Yield is already in the best protocol; compound it for maximum growth
        await autoCompound(0, executionMode);
      } else if (usersEvaluated > 0 && rebalanceNeeded) {
        console.log(`Hourly check: Rebalance needed.`);
        const currentAllocation = cycle.currentAllocation;
        
        // Batch rebalances: iterate through decisions and trigger rebalance for target protocols.
        // Since it's a single vault, we just pick the first valid target protocol to rebalance to.
        const decisionToRebalance = [...decisions.values()].find((d) => d.shouldRebalance && d.targetProtocol);
        
        if (decisionToRebalance?.targetProtocol) {
          logger.info({ targetProtocol: decisionToRebalance.targetProtocol }, 'Submitting batch rebalance transaction');
          await submitRebalanceTx(decisionToRebalance.targetProtocol, currentAllocation.apy, {
            mode: executionMode,
            decisionId,
            snapshot: cycleSnapshot,
          });
        }
      }
    } catch (error) {
      logger.error({ error: error instanceof Error ? error.message : error }, 'Decision loop error');
    }
  }, 60 * 60 * 1000);
}

async function main() {
  const executionMode = resolveExecutionMode();
  logger.info('Starting NeuroWealth AI Agent');

  configureHealthChecks(pool, server);
  await startEventListener();
  startDecisionLoop(executionMode);

  if (process.env.DATABASE_URL) {
    rollupScheduler.start();
  } else {
    logger.warn('DATABASE_URL is not set; nightly rollup/cleanup job disabled');
  }

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

    rollupScheduler.stop();
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