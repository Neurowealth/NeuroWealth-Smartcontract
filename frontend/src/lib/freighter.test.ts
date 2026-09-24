import { afterEach, describe, expect, it, vi } from 'vitest';

const { isConnectedMock, getPublicKeyMock, signTransactionMock } = vi.hoisted(() => ({
  isConnectedMock: vi.fn(),
  getPublicKeyMock: vi.fn(),
  signTransactionMock: vi.fn(),
}));

vi.mock('@stellar/freighter-api', () => ({
  isConnected: isConnectedMock,
  getPublicKey: getPublicKeyMock,
  signTransaction: signTransactionMock,
}));

import { connectFreighterWallet, isFreighterInstalled, signWithFreighter } from './freighter';

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
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns the signed XDR on success', async () => {
    signTransactionMock.mockResolvedValue('signed-xdr');

    await expect(signWithFreighter('raw-xdr')).resolves.toBe('signed-xdr');
    expect(signTransactionMock).toHaveBeenCalledWith('raw-xdr', {
      networkPassphrase: 'Test SDF Network ; September 2015',
    });
  });

  it('passes through a custom network passphrase', async () => {
    signTransactionMock.mockResolvedValue('signed-xdr');

    await signWithFreighter('raw-xdr', 'Public Global Stellar Network ; September 2015');

    expect(signTransactionMock).toHaveBeenCalledWith('raw-xdr', {
      networkPassphrase: 'Public Global Stellar Network ; September 2015',
    });
  });

  it('returns null when signing fails or is rejected', async () => {
    signTransactionMock.mockRejectedValue(new Error('user rejected'));

    await expect(signWithFreighter('raw-xdr')).resolves.toBeNull();
  });
});
