import { Account, Address, Contract, rpc, scValToNative, TransactionBuilder, xdr } from '@stellar/stellar-sdk';

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

export const server = new rpc.Server(RPC_URL);
export const networkPassphrase = NETWORK_PASSPHRASE;

export interface VaultState {
  balance: number;
  strategy: 'Conservative' | 'Balanced' | 'Growth';
  exchangeRate: number;
  apy: number;
}

const STRATEGY_MAP: Record<string, VaultState['strategy']> = {
  conservative: 'Conservative',
  balanced: 'Balanced',
  growth: 'Growth',
};

export async function simulateContractCall(
  method: string,
  args: xdr.ScVal[] = [],
  callerAddress: string,
): Promise<unknown> {
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

export async function fetchVaultState(userAddress?: string): Promise<VaultState> {
  if (!userAddress) {
    return { balance: 0, strategy: 'Balanced', exchangeRate: 1.0, apy: 0 };
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

    const balance = balanceRes.status === 'fulfilled' ? Number(balanceRes.value) / 1e7 : 0;
    const rawStrategy = strategyRes.status === 'fulfilled' ? String(strategyRes.value).toLowerCase() : '';
    const strategy = STRATEGY_MAP[rawStrategy] || 'Balanced';
    const exchangeRateRaw = exchangeRateRes.status === 'fulfilled' ? Number(exchangeRateRes.value) / 1e7 : 0;
    const totalAssets = totalAssetsRes.status === 'fulfilled' ? Number(totalAssetsRes.value) / 1e7 : 0;
    const totalShares = totalSharesRes.status === 'fulfilled' ? Number(totalSharesRes.value) / 1e7 : 0;

    const exchangeRate = exchangeRateRaw > 0 ? exchangeRateRaw : (totalShares > 0 ? totalAssets / totalShares : 1.0);
    const apy = totalAssets > 0 && totalShares > 0
      ? Number((((totalAssets / totalShares - 1) * 365 * 100).toFixed(2)))
      : 0;

    return { balance, strategy, exchangeRate, apy };
  } catch (err) {
    console.warn('Failed to fetch vault state from Soroban RPC:', err);
    return { balance: 0, strategy: 'Balanced', exchangeRate: 1.0, apy: 0 };
  }
}

export function shortenAddress(address: string, chars = 4): string {
  if (!address) return '';
  return `${address.substring(0, chars + 2)}...${address.substring(address.length - chars)}`;
}