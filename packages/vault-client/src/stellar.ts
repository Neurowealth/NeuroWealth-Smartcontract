/**
 * Shared Stellar vault helpers used by the web app and the agent.
 *
 * Kept free of Next.js and browser-test imports so the backend can depend on
 * this package instead of `frontend/src/lib/stellar`.
 */

export interface VaultState {
  balance: number;
  strategy: 'Conservative' | 'Balanced' | 'Growth';
  exchangeRate: number;
  apy: number;
}

export const EMPTY_VAULT_STATE: VaultState = {
  balance: 0,
  strategy: 'Balanced',
  exchangeRate: 1.0,
  apy: 0,
};

const STRATEGY_MAP: Record<string, VaultState['strategy']> = {
  conservative: 'Conservative',
  balanced: 'Balanced',
  growth: 'Growth',
};

export function mapVaultStrategy(raw: string): VaultState['strategy'] {
  return STRATEGY_MAP[raw.toLowerCase()] || 'Balanced';
}

export function shortenAddress(address: string, chars = 4): string {
  if (!address) return '';
  return `${address.substring(0, chars + 2)}...${address.substring(address.length - chars)}`;
}

export interface VaultStateReadings {
  /** User balance in whole USDC (stroops already scaled). */
  balance: number;
  rawStrategy: string;
  /** Exchange rate in whole units (stroops already scaled). 0 means unset. */
  exchangeRateRaw: number;
  totalAssets: number;
  totalShares: number;
}

/** Maps contract readings into the vault state both clients display. */
export function deriveVaultState(readings: VaultStateReadings): VaultState {
  const exchangeRate = readings.exchangeRateRaw > 0
    ? readings.exchangeRateRaw
    : (readings.totalShares > 0 ? readings.totalAssets / readings.totalShares : 1.0);
  const apy = readings.totalAssets > 0 && readings.totalShares > 0
    ? Number((((readings.totalAssets / readings.totalShares - 1) * 365 * 100).toFixed(2)))
    : 0;

  return {
    balance: readings.balance,
    strategy: mapVaultStrategy(readings.rawStrategy),
    exchangeRate,
    apy,
  };
}
