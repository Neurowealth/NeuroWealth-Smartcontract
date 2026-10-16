"use client";

import { useState, useEffect } from "react";
import { VaultClient } from "@neurowealth/vault-client";
import { useStore } from "@/lib/store";
import { getSorobanServer } from "@/lib/stellar";
import { Bot, AlertTriangle, CheckCircle } from "lucide-react";
import * as StellarSdk from "@stellar/stellar-sdk";
import { formatDistance } from "date-fns";

interface AgentManagementProps {
  client: VaultClient;
  signer: StellarSdk.Keypair;
  currentLedger: number;
}

export function AgentManagement({
  client,
  signer,
  currentLedger,
}: AgentManagementProps) {
  const { network } = useStore();
  const [agent, setAgent] = useState<string | null>(null);
  const [pendingAgent, setPendingAgent] = useState<string | null>(null);
  const [pendingExpiry, setPendingExpiry] = useState<number | null>(null);
  const [newAgent, setNewAgent] = useState("");

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [txHash, setTxHash] = useState<string | null>(null);
  const [showUpdateConfirm, setShowUpdateConfirm] = useState(false);
  const [activeTab, setActiveTab] = useState<"update" | "confirm" | "cancel">(
    "update",
  );

  useEffect(() => {
    fetchAgentInfo();
  }, [client]);

  const fetchAgentInfo = async () => {
    try {
      const a = await client.get_agent(signer.publicKey());
      const pending = await client.get_pending_agent_update(signer.publicKey());

      setAgent(a);
      if (pending) {
        // pending is likely { agent: string, expiry: number }
        if (typeof pending === "object" && "agent" in pending) {
          setPendingAgent((pending as any).agent);
          setPendingExpiry((pending as any).expiry);
        }
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to fetch agent info",
      );
    }
  };

  const handleUpdateAgent = async () => {
    if (!newAgent.startsWith("G")) {
      setError("Invalid Stellar address (must start with G)");
      return;
    }

    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.update_agent(signer, newAgent);
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);
      setShowUpdateConfirm(false);

      // Fetch updated info
      await new Promise((r) => setTimeout(r, 2000));
      await fetchAgentInfo();

      setNewAgent("");
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update agent");
      setShowUpdateConfirm(false);
    } finally {
      setLoading(false);
    }
  };

  const handleConfirmAgent = async () => {
    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.confirm_agent_update(signer);
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);

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

      // Fetch updated info
      await fetchAgentInfo();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to confirm agent update",
      );
    } finally {
      setLoading(false);
    }
  };

  const handleCancelAgent = async () => {
    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.cancel_agent_update(signer);
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);

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

      // Fetch updated info
      await fetchAgentInfo();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to cancel agent update",
      );
    } finally {
      setLoading(false);
    }
  };

  const canConfirmAgent =
    pendingExpiry !== null && currentLedger >= pendingExpiry;
  const ledgersUntilConfirm =
    pendingExpiry !== null ? Math.max(0, pendingExpiry - currentLedger) : 0;
  const estimatedWaitMinutes = (ledgersUntilConfirm * 5) / 60;

  return (
    <div className="card">
      <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
        <Bot size={24} className="text-emerald-400" />
        AI Agent Management (24-Hour Timelock)
      </h3>

      <div className="grid gap-4 mb-4">
        <div className="bg-slate-800/50 p-4 rounded">
          <div className="text-sm text-slate-400 mb-1">Current Agent</div>
          {agent ? (
            <div className="text-sm font-mono text-emerald-400 break-all">
              {agent}
            </div>
          ) : (
            <div className="text-sm text-slate-500">Loading...</div>
          )}
        </div>

        {pendingAgent && (
          <div className="bg-yellow-900/20 border border-yellow-700 p-4 rounded">
            <div className="flex items-start gap-3">
              <AlertTriangle
                className="text-yellow-400 flex-shrink-0 mt-1"
                size={20}
              />
              <div className="flex-1">
                <div className="text-sm text-yellow-100 font-semibold">
                  Pending Update (Timelock Active)
                </div>
                <div className="text-sm font-mono text-yellow-300 mt-1 break-all">
                  {pendingAgent}
                </div>
                <div className="text-xs text-yellow-200 mt-2">
                  {canConfirmAgent ? (
                    <span className="flex items-center gap-1">
                      <CheckCircle size={14} />
                      Ready to confirm
                    </span>
                  ) : (
                    <span>
                      Confirm available in ~{estimatedWaitMinutes.toFixed(0)}{" "}
                      minutes
                    </span>
                  )}
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      <div className="flex gap-2 mb-4 border-b border-slate-700">
        {(["update", "confirm", "cancel"] as const).map((tab) => (
          <button
            key={tab}
            onClick={() => setActiveTab(tab)}
            disabled={
              (tab === "confirm" && !canConfirmAgent && !pendingAgent) ||
              (tab === "cancel" && !pendingAgent)
            }
            className={`px-4 py-2 border-b-2 transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
              activeTab === tab
                ? "border-emerald-500 text-emerald-400"
                : "border-transparent text-slate-400 hover:text-slate-300"
            }`}
          >
            {tab === "update" && "Propose New Agent"}
            {tab === "confirm" && "Confirm Update"}
            {tab === "cancel" && "Cancel Pending"}
          </button>
        ))}
      </div>

      {activeTab === "update" && (
        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-2">
              New Agent Address
            </label>
            <textarea
              value={newAgent}
              onChange={(e) => setNewAgent(e.target.value)}
              className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded text-slate-100 font-mono text-sm"
              rows={2}
              placeholder="Enter Stellar account address (starts with G...)"
            />
            <p className="text-xs text-slate-500 mt-1">
              Agent update will be locked for 24 hours before confirmation
            </p>
          </div>

          {showUpdateConfirm ? (
            <div className="bg-red-900/20 border border-red-700 rounded p-4">
              <div className="flex items-start gap-3 mb-4">
                <AlertTriangle
                  className="text-red-400 flex-shrink-0 mt-1"
                  size={20}
                />
                <div>
                  <p className="font-semibold text-red-300">
                    Confirm Agent Update
                  </p>
                  <p className="text-sm text-red-200 mt-1">
                    New agent: <span className="font-mono">{newAgent}</span>
                    <br />
                    <br />
                    The update will be locked for 24 hours. After that, you can
                    confirm or cancel the change.
                  </p>
                </div>
              </div>
              <div className="flex gap-2">
                <button
                  onClick={handleUpdateAgent}
                  disabled={loading}
                  className="button-danger px-4 py-2 disabled:opacity-50"
                >
                  {loading ? "Processing..." : "Initiate Update"}
                </button>
                <button
                  onClick={() => setShowUpdateConfirm(false)}
                  disabled={loading}
                  className="button-secondary px-4 py-2"
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <button
              onClick={() => setShowUpdateConfirm(true)}
              disabled={!newAgent || loading}
              className="button-primary w-full py-3 disabled:opacity-50"
            >
              {loading ? "Processing..." : "Propose New Agent"}
            </button>
          )}
        </div>
      )}

      {activeTab === "confirm" && pendingAgent && (
        <div className="space-y-4">
          <div className="bg-emerald-900/20 border border-emerald-700 p-4 rounded">
            <p className="text-sm text-emerald-200 mb-4">
              Confirm the pending agent update to:
              <br />
              <span className="font-mono text-emerald-100">{pendingAgent}</span>
            </p>
            <button
              onClick={handleConfirmAgent}
              disabled={loading || !canConfirmAgent}
              className="button-primary w-full py-3 disabled:opacity-50"
            >
              {canConfirmAgent
                ? loading
                  ? "Processing..."
                  : "Confirm Agent Update"
                : `Available in ~${estimatedWaitMinutes.toFixed(0)} minutes`}
            </button>
          </div>
        </div>
      )}

      {activeTab === "cancel" && pendingAgent && (
        <div className="space-y-4">
          <div className="bg-slate-800/50 p-4 rounded">
            <p className="text-sm text-slate-300 mb-4">
              Cancel the pending agent update to:
              <br />
              <span className="font-mono text-slate-100">{pendingAgent}</span>
            </p>
            <button
              onClick={handleCancelAgent}
              disabled={loading}
              className="button-danger w-full py-3 disabled:opacity-50"
            >
              {loading ? "Processing..." : "Cancel Agent Update"}
            </button>
          </div>
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
