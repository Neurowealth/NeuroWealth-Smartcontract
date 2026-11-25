import { Keypair } from '@stellar/stellar-sdk';
import { VaultClient, VaultError, VaultErrorCode } from '@neurowealth/vault-client';
import { getWallet, getDecryptedSecretKey } from './walletService';
import { INPUT_LIMITS, SAFE_OVERSIZE_REPLY, boundText } from './inputBounds';
import { isValidStrategy, type Strategy } from './contractLimits';

function rejectUnusableAddress(publicKey: string): string | null {
  const checked = boundText(publicKey, INPUT_LIMITS.address);
  return checked.ok ? null : SAFE_OVERSIZE_REPLY;
}

function requireEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} must be set; refusing to use an unsafe vault contract default`);
  }
  return value;
}

/** USDC has 7 decimal places on this vault; see contractLimits.ts's own USDC_DECIMALS. */
const USDC_DECIMALS = 7;
const STROOPS_PER_USDC = 10 ** USDC_DECIMALS;

function toStroops(usdcAmount: number): bigint {
  return BigInt(Math.round(usdcAmount * STROOPS_PER_USDC));
}

function fromStroops(stroops: bigint): number {
  return Number(stroops) / STROOPS_PER_USDC;
}

function vaultClient(): VaultClient {
  return new VaultClient({
    contractId: requireEnv('VAULT_CONTRACT_ID'),
    rpcUrl: requireEnv('SOROBAN_RPC_URL'),
  });
}

function toStrategySymbol(strategy: string): Strategy | null {
  const symbol = strategy.trim().toLowerCase();
  return isValidStrategy(symbol) ? symbol : null;
}

/**
 * Submit `set_user_strategy` and refuse to report success without the
 * transaction hash the vault client returns (#1107).
 */
async function applyUserStrategy(
  client: VaultClient,
  signer: Keypair,
  publicKey: string,
  strategy: string,
): Promise<{ ok: true; txHash: string; symbol: Strategy } | { ok: false; message: string }> {
  const symbol = toStrategySymbol(strategy);
  if (!symbol) {
    return { ok: false, message: 'Invalid strategy specified on-chain transaction failed.' };
  }
  const result = await client.set_user_strategy(signer, publicKey, symbol);
  if (!result.hash) {
    throw new Error('set_user_strategy: transaction did not return a hash');
  }
  return { ok: true, txHash: result.hash, symbol };
}

/**
 * Maps a VaultError raised by a deposit/withdraw invocation to a message
 * safe to show a WhatsApp user. Falls back to a generic message for any
 * code not explicitly handled here, so a new or unmapped contract error
 * never leaks a raw contract code or stack trace into chat (#1001).
 */
function friendlyVaultErrorMessage(err: unknown): string {
  if (err instanceof VaultError) {
    switch (err.code) {
      case VaultErrorCode.Paused:
        return 'The vault is temporarily paused for maintenance. Please try again shortly.';
      case VaultErrorCode.AmountMustBePositive:
        return 'The amount must be greater than 0.';
      case VaultErrorCode.BelowMinimumDeposit:
        return 'That deposit is below the minimum the vault currently accepts.';
      case VaultErrorCode.MaximumDepositExceeded:
        return 'That deposit is above the maximum the vault currently accepts in one transaction.';
      case VaultErrorCode.ExceedsUserDepositCap:
        return 'That would put you over your per-user deposit cap.';
      case VaultErrorCode.ExceedsTvlCap:
        return 'The vault is at its total deposit cap right now. Please try again later.';
      case VaultErrorCode.InsufficientShares:
      case VaultErrorCode.InsufficientSharesForAmount:
      case VaultErrorCode.NoSharesToWithdraw:
        return "You don't have enough in your vault position to withdraw that amount.";
      case VaultErrorCode.InsufficientLiquidity:
      case VaultErrorCode.NoLiquidityAvailable:
      case VaultErrorCode.NoAssetsToWithdraw:
        return 'The vault does not have enough liquid funds to process that withdrawal right now. Please try again shortly.';
      default:
        return 'The transaction was rejected by the vault contract. Please try again, or contact support if this continues.';
    }
  }
  return 'The transaction could not be submitted. Please try again shortly.';
}

interface LoadedSigner {
  publicKey: string;
  signer: Keypair;
}

/** Loads the caller's wallet and decrypted signing key, or an error reply if either step fails. */
async function loadSigner(phoneHash: string): Promise<LoadedSigner | { error: string }> {
  const wallet = await getWallet(phoneHash);
  if (!wallet) {
    return { error: 'Wallet not initialized.' };
  }
  const addressError = rejectUnusableAddress(wallet.publicKey);
  if (addressError) {
    return { error: addressError };
  }
  const secret = await getDecryptedSecretKey(phoneHash);
  if (!secret) {
    return { error: 'Wallet not initialized.' };
  }
  return { publicKey: wallet.publicKey, signer: Keypair.fromSecret(secret) };
}

export interface UserPortfolio {
  balance: number;
  usdEquivalent: number;
  strategy: string;
  apy: number;
  dailyEarnings: number;
}

const STRATEGY_MAP: Record<string, string> = {
  conservative: 'Conservative',
  balanced: 'Balanced',
  growth: 'Growth',
};

/**
 * Reads vault state and portfolio details for a verified WhatsApp user.
 */
export async function getPortfolio(phoneHash: string): Promise<UserPortfolio> {
  const wallet = await getWallet(phoneHash);
  if (!wallet) {
    throw new Error('Wallet not found');
  }

  try {
    const client = vaultClient();
    const sourcePublicKey = wallet.publicKey;
    const [balanceRaw, rawStrategy, userApyRaw] = await Promise.all([
      client.get_balance(wallet.publicKey, sourcePublicKey),
      client.get_user_strategy(wallet.publicKey, sourcePublicKey),
      client.get_user_apy(wallet.publicKey, sourcePublicKey).catch(() => 0n),
    ]);

    const balance = fromStroops(balanceRaw);
    const strategy = STRATEGY_MAP[String(rawStrategy)] || 'Balanced';
    const apy = Number((Number(userApyRaw) / 100).toFixed(2));

    // get_balance already returns the user's USDC asset value (1 USDC = $1.00 USD).
    const usdEquivalent = Number(balance.toFixed(2));
    const dailyEarnings = balance > 0 && apy > 0 ? (balance * (apy / 100)) / 365 : 0;

    return {
      balance,
      usdEquivalent,
      strategy,
      apy,
      dailyEarnings: Number(dailyEarnings.toFixed(4))
    };
  } catch (err) {
    console.warn('Failed to fetch vault state from Soroban RPC:', err);
    return {
      balance: 0,
      usdEquivalent: 0,
      strategy: 'Balanced',
      apy: 0,
      dailyEarnings: 0
    };
  }
}

/**
 * Handles a deposit for a verified WhatsApp user: builds, signs with the
 * decrypted custodial key, submits and polls a real `deposit` invocation on
 * the vault contract via @neurowealth/vault-client (#1001). No success
 * reply is ever sent without on-chain confirmation; a mapped, non-leaking
 * message is sent on failure.
 */
export async function handleDeposit(
  phoneHash: string,
  amount: number,
  strategy?: string
): Promise<{ success: boolean; txHash: string; message: string }> {
  const loaded = await loadSigner(phoneHash);
  if ('error' in loaded) {
    return { success: false, txHash: '', message: loaded.error };
  }

  const selectedStrategy = strategy || 'Balanced';

  try {
    const client = vaultClient();
    // The deposit invocation has no strategy argument. Persist the choice
    // with set_user_strategy before claiming the deposit succeeded (#1107).
    if (strategy) {
      const applied = await applyUserStrategy(client, loaded.signer, loaded.publicKey, strategy);
      if (!applied.ok) {
        return { success: false, txHash: '', message: applied.message };
      }
    }
    const result = await client.deposit(loaded.signer, loaded.publicKey, toStroops(amount));
    if (!result.hash) {
      throw new Error('deposit: transaction did not return a hash');
    }

    return {
      success: true,
      txHash: result.hash,
      message: `Deposited ${amount} USDC into your ${selectedStrategy} strategy.\nTransaction Hash: ${result.hash.substring(0, 10)}...\nConfirmed on Stellar.`
    };
  } catch (err) {
    console.warn('Deposit failed:', err);
    return { success: false, txHash: '', message: friendlyVaultErrorMessage(err) };
  }
}

/**
 * Handles a withdrawal for a verified WhatsApp user: builds, signs with the
 * decrypted custodial key, submits and polls a real `withdraw` or
 * `withdraw_all` invocation on the vault contract via
 * @neurowealth/vault-client (#1001). No success reply is ever sent without
 * on-chain confirmation; a mapped, non-leaking message is sent on failure.
 */
export async function handleWithdraw(
  phoneHash: string,
  amount?: number,
  withdrawAll?: boolean
): Promise<{ success: boolean; txHash: string; message: string }> {
  const loaded = await loadSigner(phoneHash);
  if ('error' in loaded) {
    return { success: false, txHash: '', message: loaded.error };
  }

  const withdrawAmountText = withdrawAll ? 'all funds' : `${amount} USDC`;

  try {
    const client = vaultClient();
    const result = withdrawAll
      ? await client.withdraw_all(loaded.signer, loaded.publicKey)
      : await client.withdraw(loaded.signer, loaded.publicKey, toStroops(amount as number));
    if (!result.hash) {
      throw new Error('withdraw: transaction did not return a hash');
    }

    return {
      success: true,
      txHash: result.hash,
      message: `Withdrew ${withdrawAmountText} from vault contract.\nTransaction Hash: ${result.hash.substring(0, 10)}...\nFunds sent directly to your wallet.`
    };
  } catch (err) {
    console.warn('Withdraw failed:', err);
    return { success: false, txHash: '', message: friendlyVaultErrorMessage(err) };
  }
}

/**
 * Updates the user's strategy on the vault contract via `set_user_strategy`.
 * Success is reported only after the client returns a transaction hash (#1107).
 */
export async function handleStrategyUpdate(
  phoneHash: string,
  strategy: string
): Promise<{ success: boolean; txHash: string; message: string }> {
  const loaded = await loadSigner(phoneHash);
  if ('error' in loaded) {
    return { success: false, txHash: '', message: loaded.error };
  }

  try {
    const client = vaultClient();
    const applied = await applyUserStrategy(client, loaded.signer, loaded.publicKey, strategy);
    if (!applied.ok) {
      return { success: false, txHash: '', message: applied.message };
    }

    return {
      success: true,
      txHash: applied.txHash,
      message: `Strategy updated to ${strategy.toUpperCase()}. The AI agent will rebalance your portfolio on the next scheduled run.\nTransaction Hash: ${applied.txHash.substring(0, 10)}...`
    };
  } catch (err) {
    console.warn('Strategy update failed:', err);
    return { success: false, txHash: '', message: friendlyVaultErrorMessage(err) };
  }
}
