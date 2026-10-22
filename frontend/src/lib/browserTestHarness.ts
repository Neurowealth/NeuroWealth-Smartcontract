import { isMockModeEnabled } from './mockMode';

export interface BrowserTestHarness {
  connectWallet(): Promise<string | null>;
  getNetworkDetails(): Promise<{ networkPassphrase: string }>;
  signTransaction(xdr: string, networkPassphrase: string): Promise<string>;
  simulateContractCall(method: string): Promise<unknown>;
  prepareTransaction(input: { method: 'deposit' | 'withdraw'; publicKey: string; amount: string }): Promise<string>;
  sendTransaction(signedXdr: string): Promise<{ status: string; hash: string }>;
  pollTransaction(hash: string): Promise<{ status: string }>;
}

declare global {
  interface Window {
    __NEUROWEALTH_E2E__?: BrowserTestHarness;
  }
}

export function getBrowserTestHarness(): BrowserTestHarness | null {
  if (!isMockModeEnabled() || typeof window === 'undefined') return null;
  return window.__NEUROWEALTH_E2E__ ?? null;
}