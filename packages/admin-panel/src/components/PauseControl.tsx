"use client";

import { useState, useEffect } from "react";
import { VaultClient } from "@neurowealth/vault-client";
import { useStore } from "@/lib/store";
import { getSorobanServer, NETWORKS } from "@/lib/stellar";
import { AlertTriangle, Play, Pause } from "lucide-react";
import * as StellarSdk from "@stellar/stellar-sdk";

interface PauseControlProps {
  client: VaultClient;
  signer: StellarSdk.Keypair;
}

export function PauseControl({ client, signer }: PauseControlProps) {
  const { network } = useStore();
  const [paused, setPaused] = useState<boolean | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showConfirm, setShowConfirm] = useState(false);
  const [txHash, setTxHash] = useState<string | null>(null);

  useEffect(() => {
    fetchPauseStatus();
  }, [client]);

  const fetchPauseStatus = async () => {
    try {
      const status = await client.is_paused(signer.publicKey());
      setPaused(status);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to fetch pause status",
      );
    }
  };

  const handleTogglePause = async () => {
    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      let hash: string | undefined;
      if (paused) {
        // Unpause
        const result = await client.unpause(signer as any, signer.publicKey());
        hash = result?.hash;
      } else {
        // Pause
        const result = await client.pause(signer as any, signer.publicKey());
        hash = result?.hash;
      }

      if (!hash) {
        throw new Error("Transaction did not return a hash");
      }
      setTxHash(hash);
      setPaused(!paused);
      setShowConfirm(false);

      // Poll for confirmation
      const server = getSorobanServer(network);
      let confirmed = false;
      for (let i = 0; i < 60; i++) {
        try {
          const tx = await server.getTransaction(hash);
          if (tx.status === "SUCCESS") {
            confirmed = true;
            break;
          }
        } catch {
          // Not confirmed yet
        }
        await new Promise((r) => setTimeout(r, 1000));
      }

      if (!confirmed) {
        setError("Transaction not confirmed within timeout");
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to toggle pause state",
      );
      setShowConfirm(false);
    } finally {
      setLoading(false);
    }
  };

  if (paused === null) {
    return (
      <div className="card">
        <div className="text-center py-4">Loading pause status...</div>
      </div>
    );
  }

  return (
    <div
      className={`card border-2 ${paused ? "border-red-600" : "border-emerald-600"}`}
    >
      <div className="flex items-start justify-between mb-6">
        <div>
          <h3 className="text-xl font-bold flex items-center gap-2">
            {paused ? (
              <>
                <Pause className="text-red-500" size={24} />
                Vault is Paused
              </>
            ) : (
              <>
                <Play className="text-emerald-500" size={24} />
                Vault is Active
              </>
            )}
          </h3>
          <p className="text-sm text-slate-400 mt-1">
            {paused
              ? "All deposits and withdrawals are blocked"
              : "Deposits and withdrawals are allowed"}
          </p>
        </div>
        <div
          className={`px-3 py-1 rounded text-xs font-semibold ${
            paused
              ? "bg-red-900/30 text-red-400"
              : "bg-emerald-900/30 text-emerald-400"
          }`}
        >
          {paused ? "PAUSED" : "ACTIVE"}
        </div>
      </div>

      {showConfirm ? (
        <div className="bg-slate-800/50 border border-slate-700 rounded p-4 mb-4">
          <div className="flex items-start gap-3 mb-4">
            <AlertTriangle
              className="text-yellow-500 flex-shrink-0 mt-1"
              size={20}
            />
            <div>
              <p className="font-semibold">Confirm Action</p>
              <p className="text-sm text-slate-300 mt-1">
                {paused
                  ? "This will unpause the vault and allow deposits/withdrawals. Are you sure?"
                  : "This will pause the vault and block all deposits/withdrawals. This is usually for emergencies. Are you sure?"}
              </p>
            </div>
          </div>
          <div className="flex gap-2">
            <button
              onClick={handleTogglePause}
              disabled={loading}
              className={`px-4 py-2 rounded font-medium text-white ${
                paused
                  ? "bg-emerald-600 hover:bg-emerald-700 disabled:opacity-50"
                  : "bg-red-600 hover:bg-red-700 disabled:opacity-50"
              }`}
            >
              {loading ? "Processing..." : paused ? "Unpause" : "Pause"}
            </button>
            <button
              onClick={() => setShowConfirm(false)}
              disabled={loading}
              className="button-secondary px-4 py-2"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <button
          onClick={() => setShowConfirm(true)}
          disabled={loading}
          className={`w-full py-3 rounded font-semibold text-white transition-colors ${
            paused
              ? "bg-emerald-600 hover:bg-emerald-700 disabled:opacity-50"
              : "bg-red-600 hover:bg-red-700 disabled:opacity-50"
          }`}
        >
          {paused ? "Unpause Vault" : "Emergency Pause"}
        </button>
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
