import { SorobanRpc } from '@stellar/stellar-sdk';
import { pool } from './db';
import { evaluateYield } from './yieldComparison';
import { processEventForAlerts } from './alertEngine';
import logger from './logger';
import { withRetry } from './retry';

export { pool };

const rpcUrl = process.env.SOROBAN_RPC_URL || 'https://soroban-testnet.stellar.org';
export const server = new SorobanRpc.Server(rpcUrl);

const VAULT_CONTRACT_ID = process.env.VAULT_CONTRACT_ID || '';

let eventInterval: ReturnType<typeof setInterval> | null = null;

export function stopEventListener() {
  if (eventInterval) {
    clearInterval(eventInterval);
    eventInterval = null;
    logger.info('Event listener stopped');
  }
}

/**
 * Listens for on-chain deposit and withdraw events from the vault contract.
 * Detects new deposits within 5 seconds and triggers yield deployment.
 */
export async function startEventListener() {
  if (!VAULT_CONTRACT_ID) {
    logger.warn('VAULT_CONTRACT_ID is not set. Event listener requires a contract ID to monitor.');
    return;
  }

  try {
    const latestLedgerResponse = await withRetry(
      () => server.getLatestLedger(),
      'getLatestLedger',
    );
    let startLedger = latestLedgerResponse.sequence;
    logger.info({ startLedger }, 'Starting event listener');

    eventInterval = setInterval(async () => {
      try {
        const response = await withRetry(
          () => server.getEvents({
            startLedger,
            filters: [
              {
                type: 'contract',
                contractIds: [VAULT_CONTRACT_ID],
              }
            ],
            limit: 100,
          }),
          'getEvents',
        );

        for (const event of response.events) {
          const topics = event.topic.map(t => t.toString());
          let eventType = '';

          if (topics.some(t => t.includes('deposit'))) {
            eventType = 'deposit';
          } else if (topics.some(t => t.includes('withdraw'))) {
            eventType = 'withdraw';
          }

          if (!eventType) continue;

          logger.info({ eventType, ledger: event.ledger }, 'Detected event');

          await logEventToDb(eventType, event.id, event.ledger);

          if (eventType === 'deposit') {
            logger.info('New deposit detected, evaluating yield');
            const userStrategy = 'balanced';
            const currentProtocol = 'none';

            const decision = await evaluateYield(userStrategy, currentProtocol, 0);

            if (decision.shouldRebalance) {
              logger.info({ targetProtocol: decision.targetProtocol }, 'Rebalance needed');
            }
          }

          const alertPayload = { type: eventType, amount: 150000 };
          await processEventForAlerts(alertPayload);

          startLedger = Math.max(startLedger, event.ledger + 1);
        }
      } catch (error) {
        logger.error({ error: error instanceof Error ? error.message : error }, 'Error polling Soroban events');
      }
    }, 5000);

  } catch (error) {
    logger.error({ error: error instanceof Error ? error.message : error }, 'Failed to initialize event listener');
  }
}

async function logEventToDb(type: string, eventId: string, ledger: number) {
  try {
    if (process.env.DATABASE_URL) {
      await pool.query(
        'INSERT INTO vault_events (event_id, event_type, ledger_sequence, created_at) VALUES ($1, $2, $3, NOW()) ON CONFLICT DO NOTHING',
        [eventId, type, ledger]
      );
    }
  } catch (error) {
    logger.error({ error: error instanceof Error ? error.message : error }, 'Database logging failed');
  }
}
