import { Keypair, TransactionBuilder, Networks, Contract, rpc, Account, Address, nativeToScVal, scValToNative, xdr } from '@stellar/stellar-sdk';
import logger from './logger';
import { server } from './eventListener';
import { pool } from './db';
import { resolveExecutionMode, type ExecutionMode } from './executionMode';
import { rawToUsdc } from './vaultEventPayload';
import {
  executeRebalanceOperation,
  PostgresRebalanceOperationRepository,
  type OperationResult,
  type OperationTransport,
} from './rebalanceOperations';

const networkPassphrase = process.env.SOROBAN_NETWORK_PASSPHRASE || Networks.TESTNET;
const VAULT_CONTRACT_ID = process.env.VAULT_CONTRACT_ID;
const secretKey = process.env.SOROBAN_SECRET_KEY;

// Configurable transaction timeout in seconds (default: 60 seconds)
const TX_TIMEOUT_SECONDS = parseInt(process.env.TX_TIMEOUT_SECONDS || '60', 10);

// Configurable inclusion fee in stroops (default: 100 stroops)
const TX_INCLUSION_FEE = parseInt(process.env.TX_INCLUSION_FEE || '100', 10);

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

export interface VaultSubmissionOptions {
  mode?: ExecutionMode;
  decisionId?: string;
  snapshot?: unknown;
  retryReason?: string;
}

function rejectedResult(reason: string): OperationResult {
  return { operationId: '', status: 'rejected', rejectionReasons: [reason] };
}

/** Simulates every action; only explicit live mode can submit it. */
async function submitVaultTransaction(
  method: string,
  args: xdr.ScVal[] = [],
  options: VaultSubmissionOptions = {},
): Promise<OperationResult> {
  const mode = options.mode ?? resolveExecutionMode();
  const sourcePublicKey = agentKeypair?.publicKey() || process.env.SOROBAN_PUBLIC_KEY;

  try {
    let tx: ReturnType<TransactionBuilder['build']> | undefined;
    let simulation: Awaited<ReturnType<typeof server.simulateTransaction>> | undefined;
    const ensureTx = async () => {
      if (!tx) {
        if (!VAULT_CONTRACT_ID) throw new Error('VAULT_CONTRACT_ID is not configured');
        if (!sourcePublicKey) throw new Error('SOROBAN_PUBLIC_KEY or SOROBAN_SECRET_KEY is required');
        const account = await loadAccount(sourcePublicKey);
        const contract = new Contract(VAULT_CONTRACT_ID);
        const op = contract.call(method, ...args);
        tx = new TransactionBuilder(account, { fee: String(TX_INCLUSION_FEE), networkPassphrase })
          .addOperation(op)
          .setTimeout(TX_TIMEOUT_SECONDS)
          .build();
      }
      return tx;
    };
    const transport: OperationTransport = {
      async simulate() {
        const unsignedTx = await ensureTx();
        simulation = await server.simulateTransaction(unsignedTx);
        if (rpc.Api.isSimulationError(simulation)) {
          const reason = simulation.error || 'Transaction simulation failed';
          logger.warn({ method, error: reason, mode }, 'Vault transaction simulation rejected');
          return { success: false, result: simulation, rejectionReasons: [reason] };
        }
        return {
          success: true,
          result: {
            minResourceFee: simulation.minResourceFee,
            latestLedger: simulation.latestLedger,
            result: simulation.result,
          },
        };
      },
      async prepare() {
        const unsignedTx = await ensureTx();
        if (!simulation || rpc.Api.isSimulationError(simulation)) {
          throw new Error('A successful simulation is required before signing');
        }
        if (!agentKeypair) throw new Error('SOROBAN_SECRET_KEY is required in live mode');
        // Use resource fee from simulation and add inclusion fee
        const resourceFee = Number(simulation.minResourceFee) || 0;
        const _totalFee = resourceFee + TX_INCLUSION_FEE;
        const signedTx = rpc.assembleTransaction(unsignedTx, simulation).build();
        signedTx.sign(agentKeypair);
        const txHash = signedTx.hash().toString('hex');
        return {
          txHash,
          async send() {
            const response = await server.sendTransaction(signedTx);
            if (response.status === 'ERROR') {
              return { accepted: false, error: String(response.errorResult) };
            }
            if (response.status === 'PENDING') return { accepted: true };
            throw new Error(`Ambiguous Soroban submission status: ${response.status}`);
          },
        };
      },
      async reconcile(txHash) {
        try {
          const response = await server.getTransaction(txHash);
          if (response.status === 'SUCCESS') return 'confirmed';
          if (response.status === 'FAILED') return 'rejected';
          return 'unknown';
        } catch (error) {
          logger.warn({ txHash, error: error instanceof Error ? error.message : String(error) }, 'Transaction reconciliation failed');
          return 'unknown';
        }
      },
    };

    if (method === 'rebalance') {
      if (!options.decisionId || options.snapshot === undefined) {
        return rejectedResult('A stable decisionId and input snapshot are required for rebalance');
      }
      if (mode === 'live' && !process.env.DATABASE_URL) {
        return rejectedResult('DATABASE_URL is required for idempotent live rebalance submission');
      }
      const repository = new PostgresRebalanceOperationRepository(pool);
      const result = await executeRebalanceOperation({
        mode,
        decisionId: options.decisionId,
        snapshot: options.snapshot,
        retryReason: options.retryReason,
        repository,
        transport,
      });
      logger.info(
        { ...result, mode, proposedAction: { method, args }, simulationResult: result.simulation },
        mode === 'dry-run' ? 'Dry-run decision complete' : 'Rebalance operation state updated',
      );
      return result;
    }

    const simulationResult = await transport.simulate();
    if (!simulationResult.success) {
      const result: OperationResult = {
        operationId: '',
        status: mode === 'dry-run' ? 'dry_run' : 'rejected',
        simulation: simulationResult.result,
        rejectionReasons: simulationResult.rejectionReasons ?? [],
      };
      logger.info(
        { ...result, mode, proposedAction: { method, args }, simulationResult: result.simulation },
        mode === 'dry-run' ? 'Dry-run decision rejected by simulation' : 'Vault transaction rejected by simulation',
      );
      return result;
    }
    if (mode === 'dry-run') {
      const result: OperationResult = {
        operationId: `dry-run:${method}`,
        status: 'dry_run',
        simulation: simulationResult.result,
        rejectionReasons: [],
      };
      logger.info({ ...result, proposedAction: { method, args }, simulationResult: result.simulation }, 'Dry-run decision complete');
      return result;
    }
    const prepared = await transport.prepare();
    const response = await prepared.send();
    if (!response.accepted) return rejectedResult(response.error || 'Transaction submission rejected');
    return {
      operationId: prepared.txHash,
      status: 'submitted',
      txHash: prepared.txHash,
      simulation: simulationResult.result,
      rejectionReasons: [],
    };
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    logger.error({ error: reason, method, mode }, `Error submitting ${method} transaction`);
    return rejectedResult(reason);
  }
}

