import { Account, Address, Contract, nativeToScVal, rpc, scValToNative, TransactionBuilder, xdr } from '@stellar/stellar-sdk';
import { getBrowserTestHarness } from './browserTestHarness';
import { signWithFreighter, classifyWalletError } from './freighter';
import {
  EMPTY_VAULT_STATE,
  deriveVaultState,
  shortenAddress as shortenSharedAddress,
  type VaultState,
} from '../../../packages/vault-client/src/stellar';

export type { VaultState };
export { EMPTY_VAULT_STATE };

export type VaultStateResult =
  | { ok: true; state: VaultState }
  | { ok: false; error: string };

function requirePublicEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} must be set; refusing to use an unsafe vault contract default`);
  }
  return value;
}

const RPC_URL = requirePublicEnv('NEXT_PUBLIC_SOROBAN_RPC_URL');
const NETWORK_PASSPHRASE = requirePublicEnv('NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE');
const VAULT_CONTRACT_ID = requirePublicEnv('NEXT_PUBLIC_VAULT_CONTRACT_ID');

export const server = new rpc.Server(RPC_URL, {
  allowHttp: process.env.NEXT_PUBLIC_E2E_TESTS === 'true',
});
export const networkPassphrase = NETWORK_PASSPHRASE;

export async function simulateContractCall(
  method: string,
  args: xdr.ScVal[] = [],
  callerAddress: string,
): Promise<unknown> {
  const testHarness = getBrowserTestHarness();
  if (testHarness) return testHarness.simulateContractCall(method);

  const account = new Account(callerAddress, '0');
  const contract = new Contract(VAULT_CONTRACT_ID);
  const tx = new TransactionBuilder(account, {
    fee: '100',
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(contract.call(method, ...args))
    .setTimeout(30)
    .build();

  const sim = await server.simulateTransaction(tx);
  if (!rpc.Api.isSimulationSuccess(sim) || !sim.result?.retval) {
    throw new Error(`Simulation failed for ${method}: ${'error' in sim ? sim.error : 'No retval'}`);
  }
  return scValToNative(sim.result.retval);
}

export async function fetchVaultState(userAddress?: string): Promise<VaultStateResult> {
  if (!userAddress) {
    return { ok: true, state: { ...EMPTY_VAULT_STATE } };
  }

  try {
    const userScVal = new Address(userAddress).toScVal();

    const [balanceRes, strategyRes, exchangeRateRes, totalAssetsRes, totalSharesRes] = await Promise.allSettled([
      simulateContractCall('get_balance', [userScVal], userAddress),
      simulateContractCall('get_user_strategy', [userScVal], userAddress),
      simulateContractCall('get_exchange_rate', [], userAddress),
      simulateContractCall('get_total_assets', [], userAddress),
      simulateContractCall('get_total_shares', [], userAddress),
    ]);

    const failedCall = [balanceRes, strategyRes, exchangeRateRes, totalAssetsRes, totalSharesRes]
      .find((result) => result.status === 'rejected');
    if (failedCall?.status === 'rejected') {
      throw failedCall.reason;
    }

    return { ok: true, state: deriveVaultState({
      balance: balanceRes.status === 'fulfilled' ? Number(balanceRes.value) / 1e7 : 0,
      rawStrategy: strategyRes.status === 'fulfilled' ? String(strategyRes.value) : '',
      exchangeRateRaw: exchangeRateRes.status === 'fulfilled' ? Number(exchangeRateRes.value) / 1e7 : 0,
      totalAssets: totalAssetsRes.status === 'fulfilled' ? Number(totalAssetsRes.value) / 1e7 : 0,
      totalShares: totalSharesRes.status === 'fulfilled' ? Number(totalSharesRes.value) / 1e7 : 0,
    }) };
  } catch (err) {
    console.warn('Failed to fetch vault state from Soroban RPC:', err);
    return {
      ok: false,
      error: err instanceof Error ? err.message : 'Unable to fetch vault state from Soroban RPC.',
    };
  }
}

export function shortenAddress(address: string, chars = 4): string {
  return shortenSharedAddress(address, chars);
}

export async function setUserStrategy(
  userAddress: string,
  strategy: 'Conservative' | 'Balanced' | 'Growth',
): Promise<{ ok: boolean; hash?: string; error?: string }> {
  const testHarness = getBrowserTestHarness();
  const rawStrategy = strategy.toLowerCase();

  try {
    if (testHarness) {
      return { ok: true, hash: 'test-strategy-hash' };
    }

    const account = await server.getAccount(userAddress);
    const contract = new Contract(VAULT_CONTRACT_ID);
    const tx = new TransactionBuilder(account, {
      fee: '100',
      networkPassphrase: NETWORK_PASSPHRASE,
    })
      .addOperation(
        contract.call(
          'set_user_strategy',
          new Address(userAddress).toScVal(),
          nativeToScVal(rawStrategy, { type: 'symbol' }),
        ),
      )
      .setTimeout(300)
      .build();

    const prepared = await server.prepareTransaction(tx);
    const signedXdr = await signWithFreighter(prepared.toXDR(), NETWORK_PASSPHRASE);
    const sendResult = await server.sendTransaction(
      TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE),
    );

    if (sendResult.status === 'ERROR' || sendResult.status === 'TRY_AGAIN_LATER') {
      return { ok: false, error: 'Transaction failed to submit to the Stellar network.' };
    }

    const confirmation = await server.pollTransaction(sendResult.hash, { attempts: 10 });
    if (confirmation.status === 'SUCCESS') {
      return { ok: true, hash: sendResult.hash };
    }
    return { ok: false, error: 'Transaction execution failed on-chain.' };
  } catch (err) {
    const kind = classifyWalletError(err);
    if (kind === 'user_rejected') {
      return { ok: false, error: 'Transaction signature was rejected.' };
    }
    return {
      ok: false,
      error: err instanceof Error ? err.message : 'Failed to update strategy.',
    };
  }
}