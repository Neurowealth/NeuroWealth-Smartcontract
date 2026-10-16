"use client";

import { useState, useEffect } from "react";
import { VaultClient } from "@neurowealth/vault-client";
import { useStore } from "@/lib/store";
import { getSorobanServer } from "@/lib/stellar";
import { Clock } from "lucide-react";
import * as StellarSdk from "@stellar/stellar-sdk";

interface RebalanceCooldownProps {
  client: VaultClient;
  signer: StellarSdk.Keypair;
}

export function RebalanceCooldown({ client, signer }: RebalanceCooldownProps) {
  const { network } = useStore();
  const [cooldown, setCooldown] = useState<number | null>(null);
  const [lastRebalance, setLastRebalance] = useState<number | null>(null);
  const [newCooldown, setNewCooldown] = useState("");

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [txHash, setTxHash] = useState<string | null>(null);

  useEffect(() => {
    fetchCooldownInfo();
  }, [client]);

  const fetchCooldownInfo = async () => {
    try {
      const cd = await client.get_rebalance_cooldown(signer.publicKey());
      const lr = await client.get_last_rebalance_ledger(signer.publicKey());

      setCooldown(cd);
      setLastRebalance(lr);
      setNewCooldown(cd.toString());
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to fetch cooldown info",
      );
    }
  };

  const handleSetCooldown = async () => {
    if (!newCooldown || parseInt(newCooldown) < 0) {
      setError("Invalid cooldown value");
      return;
    }

    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const cooldownValue = parseInt(newCooldown);
      const result = await client.set_rebalance_cooldown(signer, cooldownValue);
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);
      setCooldown(cooldownValue);

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
      setError(err instanceof Error ? err.message : "Failed to set cooldown");
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="card">
      <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
        <Clock size={24} className="text-emerald-400" />
        Rebalance Cooldown
      </h3>

      <div className="grid grid-cols-2 gap-4 mb-4">
        <div className="bg-slate-800/50 p-4 rounded">
          <div className="text-sm text-slate-400 mb-1">Current Cooldown</div>
          {cooldown !== null ? (
            <div>
              <div className="text-2xl font-mono font-bold text-emerald-400">
                {cooldown}
              </div>
              <div className="text-xs text-slate-500 mt-1">
                ledgers (~{(cooldown * 5).toFixed(0)}s)
              </div>
            </div>
          ) : (
            <div className="text-sm text-slate-500">Loading...</div>
          )}
        </div>

        <div className="bg-slate-800/50 p-4 rounded">
          <div className="text-sm text-slate-400 mb-1">Last Rebalance</div>
          {lastRebalance !== null ? (
            <div>
              <div className="text-2xl font-mono font-bold text-emerald-400">
                {lastRebalance}
              </div>
              <div className="text-xs text-slate-500 mt-1">ledger height</div>
            </div>
          ) : (
            <div className="text-sm text-slate-500">N/A</div>
          )}
        </div>
      </div>

      <div className="bg-slate-800/50 border border-slate-700 p-4 rounded mb-4">
        <p className="text-sm text-slate-300">
          The cooldown period (in ledgers) prevents the AI agent from
          rebalancing too frequently. Each Stellar ledger closes approximately
          every 5 seconds.
        </p>
      </div>

      <div>
        <label className="block text-sm font-medium mb-2">
          New Cooldown (ledgers)
        </label>
        <div className="flex gap-2">
          <input
            type="number"
            value={newCooldown}
            onChange={(e) => setNewCooldown(e.target.value)}
            className="flex-1 px-3 py-2 bg-slate-800 border border-slate-700 rounded text-slate-100"
            placeholder="e.g., 200 (approximately 16 minutes)"
            min="0"
          />
          <button
            onClick={handleSetCooldown}
            disabled={loading}
            className="button-primary px-6 py-2 disabled:opacity-50"
          >
            {loading ? "Updating..." : "Update"}
          </button>
        </div>
        <p className="text-xs text-slate-500 mt-2">
          0 = no cooldown. Set value disables future rebalances until agent is
          manually reinitiated.
        </p>
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