/** Upper bound the vault accepts for `expected_apy` (basis points). */
export const MAX_EXPECTED_APY_BPS = 10_000;

/**
 * Converts an APY in percent points (e.g. 5.25) into the basis-point integer
 * the vault's `rebalance` entrypoint expects (e.g. 525). Returns null when the
 * value is not finite or falls outside the contract's accepted 0..=10_000 range.
 */
export function toExpectedApyBps(apyPercent: number): number | null {
  if (!Number.isFinite(apyPercent)) return null;
  const bps = Math.round(apyPercent * 100);
  if (bps < 0 || bps > MAX_EXPECTED_APY_BPS) return null;
  return bps;
}

/**
 * Builds the argument list for the vault's
 * `rebalance(agent: Address, protocol: Symbol, expected_apy: i128, min_out: i128)`.
 */
export function buildRebalanceArgs(
  agentPublicKey: string,
  targetProtocol: string,
  expectedApyBps: number,
  minOut = 0,
): xdr.ScVal[] {
  return [
    new Address(agentPublicKey).toScVal(),
    nativeToScVal(targetProtocol, { type: 'symbol' }),
    nativeToScVal(expectedApyBps, { type: 'i128' }),
    nativeToScVal(minOut, { type: 'i128' }),
  ];
}

/**
 * Submits a rebalance transaction to the vault.
 * Rebalances vault funds into the target protocol.
 *
 * `expectedApy` is in percent points (the unit the yield evaluator uses) and is
 * converted to basis points before it is sent on chain.
 */
export async function submitRebalanceTx(
  targetProtocol: string,
  expectedApy: number,
  options: VaultSubmissionOptions = {},
): Promise<OperationResult> {
  const agentPublicKey = agentKeypair?.publicKey() || process.env.SOROBAN_PUBLIC_KEY;
  if (!agentPublicKey) {
    return rejectedResult('SOROBAN_PUBLIC_KEY or SOROBAN_SECRET_KEY is required to rebalance');
  }
  const expectedApyBps = toExpectedApyBps(expectedApy);
  if (expectedApyBps === null) {
    return rejectedResult(`Expected APY ${expectedApy}% is outside the vault's accepted range`);
  }

  const args = buildRebalanceArgs(agentPublicKey, targetProtocol, expectedApyBps);
  return submitVaultTransaction('rebalance', args, options);
}

/**
 * Reads the vault's current total assets (in whole USDC) from chain via a
 * read-only `get_total_assets` simulation. Returns null when the vault cannot
 * be read, so callers never fall back to fabricated vault state.
 */
export async function fetchVaultTotalAssets(): Promise<number | null> {
  const sourcePublicKey = agentKeypair?.publicKey() || process.env.SOROBAN_PUBLIC_KEY;
  if (!VAULT_CONTRACT_ID || !sourcePublicKey) return null;

  try {
    const tx = new TransactionBuilder(new Account(sourcePublicKey, '0'), {
      fee: String(TX_INCLUSION_FEE),
      networkPassphrase,
    })
      .addOperation(new Contract(VAULT_CONTRACT_ID).call('get_total_assets'))
      .setTimeout(TX_TIMEOUT_SECONDS)
      .build();
    const simulation = await server.simulateTransaction(tx);
    if (!rpc.Api.isSimulationSuccess(simulation) || !simulation.result?.retval) {
      logger.warn({ error: 'error' in simulation ? simulation.error : 'No retval' }, 'get_total_assets simulation failed');
      return null;
    }
    return rawToUsdc(BigInt(scValToNative(simulation.result.retval)));
  } catch (error) {
    logger.warn({ error: error instanceof Error ? error.message : String(error) }, 'Failed to read vault total assets');
    return null;
  }
}

/**
 * Submits an auto_compound transaction to the vault.
 */
export async function submitAutoCompoundTx(minOut: number = 0, mode?: ExecutionMode): Promise<OperationResult> {
  const args = [
    nativeToScVal(minOut, { type: 'i128' })
  ];

  return submitVaultTransaction('auto_compound', args, { mode });
}
