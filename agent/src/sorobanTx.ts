import { Keypair, TransactionBuilder, Networks, Contract, rpc, Account, TimeoutInfinite, xdr } from '@stellar/stellar-sdk';
import logger from './logger';
import { server } from './eventListener';

const networkPassphrase = process.env.SOROBAN_NETWORK_PASSPHRASE || Networks.TESTNET;
const VAULT_CONTRACT_ID = process.env.VAULT_CONTRACT_ID;
const secretKey = process.env.SOROBAN_SECRET_KEY;

let agentKeypair: Keypair | null = null;
if (secretKey) {
  try {
    agentKeypair = Keypair.fromSecret(secretKey);
  } catch {
    logger.error('Invalid SOROBAN_SECRET_KEY provided.');
  }
}

async function loadAccount(publicKey: string): Promise<Account> {
  const accountResp = await server.getAccount(publicKey);
  return new Account(publicKey, accountResp.sequenceNumber());
}

/**
 * Submits a transaction to the vault contract.
 */
async function submitVaultTransaction(method: string, args: xdr.ScVal[] = []): Promise<boolean> {
  if (!agentKeypair) {
    logger.warn('No SOROBAN_SECRET_KEY set. Cannot submit transaction.');
    return false;
  }
  if (!VAULT_CONTRACT_ID) {
    logger.warn('No VAULT_CONTRACT_ID set. Cannot submit transaction.');
    return false;
  }

  try {
    logger.info(`Submitting ${method} transaction to vault...`);
    const account = await loadAccount(agentKeypair.publicKey());
    
    // Simulate transaction to get footprint and calculate fee
    const contract = new Contract(VAULT_CONTRACT_ID);
    const op = contract.call(method, ...args);

    let tx = new TransactionBuilder(account, {
      fee: '100', // Base fee, gets updated by simulation
      networkPassphrase,
    })
      .addOperation(op)
      .setTimeout(TimeoutInfinite)
      .build();

    const simulated = await server.simulateTransaction(tx);
    
    if (rpc.Api.isSimulationError(simulated)) {
      logger.error({ error: simulated.error }, 'Transaction simulation failed');
      return false;
    }

    tx = rpc.assembleTransaction(tx, simulated).build();
    tx.sign(agentKeypair);

    const txResponse = await server.sendTransaction(tx);
    
    if (txResponse.status === 'ERROR') {
      logger.error({ error: txResponse.errorResult }, 'Transaction submission failed');
      return false;
    }

    if (txResponse.status === 'PENDING') {
      // In production, we'd poll for GetTransaction status, but we'll consider it submitted for now.
      logger.info({ hash: txResponse.hash, status: txResponse.status }, `Transaction submitted successfully.`);
      return true;
    }

    return false;
  } catch (error) {
    logger.error({ error: error instanceof Error ? error.message : String(error) }, `Error submitting ${method} transaction`);
    return false;
  }
}

/**
 * Submits a rebalance transaction to the vault.
 * Rebalances vault funds into the target protocol.
 */
export async function submitRebalanceTx(targetProtocol: string, expectedApy: number): Promise<boolean> {
  // Map targetProtocol string to symbol. 
  // Assuming the contract requires 'rebalance' method and args like (protocol: Symbol, expected_apy: i128, min_out: i128).
  // Native to ScVal using nativeToScVal
  const { nativeToScVal } = await import('@stellar/stellar-sdk');
  
  const args = [
    nativeToScVal(targetProtocol, { type: 'symbol' }),
    nativeToScVal(expectedApy, { type: 'i128' }), // Expected APY
    nativeToScVal(0, { type: 'i128' }) // min_out
  ];

  return submitVaultTransaction('rebalance', args);
}

/**
 * Submits an auto_compound transaction to the vault.
 */
export async function submitAutoCompoundTx(minOut: number = 0): Promise<boolean> {
  const { nativeToScVal } = await import('@stellar/stellar-sdk');
  
  const args = [
    nativeToScVal(minOut, { type: 'i128' })
  ];

  return submitVaultTransaction('auto_compound', args);
}
