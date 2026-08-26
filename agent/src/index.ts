import 'dotenv/config';
import { startEventListener } from './eventListener';
import { evaluateYield } from './yieldComparison';
import { startMetricsServer } from './metrics';

/**
 * Main entry point for the NeuroWealth AI Agent backend.
 */
async function main() {
  console.log("Starting NeuroWealth AI Agent...");
  
  // Start metrics server
  startMetricsServer();

  // Start the event listener to detect real-time deposits and withdrawals
  await startEventListener();

  // Start the hourly decision loop for periodic yield optimization
  startDecisionLoop();
}

function startDecisionLoop() {
  console.log("Initializing the hourly decision loop...");
  
  // Run every hour (3600000 ms)
  setInterval(async () => {
    try {
      console.log("Running hourly yield evaluation...");
      // In a real scenario, this would iterate over all users or active vaults
      // and evaluate their specific strategy.
      const decision = await evaluateYield('balanced', 'blend', 6.5);
      
      if (decision.shouldRebalance) {
        console.log(`Hourly check: Rebalance needed to ${decision.targetProtocol}`);
      } else {
        console.log(`Hourly check: Yield is optimal. No action needed.`);
      }
    } catch (error) {
      console.error("Hourly decision loop encountered an error:", error);
    }
  }, 60 * 60 * 1000);
}

main().catch(console.error);
