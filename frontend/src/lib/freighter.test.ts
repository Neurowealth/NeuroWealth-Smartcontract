import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { isConnectedMock, getPublicKeyMock, signTransactionMock, getNetworkDetailsMock } = vi.hoisted(() => ({
  isConnectedMock: vi.fn(),
  getPublicKeyMock: vi.fn(),
  signTransactionMock: vi.fn(),
  getNetworkDetailsMock: vi.fn(),
}));

vi.mock('@stellar/freighter-api', () => ({
  isConnected: isConnectedMock,
  getPublicKey: getPublicKeyMock,
  signTransaction: signTransactionMock,
  getNetworkDetails: getNetworkDetailsMock,
}));

import {
  classifyWalletError,
  connectFreighterWallet,
  isFreighterInstalled,
  signWithFreighter,
  WalletSigningError,
} from './freighter';

describe('classifyWalletError', () => {
  it.each([
    ['User declined access', 'user_rejected'],
    ['User rejected the request', 'user_rejected'],
    ['Request was cancelled', 'user_rejected'],
    ['Freighter is connected to the wrong network. Expected: TESTNET', 'wrong_network'],
    ['Network passphrase is not configured in the environment.', 'wrong_network'],
    ['Freighter is not allowed to access this account', 'wallet_disconnected'],
    ['Freighter is not connected', 'wallet_disconnected'],
    ['Something exploded', 'unknown'],
  ] as const)('classifies "%s" as %s', (message, expectedKind) => {
    expect(classifyWalletError(new Error(message))).toBe(expectedKind);
  });
});

describe('isFreighterInstalled', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns true when the extension reports connected', async () => {
    isConnectedMock.mockResolvedValue(true);
    await expect(isFreighterInstalled()).resolves.toBe(true);
  });

  it('returns false when the underlying check throws', async () => {
    isConnectedMock.mockRejectedValue(new Error('not installed'));
    await expect(isFreighterInstalled()).resolves.toBe(false);
  });
});

describe('connectFreighterWallet', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns the public key when Freighter is installed and connects', async () => {
    isConnectedMock.mockResolvedValue(true);
    getPublicKeyMock.mockResolvedValue('GABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890');

    await expect(connectFreighterWallet()).resolves.toBe('GABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890');
  });

  it('alerts and returns null when Freighter is not installed', async () => {
    isConnectedMock.mockResolvedValue(false);
    const alertSpy = vi.spyOn(window, 'alert').mockImplementation(() => {});

    await expect(connectFreighterWallet()).resolves.toBeNull();
    expect(alertSpy).toHaveBeenCalledOnce();
    expect(getPublicKeyMock).not.toHaveBeenCalled();
  });

  it('returns null when getPublicKey throws', async () => {
    isConnectedMock.mockResolvedValue(true);
    getPublicKeyMock.mockRejectedValue(new Error('user rejected'));

    await expect(connectFreighterWallet()).resolves.toBeNull();
  });

  it('returns null when getPublicKey resolves with an empty key', async () => {
    isConnectedMock.mockResolvedValue(true);
    getPublicKeyMock.mockResolvedValue('');

    await expect(connectFreighterWallet()).resolves.toBeNull();
  });
});

describe('signWithFreighter', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    vi.resetAllMocks();
    process.env = { ...originalEnv };
  });

  afterEach(() => {
    process.env = originalEnv;
  });

  it('rejects with a wrong_network error if the required passphrase is not configured', async () => {
    delete process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE;
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const error = await signWithFreighter('xdr_string').catch((e) => e);

    expect(error).toBeInstanceOf(WalletSigningError);
    expect(error.kind).toBe('wrong_network');
    expect(error.message).toBe('Network passphrase is not configured in the environment.');
    expect(consoleErrorSpy).toHaveBeenCalledWith('Wallet signing failed:', expect.any(Error));

    consoleErrorSpy.mockRestore();
  });

  it('rejects with a wrong_network error if Freighter reports a mismatched network', async () => {
    process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = 'Expected Network Passphrase';
    getNetworkDetailsMock.mockResolvedValue({
      network: 'PUBLIC',
      networkUrl: 'https://horizon.stellar.org',
      networkPassphrase: 'Wrong Network Passphrase',
    });
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const error = await signWithFreighter('xdr_string').catch((e) => e);

    expect(error).toBeInstanceOf(WalletSigningError);
    expect(error.kind).toBe('wrong_network');
    expect(error.message).toContain('Freighter is connected to the wrong network');

    consoleErrorSpy.mockRestore();
  });

  it('rejects with a user_rejected error when the user declines the signature request', async () => {
    const expectedPassphrase = 'Expected Network Passphrase';
    process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = expectedPassphrase;
    getNetworkDetailsMock.mockResolvedValue({
      network: 'TESTNET',
      networkUrl: 'https://horizon-testnet.stellar.org',
      networkPassphrase: expectedPassphrase,
    });
    signTransactionMock.mockRejectedValue(new Error('User declined access'));
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const error = await signWithFreighter('xdr_string').catch((e) => e);

    expect(error).toBeInstanceOf(WalletSigningError);
    expect(error.kind).toBe('user_rejected');

    consoleErrorSpy.mockRestore();
  });

  it('rejects with a wallet_disconnected error when Freighter has no active permission', async () => {
    const expectedPassphrase = 'Expected Network Passphrase';
    process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = expectedPassphrase;
    getNetworkDetailsMock.mockResolvedValue({
      network: 'TESTNET',
      networkUrl: 'https://horizon-testnet.stellar.org',
      networkPassphrase: expectedPassphrase,
    });
    signTransactionMock.mockRejectedValue(
      new Error('Freighter is not allowed to access this account'),
    );
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    const error = await signWithFreighter('xdr_string').catch((e) => e);

    expect(error).toBeInstanceOf(WalletSigningError);
    expect(error.kind).toBe('wallet_disconnected');

    consoleErrorSpy.mockRestore();
  });

  it('signs successfully when passphrase matches', async () => {
    const expectedPassphrase = 'Expected Network Passphrase';
    process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = expectedPassphrase;
    getNetworkDetailsMock.mockResolvedValue({
      network: 'TESTNET',
      networkUrl: 'https://horizon-testnet.stellar.org',
      networkPassphrase: expectedPassphrase,
    });
    signTransactionMock.mockResolvedValue('signed_xdr');

    const result = await signWithFreighter('xdr_string');
    expect(result).toBe('signed_xdr');
    expect(signTransactionMock).toHaveBeenCalledWith('xdr_string', {
      networkPassphrase: expectedPassphrase,
    });
  });

  it('passes through a custom network passphrase', async () => {
    const customPassphrase = 'Public Global Stellar Network ; September 2015';
    getNetworkDetailsMock.mockResolvedValue({
      network: 'PUBLIC',
      networkUrl: 'https://horizon.stellar.org',
      networkPassphrase: customPassphrase,
    });
    signTransactionMock.mockResolvedValue('signed-xdr');

    await signWithFreighter('raw-xdr', customPassphrase);

    expect(signTransactionMock).toHaveBeenCalledWith('raw-xdr', {
      networkPassphrase: customPassphrase,
    });
  });
});
