"use client";

import { useState, useEffect } from "react";
import { VaultClient } from "@neurowealth/vault-client";
import { useStore } from "@/lib/store";
import { getSorobanServer } from "@/lib/stellar";
import { Network } from "lucide-react";
import * as StellarSdk from "@stellar/stellar-sdk";

interface PoolConfigurationProps {
  client: VaultClient;
  signer: StellarSdk.Keypair;
}

export function PoolConfiguration({ client, signer }: PoolConfigurationProps) {
  const { network } = useStore();
  const [blendPool, setBlendPool] = useState<string | null>(null);
  const [dexPool, setDexPool] = useState<string | null>(null);
  const [newBlendPool, setNewBlendPool] = useState("");
  const [newDexPool, setNewDexPool] = useState("");

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [txHash, setTxHash] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"blend" | "dex">("blend");

  useEffect(() => {
    fetchPools();
  }, [client]);

  const fetchPools = async () => {
    try {
      const [blend, dex] = await Promise.all([
        client.get_blend_pool(signer.publicKey()),
        client.get_dex_pool(signer.publicKey()),
      ]);

      setBlendPool(blend);
      setDexPool(dex);
      setNewBlendPool(blend || "");
      setNewDexPool(dex || "");
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to fetch pool addresses",
      );
    }
  };

  const handleSetBlendPool = async () => {
    if (!newBlendPool.startsWith("C")) {
      setError("Invalid Soroban contract address (must start with C)");
      return;
    }

    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.set_blend_pool(
        signer as any,
        signer.publicKey(),
        newBlendPool,
      );
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);
      setBlendPool(newBlendPool);

      // Poll for confirmation
      const server = getSorobanServer(network);
      const txHash = result?.hash;
      if (!txHash) {
        throw new Error("Transaction hash missing");
      }
      for (let i = 0; i < 60; i++) {
        try {
          const tx = await server.getTransaction(txHash);
          if (tx.status === "SUCCESS") break;
        } catch {
          // Not confirmed yet
        }
        await new Promise((r) => setTimeout(r, 1000));
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to set Blend pool");
    } finally {
      setLoading(false);
    }
  };

  const handleSetDexPool = async () => {
    if (!newDexPool.startsWith("C")) {
      setError("Invalid Soroban contract address (must start with C)");
      return;
    }

    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.set_dex_pool(
        signer as any,
        signer.publicKey(),
        newDexPool,
      );
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);
      setDexPool(newDexPool);

      // Poll for confirmation
      const server = getSorobanServer(network);
      const txHash = result?.hash;
      if (!txHash) {
        throw new Error("Transaction hash missing");
      }
      for (let i = 0; i < 60; i++) {
        try {
          const tx = await server.getTransaction(txHash);
          if (tx.status === "SUCCESS") break;
        } catch {
          // Not confirmed yet
        }
        await new Promise((r) => setTimeout(r, 1000));
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to set DEX pool");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="card">
      <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
        <Network size={24} className="text-emerald-400" />
        Protocol Pool Configuration
      </h3>

      <div className="flex gap-2 mb-6 border-b border-slate-700">
        {(["blend", "dex"] as const).map((tab) => (
          <button
            key={tab}
            onClick={() => setActiveTab(tab)}
            className={`px-4 py-2 border-b-2 transition-colors ${
              activeTab === tab
                ? "border-emerald-500 text-emerald-400"
                : "border-transparent text-slate-400 hover:text-slate-300"
            }`}
          >
            {tab === "blend" && "Blend Pool"}
            {tab === "dex" && "DEX Pool"}
          </button>
        ))}
      </div>

      {activeTab === "blend" && (
        <div className="space-y-4">
          <div className="bg-slate-800/50 p-4 rounded">
            <div className="text-sm text-slate-400 mb-1">
              Current Blend Pool Address
            </div>
            {blendPool ? (
              <div className="text-sm font-mono text-emerald-400 break-all">
                {blendPool}
              </div>
            ) : (
              <div className="text-sm text-slate-500">Not configured</div>
            )}
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">
              New Blend Pool Address
            </label>
            <textarea
              value={newBlendPool}
              onChange={(e) => setNewBlendPool(e.target.value)}
              className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded text-slate-100 font-mono text-sm"
              rows={2}
              placeholder="Enter Soroban contract address (starts with C...)"
            />
            <p className="text-xs text-slate-500 mt-1">
              Blend protocol address for lending strategy deployment
            </p>
          </div>

          <button
            onClick={handleSetBlendPool}
            disabled={loading}
            className="button-primary w-full py-3 disabled:opacity-50"
          >
            {loading ? "Updating..." : "Update Blend Pool"}
          </button>
        </div>
      )}

      {activeTab === "dex" && (
        <div className="space-y-4">
          <div className="bg-slate-800/50 p-4 rounded">
            <div className="text-sm text-slate-400 mb-1">
              Current DEX Pool Address
            </div>
            {dexPool ? (
              <div className="text-sm font-mono text-emerald-400 break-all">
                {dexPool}
              </div>
            ) : (
              <div className="text-sm text-slate-500">Not configured</div>
            )}
          </div>

          <div>
            <label className="block text-sm font-medium mb-2">
              New DEX Pool Address
            </label>
            <textarea
              value={newDexPool}
              onChange={(e) => setNewDexPool(e.target.value)}
              className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded text-slate-100 font-mono text-sm"
              rows={2}
              placeholder="Enter Soroban contract address (starts with C...)"
            />
            <p className="text-xs text-slate-500 mt-1">
              Stellar DEX pool address for liquidity provision strategy
            </p>
          </div>

          <button
            onClick={handleSetDexPool}
            disabled={loading}
            className="button-primary w-full py-3 disabled:opacity-50"
          >
            {loading ? "Updating..." : "Update DEX Pool"}
          </button>
        </div>
      )}

      {txHash && (
        <div className="mt-4 p-3 bg-emerald-900/20 border border-emerald-700 rounded text-sm text-emerald-300">
          <p className="font-mono break-all">Tx: {txHash}</p>
        </div>
      )}

      {error && (
        <div className="mt-4 p-3 bg-red-900/20 border border-red-700 rounded text-sm text-red-300">
          {error}
        </div>
      )}
    </div>
  );
}
