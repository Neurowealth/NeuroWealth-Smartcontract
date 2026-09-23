import {
  Account,
  BASE_FEE,
  Contract,
  Keypair,
  TransactionBuilder,
  nativeToScVal,
  rpc,
} from '@stellar/stellar-sdk';

/**
 * Submits the vault's `rebalance(protocol, expected_apy, min_out)` call (#687).
 *
 * The agent is allowed to move funds, so nothing here is guessed: the slippage
 * floor and the expected APY have to come from configuration, and the submitter
 * refuses to run without them rather than inventing the values that decide how
 * much an on-chain trade is allowed to lose.
 */

export const TESTNET_PASSPHRASE = 'Test SDF Network ; September 2015';

export interface RebalanceConfig {
  agentSecretKey: string;
  expectedApyBps: number;
  minOut: string;
  networkPassphrase: string;
}

export type ConfigResult =
  | { ok: true; config: RebalanceConfig }
  | { ok: false; reason: string };

function parseInteger(value: string | undefined): number | null {
  if (value === undefined || value.trim() === '') return null;
  if (!/^-?[0-9]+$/.test(value.trim())) return null;
  const parsed = Number(value.trim());
  return Number.isSafeInteger(parsed) ? parsed : null;
}

/**
 * Reads the submission configuration, or explains what is missing.
 *
 * Deliberately strict: every value that influences the on-chain trade has to be
 * supplied, so an operator cannot end up rebalancing with a slippage floor or an
 * APY target they never chose.
 */
export function rebalanceConfigFromEnv(
  env: NodeJS.ProcessEnv = process.env,
): ConfigResult {
  const secret = env.AGENT_SECRET_KEY?.trim();
  if (!secret) {
    return { ok: false, reason: 'AGENT_SECRET_KEY is not set' };
  }
  if (!secret.startsWith('S') || secret.length !== 56) {
    return { ok: false, reason: 'AGENT_SECRET_KEY is not a Stellar secret key' };
  }

  const expectedApyBps = parseInteger(env.REBALANCE_EXPECTED_APY_BPS);
  if (expectedApyBps === null) {
    return { ok: false, reason: 'REBALANCE_EXPECTED_APY_BPS is not set to an integer' };
  }
  if (expectedApyBps < 0 || expectedApyBps > 10_000) {
    return { ok: false, reason: 'REBALANCE_EXPECTED_APY_BPS must be between 0 and 10000' };
  }

  const minOut = env.REBALANCE_MIN_OUT?.trim();
  if (!minOut) {
    return { ok: false, reason: 'REBALANCE_MIN_OUT is not set' };
  }
  if (!/^[0-9]+$/.test(minOut)) {
    return { ok: false, reason: 'REBALANCE_MIN_OUT must be a non-negative integer' };
  }

  return {
    ok: true,
    config: {
      agentSecretKey: secret,
      expectedApyBps,
      minOut,
      networkPassphrase: env.STELLAR_NETWORK_PASSPHRASE?.trim() || TESTNET_PASSPHRASE,
    },
  };
}

export interface RebalanceTarget {
  contractId: string;
  protocol: string;
  config: RebalanceConfig;
}

export type RebalanceOutcome =
  | { submitted: true; hash: string; status: 'SUCCESS' }
  | { submitted: false; reason: string };

export interface SubmitDependencies {
  /** Injectable so the build, submit and polling path can be tested. */
  server: Pick<
    rpc.Server,
    'getAccount' | 'prepareTransaction' | 'sendTransaction' | 'getTransaction'
  >;
  /** Overrides the signing step; defaults to signing with the configured key. */
  sign?: (prepared: unknown, keypair: Keypair) => void;
  pollIntervalMs?: number;
  maxPolls?: number;
  sleep?: (ms: number) => Promise<void>;
}

const defaultSleep = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });

/**
 * Builds, signs and submits the rebalance, then waits for a terminal status.
 *
 * Failures are returned rather than thrown: the event listener treats a failed
 * rebalance as something to log and move past, not as a reason to stop watching
 * deposits.
 */
export async function submitRebalance(
  target: RebalanceTarget,
  deps: SubmitDependencies,
): Promise<RebalanceOutcome> {
  const pollIntervalMs = deps.pollIntervalMs ?? 1500;
  const maxPolls = deps.maxPolls ?? 10;
  const sleep = deps.sleep ?? defaultSleep;

  let keypair: Keypair;
  try {
    keypair = Keypair.fromSecret(target.config.agentSecretKey);
  } catch {
    return { submitted: false, reason: 'agent secret key could not be loaded' };
  }

  try {
    const account: Account = await deps.server.getAccount(keypair.publicKey());
    const contract = new Contract(target.contractId);

    const transaction = new TransactionBuilder(account, {
      fee: BASE_FEE,
      networkPassphrase: target.config.networkPassphrase,
    })
      .addOperation(
        contract.call(
          'rebalance',
          nativeToScVal(target.protocol, { type: 'symbol' }),
          nativeToScVal(target.config.expectedApyBps, { type: 'i128' }),
          nativeToScVal(target.config.minOut, { type: 'i128' }),
        ),
      )
      .setTimeout(30)
      .build();

    const prepared = await deps.server.prepareTransaction(transaction);
    if (deps.sign) {
      deps.sign(prepared, keypair);
    } else {
      (prepared as unknown as { sign: (keypair: Keypair) => void }).sign(keypair);
    }

    const sent = await deps.server.sendTransaction(prepared);
    if (sent.status === 'ERROR') {
      return { submitted: false, reason: 'the network rejected the transaction' };
    }

    for (let attempt = 0; attempt < maxPolls; attempt += 1) {
      const result = await deps.server.getTransaction(sent.hash);
      if (result.status === 'SUCCESS') {
        return { submitted: true, hash: sent.hash, status: 'SUCCESS' };
      }
      if (result.status === 'FAILED') {
        return { submitted: false, reason: 'the rebalance transaction failed on-chain' };
      }
      await sleep(pollIntervalMs);
    }

    return { submitted: false, reason: 'the rebalance transaction did not confirm in time' };
  } catch (error) {
    return {
      submitted: false,
      reason: error instanceof Error ? error.message : String(error),
    };
  }
}
