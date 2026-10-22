"use client";

import { useState, useEffect } from "react";
import { VaultClient } from "@neurowealth/vault-client";
import { useStore } from "@/lib/store";
import { getSorobanServer } from "@/lib/stellar";
import { Shield, AlertTriangle } from "lucide-react";
import * as StellarSdk from "@stellar/stellar-sdk";

interface OwnershipTransferProps {
  client: VaultClient;
  signer: StellarSdk.Keypair;
}

export function OwnershipTransfer({ client, signer }: OwnershipTransferProps) {
  const { network } = useStore();
  const [owner, setOwner] = useState<string | null>(null);
  const [pendingOwner, setPendingOwner] = useState<string | null>(null);
  const [newOwner, setNewOwner] = useState("");

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [txHash, setTxHash] = useState<string | null>(null);
  const [showInitiateConfirm, setShowInitiateConfirm] = useState(false);
  const [activeTab, setActiveTab] = useState<"initiate" | "cancel">("initiate");

  useEffect(() => {
    fetchOwnershipInfo();
  }, [client]);

  const fetchOwnershipInfo = async () => {
    try {
      const [o, po] = await Promise.all([
        client.get_owner(signer.publicKey()),
        client.get_pending_owner(signer.publicKey()),
      ]);

      setOwner(o);
      setPendingOwner(po);
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to fetch ownership info",
      );
    }
  };

  const handleInitiateTransfer = async () => {
    if (!newOwner.startsWith("G")) {
      setError("Invalid Stellar address (must start with G)");
      return;
    }

    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.transfer_ownership(signer, newOwner);
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);
      setPendingOwner(newOwner);
      setShowInitiateConfirm(false);
      setNewOwner("");

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
      setError(
        err instanceof Error
          ? err.message
          : "Failed to initiate ownership transfer",
      );
      setShowInitiateConfirm(false);
    } finally {
      setLoading(false);
    }
  };

  const handleCancelTransfer = async () => {
    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.cancel_ownership_transfer(signer);
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);
      setPendingOwner(null);

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
      setError(
        err instanceof Error ? err.message : "Failed to cancel transfer",
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="card border-2 border-yellow-600">
      <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
        <Shield size={24} className="text-yellow-400" />
        Ownership Transfer (2-Step Process)
      </h3>

      <div className="bg-yellow-900/20 border border-yellow-700 rounded p-4 mb-4">
        <div className="flex gap-3">
          <AlertTriangle
            className="text-yellow-400 flex-shrink-0 mt-1"
            size={20}
          />
          <div className="text-sm text-yellow-100">
            <p className="font-semibold mb-1">Critical Operation</p>
            <p>
              Ownership transfer requires two steps:
              <br />
              1. Current owner initiates transfer (this step)
              <br />
              2. New owner must accept via accept_ownership() function
            </p>
          </div>
        </div>
      </div>

      <div className="grid grid-cols-2 gap-4 mb-4">
        <div className="bg-slate-800/50 p-4 rounded">
          <div className="text-sm text-slate-400 mb-1">Current Owner</div>
          {owner ? (
            <div className="text-sm font-mono text-emerald-400 break-all">
              {owner}
            </div>
          ) : (
            <div className="text-sm text-slate-500">Loading...</div>
          )}
        </div>

        <div className="bg-slate-800/50 p-4 rounded">
          <div className="text-sm text-slate-400 mb-1">Pending Owner</div>
          {pendingOwner ? (
            <div className="text-sm font-mono text-yellow-400 break-all">
              {pendingOwner}
            </div>
          ) : (
            <div className="text-sm text-slate-500">None</div>
          )}
        </div>
      </div>

      <div className="flex gap-2 mb-4 border-b border-slate-700">
        {(["initiate", "cancel"] as const).map((tab) => (
          <button
            key={tab}
            onClick={() => setActiveTab(tab)}
            disabled={tab === "cancel" && !pendingOwner}
            className={`px-4 py-2 border-b-2 transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
              activeTab === tab
                ? "border-emerald-500 text-emerald-400"
                : "border-transparent text-slate-400 hover:text-slate-300"
            }`}
          >
            {tab === "initiate" && "Initiate Transfer"}
            {tab === "cancel" && "Cancel Pending"}
          </button>
        ))}
      </div>

      {activeTab === "initiate" && (
        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-2">
              New Owner Address
            </label>
            <textarea
              value={newOwner}
              onChange={(e) => setNewOwner(e.target.value)}
              className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded text-slate-100 font-mono text-sm"
              rows={2}
              placeholder="Enter Stellar account address (starts with G...)"
            />
            <p className="text-xs text-slate-500 mt-1">
              This account will need to call accept_ownership() to complete the
              transfer
            </p>
          </div>

          {showInitiateConfirm ? (
            <div className="bg-red-900/20 border border-red-700 rounded p-4">
              <div className="flex items-start gap-3 mb-4">
                <AlertTriangle
                  className="text-red-400 flex-shrink-0 mt-1"
                  size={20}
                />
                <div>
                  <p className="font-semibold text-red-300">
                    Final Confirmation
                  </p>
                  <p className="text-sm text-red-200 mt-1">
                    You are about to initiate ownership transfer to:
                    <br />
                    <span className="font-mono">{newOwner}</span>
                    <br />
                    <br />
                    This action cannot be undone. The new owner must call
                    accept_ownership() to complete the transfer.
                  </p>
                </div>
              </div>
              <div className="flex gap-2">
                <button
                  onClick={handleInitiateTransfer}
                  disabled={loading}
                  className="button-danger px-4 py-2 disabled:opacity-50"
                >
                  {loading ? "Processing..." : "Confirm Transfer"}
                </button>
                <button
                  onClick={() => setShowInitiateConfirm(false)}
                  disabled={loading}
                  className="button-secondary px-4 py-2"
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <button
              onClick={() => setShowInitiateConfirm(true)}
              disabled={!newOwner || loading}
              className="button-primary w-full py-3 disabled:opacity-50"
            >
              {loading ? "Processing..." : "Initiate Transfer"}
            </button>
          )}
        </div>
      )}

      {activeTab === "cancel" && pendingOwner && (
        <div className="space-y-4">
          <div className="bg-slate-800/50 p-4 rounded">
            <p className="text-sm text-slate-300 mb-4">
              Cancel the pending transfer to:
              <br />
              <span className="font-mono text-slate-100">{pendingOwner}</span>
            </p>
            <button
              onClick={handleCancelTransfer}
              disabled={loading}
              className="button-danger w-full py-3 disabled:opacity-50"
            >
              {loading ? "Processing..." : "Cancel Pending Transfer"}
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
