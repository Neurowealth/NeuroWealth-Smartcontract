import 'dotenv/config';
import { startEventListener } from './eventListener';
import { evaluateYield } from './yieldComparison';

/**
 * Main entry point for the NeuroWealth AI Agent backend.
 */
async function main() {
  console.log("Starting NeuroWealth AI Agent...");
  
  // Start the event listener to detect real-time deposits and withdrawals
  await startEventListener();

  // Start the hourly decision loop for periodic yield optimization
  startDecisionLoop();
}

/**
 * Invokes the vault contract's `auto_compound(min_out)` function to harvest
 * accrued yield and immediately reinvest it in the same protocol, maximizing
 * compound growth without user intervention.
 *
 * @param minOut - Minimum amount of yield that must be compounded; reverts if
 *                 the available yield is below this threshold.
 */
async function autoCompound(minOut: number = 0): Promise<void> {
  const vaultAddress = process.env.VAULT_ADDRESS;
  if (!vaultAddress) {
    throw new Error("VAULT_ADDRESS environment variable is not set");
  }

  console.log(`Auto-compounding yield on vault ${vaultAddress} with min_out=${minOut}`);

  // TODO: Replace with a real Soroban contract invocation, e.g.:
  // const vault = new Contract(vaultAddress);
  // await vault.call('auto_compound', minOut);
  // This is intentionally left as a placeholder because the RPC client
  // configuration is environment-specific.
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
        // Yield is already in the best protocol; compound it for maximum growth
        await autoCompound(0);
      }
    } catch (error) {
      console.error("Hourly decision loop encountered an error:", error);
    }
  }, 60 * 60 * 1000);
}

main().catch(console.error);
