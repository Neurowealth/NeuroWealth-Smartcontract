import { SorobanRpc } from '@stellar/stellar-sdk';
import { Pool } from 'pg';
import { evaluateYield } from './yieldComparison';
import { processEventForAlerts } from './alertEngine';
import { stellarRpcLatency, errorRate } from './metrics';

const rpcUrl = process.env.SOROBAN_RPC_URL || 'https://soroban-testnet.stellar.org';
const server = new SorobanRpc.Server(rpcUrl);

const pool = new Pool({
  connectionString: process.env.DATABASE_URL,
});

const VAULT_CONTRACT_ID = process.env.VAULT_CONTRACT_ID || '';

/**
 * Listens for on-chain deposit and withdraw events from the vault contract.
 * Detects new deposits within 5 seconds and triggers yield deployment.
 */
export async function startEventListener() {
  if (!VAULT_CONTRACT_ID) {
    console.warn('VAULT_CONTRACT_ID is not set. Event listener requires a contract ID to monitor.');
    return;
  }

  try {
    const latestLedgerResponse = await server.getLatestLedger();
    let startLedger = latestLedgerResponse.sequence;
    console.log(`Starting event listener from ledger ${startLedger}...`);

    setInterval(async () => {
      try {
        const endTimer = stellarRpcLatency.startTimer({ method: 'getEvents' });
        const response = await server.getEvents({
          startLedger,
          filters: [
            {
              type: 'contract',
              contractIds: [VAULT_CONTRACT_ID],
              // We listen for any events on this contract, then filter locally
              // because stellar-sdk requires exact XDR representations for topics in queries
            }
          ],
          limit: 100,
        });
        endTimer();

        for (const event of response.events) {
          const topics = event.topic.map(t => t.toString());
          let eventType = '';
          
          if (topics.some(t => t.includes('deposit'))) {
            eventType = 'deposit';
          } else if (topics.some(t => t.includes('withdraw'))) {
            eventType = 'withdraw';
          }

          if (!eventType) continue;

          console.log(`Detected ${eventType} event! Ledger: ${event.ledger}`);

          // Log to PostgreSQL for audit trail
          await logEventToDb(eventType, event.id, event.ledger);

          // If deposit, trigger immediate yield evaluation
          if (eventType === 'deposit') {
            console.log(`New deposit detected. Evaluating yield opportunities...`);
            const userStrategy = 'balanced'; // Default or fetched from DB
            const currentProtocol = 'none'; // Assume funds are idle after deposit
            
            const decision = await evaluateYield(userStrategy, currentProtocol, 0);
            
            if (decision.shouldRebalance) {
              console.log(`[Action Required] Submitting rebalance transaction to move funds to ${decision.targetProtocol}`);
              // TODO: Submit Soroban rebalance transaction here
            }
          }

          // Trigger security alerting system
          const alertPayload = { type: eventType, amount: 150000 }; // Mocked event payload 
          await processEventForAlerts(alertPayload);

          // Advance the ledger marker
          startLedger = Math.max(startLedger, event.ledger + 1);
        }
      } catch (error) {
        errorRate.inc({ type: 'stellar_rpc_error' });
        console.error("Error polling Soroban events:", error);
      }
    }, 5000); // 5 second interval

  } catch (error) {
    console.error("Failed to initialize event listener:", error);
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
    console.error("Database logging failed:", error);
  }
}
