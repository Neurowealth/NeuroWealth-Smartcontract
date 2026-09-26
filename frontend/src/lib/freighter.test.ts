import { describe, it, expect, vi, beforeEach } from 'vitest';
import { signWithFreighter, WalletSigningError, classifyWalletError } from './freighter';
import * as freighterApi from '@stellar/freighter-api';

vi.mock('@stellar/freighter-api', () => ({
  getNetworkDetails: vi.fn(),
  signTransaction: vi.fn(),
}));

describe('freighter', () => {
  const originalEnv = process.env;

  beforeEach(() => {
    vi.resetAllMocks();
    process.env = { ...originalEnv };
  });

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

  describe('signWithFreighter', () => {
    it('rejects with a wrong_network error if the required passphrase is not configured', async () => {
      delete process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE;
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      const error = await signWithFreighter('xdr_string').catch(e => e);

      expect(error).toBeInstanceOf(WalletSigningError);
      expect(error.kind).toBe('wrong_network');
      expect(error.message).toBe('Network passphrase is not configured in the environment.');
      expect(consoleErrorSpy).toHaveBeenCalledWith('Wallet signing failed:', expect.any(Error));

      consoleErrorSpy.mockRestore();
    });

    it('rejects with a wrong_network error if Freighter reports a mismatched network', async () => {
      process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = 'Expected Network Passphrase';
      vi.mocked(freighterApi.getNetworkDetails).mockResolvedValue({
        network: 'PUBLIC',
        networkUrl: 'https://horizon.stellar.org',
        networkPassphrase: 'Wrong Network Passphrase'
      });
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      const error = await signWithFreighter('xdr_string').catch(e => e);

      expect(error).toBeInstanceOf(WalletSigningError);
      expect(error.kind).toBe('wrong_network');
      expect(error.message).toContain('Freighter is connected to the wrong network');

      consoleErrorSpy.mockRestore();
    });

    it('rejects with a user_rejected error when the user declines the signature request', async () => {
      const expectedPassphrase = 'Expected Network Passphrase';
      process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = expectedPassphrase;
      vi.mocked(freighterApi.getNetworkDetails).mockResolvedValue({
        network: 'TESTNET',
        networkUrl: 'https://horizon-testnet.stellar.org',
        networkPassphrase: expectedPassphrase
      });
      vi.mocked(freighterApi.signTransaction).mockRejectedValue(new Error('User declined access'));
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      const error = await signWithFreighter('xdr_string').catch(e => e);

      expect(error).toBeInstanceOf(WalletSigningError);
      expect(error.kind).toBe('user_rejected');

      consoleErrorSpy.mockRestore();
    });

    it('rejects with a wallet_disconnected error when Freighter has no active permission', async () => {
      const expectedPassphrase = 'Expected Network Passphrase';
      process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = expectedPassphrase;
      vi.mocked(freighterApi.getNetworkDetails).mockResolvedValue({
        network: 'TESTNET',
        networkUrl: 'https://horizon-testnet.stellar.org',
        networkPassphrase: expectedPassphrase
      });
      vi.mocked(freighterApi.signTransaction).mockRejectedValue(
        new Error('Freighter is not allowed to access this account')
      );
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

      const error = await signWithFreighter('xdr_string').catch(e => e);

      expect(error).toBeInstanceOf(WalletSigningError);
      expect(error.kind).toBe('wallet_disconnected');

      consoleErrorSpy.mockRestore();
    });

    it('signs successfully when passphrase matches', async () => {
      const expectedPassphrase = 'Expected Network Passphrase';
      process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = expectedPassphrase;
      vi.mocked(freighterApi.getNetworkDetails).mockResolvedValue({
        network: 'TESTNET',
        networkUrl: 'https://horizon-testnet.stellar.org',
        networkPassphrase: expectedPassphrase
      });
      vi.mocked(freighterApi.signTransaction).mockResolvedValue('signed_xdr');

      const result = await signWithFreighter('xdr_string');
      expect(result).toBe('signed_xdr');
      expect(freighterApi.signTransaction).toHaveBeenCalledWith('xdr_string', {
        networkPassphrase: expectedPassphrase
      });
    });
  });
});
