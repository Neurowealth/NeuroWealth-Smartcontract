import * as StellarSdk from "@stellar/stellar-sdk";

// Network configuration
export const NETWORKS = {
  testnet: {
    network: "TESTNET",
    networkPassphrase: StellarSdk.Networks.TESTNET,
    rpcUrl: "https://soroban-testnet.stellar.org",
    horizonUrl: "https://horizon-testnet.stellar.org",
  },
  mainnet: {
    network: "MAINNET",
    networkPassphrase: StellarSdk.Networks.PUBLIC,
    rpcUrl: "https://soroban.stellar.org",
    horizonUrl: "https://horizon.stellar.org",
  },
  devnet: {
    network: "DEVNET",
    networkPassphrase: StellarSdk.Networks.STANDALONE,
    rpcUrl: "http://localhost:8000/soroban/rpc",
    horizonUrl: "http://localhost:8000",
  },
};

export type NetworkType = keyof typeof NETWORKS;

// Freighter wallet helper
export async function connectFreighter(network: NetworkType) {
  if (!window.freighter) {
    throw new Error("Freighter wallet not installed");
  }

  try {
    const isAllowed = await window.freighter.isAllowed();
    if (!isAllowed) {
      throw new Error("Freighter access not allowed");
    }

    const publicKey = await window.freighter.getPublicKey();
    return {
      publicKey,
      isConnected: true,
    };
  } catch (error) {
    throw new Error(`Failed to connect Freighter: ${error}`);
  }
}

// Sign transaction with Freighter
export async function signWithFreighter(xdr: string, network: NetworkType) {
  if (!window.freighter) {
    throw new Error("Freighter wallet not installed");
  }

  try {
    const signedXdr = await window.freighter.signTransaction(
      xdr,
      NETWORKS[network].networkPassphrase,
    );
    return signedXdr;
  } catch (error) {
    throw new Error(`Failed to sign transaction: ${error}`);
  }
}

// Soroban RPC client
export function getSorobanServer(network: NetworkType) {
  return new StellarSdk.SorobanRpc.Server(NETWORKS[network].rpcUrl);
}

// Type definitions for Freighter
declare global {
  interface Window {
    freighter?: {
      isAllowed(): Promise<boolean>;
      getPublicKey(): Promise<string>;
      signTransaction(xdr: string, networkPassphrase: string): Promise<string>;
    };
  }
}
