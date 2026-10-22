import {
  isConnected as checkFreighterConnected,
  getPublicKey as getFreighterPublicKey,
  signTransaction as signFreighterTx,
  getNetworkDetails
} from '@stellar/freighter-api';
import { getBrowserTestHarness } from './browserTestHarness';

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
  const testHarness = getBrowserTestHarness();
  if (testHarness) return testHarness.connectWallet();

  try {
    const installed = await isFreighterInstalled();
    if (!installed) {
      alert('Freighter wallet extension is not installed. Please install Freighter to connect.');
      return null;
    }

    const key = await getFreighterPublicKey();
    return key || null;
  } catch (err: unknown) {
    console.error('Failed to connect Freighter wallet:', err);
    return null;
  }

}

const KNOWN_NETWORKS: Record<string, string> = {
  'Public Global Stellar Network ; September 2015': 'Mainnet',
  'Test SDF Network ; September 2015': 'Testnet',
  'Test SDF Future Network ; October 2022': 'Futurenet',
};

/** Human-readable name for a Stellar network passphrase. */
export function describeNetwork(passphrase: string | null | undefined): string {
  if (!passphrase) return 'an unknown network';
  return KNOWN_NETWORKS[passphrase] ?? 'a custom network';
}

/**
 * Reads the network Freighter is currently using and throws unless it matches
 * `requiredPassphrase`. Must run before any signature is requested so a wallet
 * on Testnet can never sign a Mainnet transaction (or vice versa).
 */
export async function assertFreighterNetwork(requiredPassphrase: string): Promise<void> {
  const testHarness = getBrowserTestHarness();
  const networkDetails: { networkPassphrase?: string; error?: string } = testHarness
    ? await testHarness.getNetworkDetails()
    : await getNetworkDetails();

  const activePassphrase = networkDetails?.networkPassphrase;
  if (!activePassphrase || networkDetails?.error) {
    throw new Error('Freighter is not connected: unable to read its active network.');
  }

  if (activePassphrase !== requiredPassphrase) {
    throw new Error(
      `Freighter is connected to the wrong network: the wallet is on ${describeNetwork(activePassphrase)} ` +
        `but this app requires ${describeNetwork(requiredPassphrase)}. ` +
        `Switch networks in Freighter and try again. Expected network passphrase: ${requiredPassphrase}`,
    );
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

    // Block network mismatches before Freighter is ever asked for a signature.
    await assertFreighterNetwork(requiredPassphrase);

    const testHarness = getBrowserTestHarness();
    const signedXdr = testHarness
      ? await testHarness.signTransaction(xdr, requiredPassphrase)
      : await signFreighterTx(xdr, { networkPassphrase: requiredPassphrase });
    return signedXdr;
  } catch (err) {
    console.error('Wallet signing failed:', err);
    const kind = classifyWalletError(err);
    const message = err instanceof Error ? err.message : 'Failed to sign transaction with Freighter.';
    throw new WalletSigningError(kind, message);
  }
}
