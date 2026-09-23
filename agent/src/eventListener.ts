import { rpc, scValToNative } from '@stellar/stellar-sdk';
import { pool } from './db';
import { evaluateYield } from './yieldComparison';
import { processEventForAlerts } from './alertEngine';
import logger from './logger';
import { withRetry } from './retry';
import {
  rebalanceConfigFromEnv,
  submitRebalance,
  type RebalanceOutcome,
} from './rebalanceSubmitter';

export { pool };

const rpcUrl = process.env.SOROBAN_RPC_URL || 'https://soroban-testnet.stellar.org';
export const server = new rpc.Server(rpcUrl);

const VAULT_CONTRACT_ID = process.env.VAULT_CONTRACT_ID || '';

let eventInterval: ReturnType<typeof setInterval> | null = null;

/**
 * Reads the user and USDC amount out of an event payload (#687).
 *
 * The vault emits `DepositEvent { user, amount, shares }`; amounts arrive in
 * stroops (7 decimals) and are returned here in whole USDC, which is the unit the
 * alert rules compare against. An event whose payload cannot be read yields zero
 * rather than a guess.
 */
export function decodeEventPayload(value: unknown): { user: string | null; amount: number } {
  try {
    const native = scValToNative(value as never) as Record<string, unknown> | Map<string, unknown> | null;
    if (!native) return { user: null, amount: 0 };

    const read = (key: string): unknown =>
      native instanceof Map ? native.get(key) : (native as Record<string, unknown>)[key];

    const rawAmount = read('amount');
    const stroops =
      typeof rawAmount === 'bigint'
        ? rawAmount
        : typeof rawAmount === 'number'
          ? BigInt(Math.trunc(rawAmount))
          : typeof rawAmount === 'string' && /^[0-9]+$/.test(rawAmount)
            ? BigInt(rawAmount)
            : null;

    const rawUser = read('user');
    const user = typeof rawUser === 'string' ? rawUser : null;

    if (stroops === null) return { user, amount: 0 };
    return { user, amount: Number(stroops) / 10 ** 7 };
  } catch {
    return { user: null, amount: 0 };
  }
}

/**
 * Submits the rebalance the yield decision asked for, or explains why it did not (#687).
 *
 * Submission is opt-in on configuration: `AGENT_SECRET_KEY`,
 * `REBALANCE_EXPECTED_APY_BPS` and `REBALANCE_MIN_OUT` all have to be set, because the
 * last two decide how much the on-chain trade may lose. Without them the listener
 * keeps making the decision and logs it, rather than rebalancing with values
 * nobody chose.
 */
async function maybeSubmitRebalance(targetProtocol: string): Promise<void> {
  const configured = rebalanceConfigFromEnv(process.env);
  if (!configured.ok) {
    logger.info(
      { reason: configured.reason },
      'Rebalance submission is not configured; decision logged only',
    );
    return;
  }

  const outcome: RebalanceOutcome = await submitRebalance(
    { contractId: VAULT_CONTRACT_ID, protocol: targetProtocol, config: configured.config },
    { server },
  );

  if (outcome.submitted) {
    logger.info({ hash: outcome.hash, protocol: targetProtocol }, 'Rebalance submitted');
  } else {
    logger.error({ reason: outcome.reason }, 'Rebalance submission failed');
  }
}

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

          const decoded = decodeEventPayload(event.value);
          logger.info(
            { eventType, ledger: event.ledger, user: decoded.user, amount: decoded.amount },
            'Detected event',
          );

          await logEventToDb(eventType, event.id, event.ledger);

          if (eventType === 'deposit') {
            logger.info('New deposit detected, evaluating yield');
            const userStrategy = 'balanced';
            const currentProtocol = 'none';

            const decision = await evaluateYield(userStrategy, currentProtocol, decoded.amount);

            if (decision.shouldRebalance) {
              logger.info({ targetProtocol: decision.targetProtocol }, 'Rebalance needed');
              await maybeSubmitRebalance(decision.targetProtocol);
            }
          }

          const alertPayload = { type: eventType, amount: decoded.amount };
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
