import {
  isConnected as checkFreighterConnected,
  getPublicKey as getFreighterPublicKey,
  signTransaction as signFreighterTx,
  getNetworkDetails
} from '@stellar/freighter-api';

export interface FreighterWalletState {
  isConnected: boolean;
  publicKey: string | null;
  error: string | null;
}

/**
 * Wallet-layer failure classes. These are normal, recoverable states that must not be
 * conflated with an on-chain transaction failure.
 */
export type WalletErrorKind = 'user_rejected' | 'wrong_network' | 'wallet_disconnected' | 'unknown';

export class WalletSigningError extends Error {
  readonly kind: WalletErrorKind;

  constructor(kind: WalletErrorKind, message: string) {
    super(message);
    this.name = 'WalletSigningError';
    this.kind = kind;
  }
}

/**
 * Classifies an error raised by the Freighter extension (or our own pre-flight checks)
 * into one of a small set of recoverable wallet-error classes, based on the wording
 * Freighter is known to use for each case.
 */
export function classifyWalletError(err: unknown): WalletErrorKind {
  const message = err instanceof Error ? err.message : String(err ?? '');
  const normalized = message.toLowerCase();

  if (normalized.includes('wrong network') || normalized.includes('network passphrase')) {
    return 'wrong_network';
  }
  if (
    normalized.includes('declin') ||
    normalized.includes('reject') ||
    normalized.includes('denied') ||
    normalized.includes('cancel')
  ) {
    return 'user_rejected';
  }
  if (
    normalized.includes('not connected') ||
    normalized.includes('not installed') ||
    normalized.includes('not allowed') ||
    normalized.includes('no public key') ||
    normalized.includes('disconnected') ||
    normalized.includes('permission')
  ) {
    return 'wallet_disconnected';
  }
  return 'unknown';
}

/**
 * Checks if Freighter extension is installed in the user's browser.
 */
export async function isFreighterInstalled(): Promise<boolean> {
  try {
    return await checkFreighterConnected();
  } catch (err) {
    return false;
  }
}

/**
 * Requests wallet connection from Freighter and returns the active public key.
 * Non-custodial signing model - private keys never enter web app.
 */
export async function connectFreighterWallet(): Promise<string | null> {
  try {
    const installed = await isFreighterInstalled();
    if (!installed) {
      alert('Freighter wallet extension is not installed. Please install Freighter to connect.');
      return null;
    }

    const key = await getFreighterPublicKey();
    return key || null;
  } catch (err: any) {
    console.error('Failed to connect Freighter wallet:', err);
    return null;
  }
}

/**
 * Signs a XDR transaction string using Freighter extension.
 *
 * On failure this rejects with a `WalletSigningError` carrying a classified `kind`
 * (user rejection, wrong network, or a disconnected wallet) so callers can render
 * recovery guidance appropriate to each case instead of a generic failure.
 */
export async function signWithFreighter(xdr: string, networkPassphrase?: string): Promise<string> {
  try {
    const requiredPassphrase = networkPassphrase || process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE;

    if (!requiredPassphrase) {
      throw new Error('Network passphrase is not configured in the environment.');
    }

    const networkDetails = await getNetworkDetails();
    if (networkDetails.networkPassphrase !== requiredPassphrase) {
      throw new Error(`Freighter is connected to the wrong network. Expected network passphrase: ${requiredPassphrase}`);
    }

    const signedXdr = await signFreighterTx(xdr, {
      networkPassphrase: requiredPassphrase
    });
    return signedXdr;
  } catch (err) {
    console.error('Wallet signing failed:', err);
    const kind = classifyWalletError(err);
    const message = err instanceof Error ? err.message : 'Failed to sign transaction with Freighter.';
    throw new WalletSigningError(kind, message);
  }
}
