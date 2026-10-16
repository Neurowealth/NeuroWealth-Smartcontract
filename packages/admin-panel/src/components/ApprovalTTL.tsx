"use client";

import { useState, useEffect } from "react";
import { VaultClient } from "@neurowealth/vault-client";
import { useStore } from "@/lib/store";
import { getSorobanServer } from "@/lib/stellar";
import { Timer } from "lucide-react";
import * as StellarSdk from "@stellar/stellar-sdk";

interface ApprovalTTLProps {
  client: VaultClient;
  signer: StellarSdk.Keypair;
}

export function ApprovalTTL({ client, signer }: ApprovalTTLProps) {
  const { network } = useStore();
  const [ttl, setTtl] = useState<number | null>(null);
  const [newTtl, setNewTtl] = useState("");

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [txHash, setTxHash] = useState<string | null>(null);

  useEffect(() => {
    fetchTtl();
  }, [client]);

  const fetchTtl = async () => {
    try {
      const currentTtl = await client.get_approval_ttl(signer.publicKey());
      setTtl(currentTtl);
      setNewTtl(currentTtl.toString());
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to fetch approval TTL",
      );
    }
  };

  const handleSetTtl = async () => {
    const value = parseInt(newTtl);
    if (!newTtl || value < 1000 || value > 500000) {
      setError("TTL must be between 1,000 and 500,000 ledgers");
      return;
    }

    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.set_approval_ttl(signer, value);
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);
      setTtl(value);

      // Poll for confirmation
      const server = getSorobanServer(network);
      for (let i = 0; i < 60; i++) {
        try {
          const tx = await server.getTransaction(result.hash);
          if (tx.status === "SUCCESS") break;
        } catch {
          // Not confirmed yet
        }
        await new Promise((r) => setTimeout(r, 1000));
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to set approval TTL",
      );
    } finally {
      setLoading(false);
    }
  };

  const formatDays = (ledgers: number) => {
    const seconds = ledgers * 5; // ~5 seconds per ledger
    const days = seconds / (24 * 3600);
    return days.toFixed(1);
  };

  return (
    <div className="card">
      <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
        <Timer size={24} className="text-emerald-400" />
        Token Approval TTL
      </h3>

      <div className="bg-slate-800/50 p-4 rounded mb-4">
        <div className="text-sm text-slate-400 mb-1">Current Approval TTL</div>
        {ttl !== null ? (
          <div>
            <div className="text-2xl font-mono font-bold text-emerald-400">
              {ttl}
            </div>
            <div className="text-xs text-slate-500 mt-1">
              (~{formatDays(ttl)} days or ~{((ttl * 5) / 60).toFixed(0)}{" "}
              minutes)
            </div>
          </div>
        ) : (
          <div className="text-sm text-slate-500">Loading...</div>
        )}
      </div>

      <div className="bg-slate-800/50 border border-slate-700 p-4 rounded mb-4">
        <p className="text-sm text-slate-300">
          The approval TTL (Time To Live) controls how long token approvals to
          Blend and DEX pools remain valid. This determines how frequently the
          vault needs to refresh approvals.
        </p>
      </div>

      <div>
        <label className="block text-sm font-medium mb-2">
          New Approval TTL (ledgers)
        </label>
        <p className="text-xs text-slate-400 mb-2">
          Valid range: 1,000 - 500,000 ledgers
        </p>
        <div className="flex gap-2">
          <input
            type="number"
            value={newTtl}
            onChange={(e) => setNewTtl(e.target.value)}
            className="flex-1 px-3 py-2 bg-slate-800 border border-slate-700 rounded text-slate-100"
            placeholder="e.g., 52560 (~30 days)"
            min="1000"
            max="500000"
          />
          <button
            onClick={handleSetTtl}
            disabled={loading}
            className="button-primary px-6 py-2 disabled:opacity-50"
          >
            {loading ? "Updating..." : "Update"}
          </button>
        </div>

        {newTtl && (
          <div className="mt-3 p-2 bg-slate-800 rounded text-xs text-slate-400">
            <div>Approximately {formatDays(parseInt(newTtl))} days</div>
            <div>
              Or approximately {((parseInt(newTtl) * 5) / 60).toFixed(0)}{" "}
              minutes
            </div>
          </div>
        )}
      </div>

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
