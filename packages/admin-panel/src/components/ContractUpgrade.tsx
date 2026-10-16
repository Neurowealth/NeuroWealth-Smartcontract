"use client";

import { useState, useEffect } from "react";
import { VaultClient } from "@neurowealth/vault-client";
import { useStore } from "@/lib/store";
import { getSorobanServer } from "@/lib/stellar";
import { GitBranch, AlertTriangle, CheckCircle } from "lucide-react";
import * as StellarSdk from "@stellar/stellar-sdk";

interface ContractUpgradeProps {
  client: VaultClient;
  signer: StellarSdk.Keypair;
  currentLedger: number;
}

export function ContractUpgrade({
  client,
  signer,
  currentLedger,
}: ContractUpgradeProps) {
  const { network } = useStore();
  const [version, setVersion] = useState<number | null>(null);
  const [pendingHash, setPendingHash] = useState<string | null>(null);
  const [pendingExpiry, setPendingExpiry] = useState<number | null>(null);
  const [wasmFile, setWasmFile] = useState<File | null>(null);
  const [wasmHash, setWasmHash] = useState<string>("");

  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [txHash, setTxHash] = useState<string | null>(null);
  const [showScheduleConfirm, setShowScheduleConfirm] = useState(false);
  const [activeTab, setActiveTab] = useState<"schedule" | "execute" | "cancel">(
    "schedule",
  );

  useEffect(() => {
    fetchUpgradeInfo();
  }, [client]);

  const fetchUpgradeInfo = async () => {
    try {
      const v = await client.get_version(signer.publicKey());
      const pending = await client.get_pending_upgrade(signer.publicKey());

      setVersion(v);
      if (pending && typeof pending === "object") {
        const hash = (pending as any).wasm_hash || (pending as any).hash;
        setPendingHash(hash);
        setPendingExpiry((pending as any).expiry);
      }
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to fetch upgrade info",
      );
    }
  };

  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    if (!file.name.endsWith(".wasm")) {
      setError("Please select a .wasm file");
      return;
    }

    setWasmFile(file);

    // Calculate SHA256 hash of the file
    const buffer = await file.arrayBuffer();
    const hashBuffer = await crypto.subtle.digest("SHA-256", buffer);
    const hashArray = Array.from(new Uint8Array(hashBuffer));
    const hashHex = hashArray
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");

    // Format as Soroban BytesN<32>
    setWasmHash(hashHex);
  };

  const handleScheduleUpgrade = async () => {
    if (!wasmHash) {
      setError("Please select a WASM file");
      return;
    }

    if (wasmHash.length !== 64) {
      setError("Invalid WASM hash length");
      return;
    }

    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      // Convert hex to BytesN<32>
      const hashBytes = Buffer.from(wasmHash, "hex");
      const result = await client.schedule_upgrade(
        signer,
        signer.publicKey(),
        hashBytes as any,
      );
      if (!result.hash) {
        throw new Error('Transaction did not return a hash');
      }
      setTxHash(result.hash);
      setShowScheduleConfirm(false);

      // Fetch updated info
      await new Promise((r) => setTimeout(r, 2000));
      await fetchUpgradeInfo();

      setWasmFile(null);
      setWasmHash("");
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to schedule upgrade",
      );
      setShowScheduleConfirm(false);
    } finally {
      setLoading(false);
    }
  };

  const handleExecuteUpgrade = async () => {
    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.execute_upgrade(signer, signer.publicKey());
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
      await fetchUpgradeInfo();
    } catch (err) {
      setError(
        err instanceof Error ? err.message : "Failed to execute upgrade",
      );
    } finally {
      setLoading(false);
    }
  };

  const handleCancelUpgrade = async () => {
    setLoading(true);
    setError(null);
    setTxHash(null);

    try {
      const result = await client.cancel_upgrade(signer, signer.publicKey());
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
      await fetchUpgradeInfo();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to cancel upgrade");
    } finally {
      setLoading(false);
    }
  };

  const canExecuteUpgrade =
    pendingExpiry !== null && currentLedger >= pendingExpiry;
  const ledgersUntilExecution =
    pendingExpiry !== null ? Math.max(0, pendingExpiry - currentLedger) : 0;
  const estimatedWaitMinutes = (ledgersUntilExecution * 5) / 60;

  return (
    <div className="card border-2 border-red-600">
      <h3 className="text-xl font-bold mb-4 flex items-center gap-2">
        <GitBranch size={24} className="text-red-400" />
        Contract Upgrade (24-Hour Timelock)
      </h3>

      <div className="bg-red-900/20 border border-red-700 rounded p-4 mb-4">
        <div className="flex gap-3">
          <AlertTriangle
            className="text-red-400 flex-shrink-0 mt-1"
            size={20}
          />
          <div className="text-sm text-red-100">
            <p className="font-semibold mb-1">Dangerous Operation</p>
            <p>Contract upgrades are protected by a 24-hour timelock.</p>
          </div>
        </div>
      </div>

      <div className="grid gap-4 mb-4">
        <div className="bg-slate-800/50 p-4 rounded">
          <div className="text-sm text-slate-400 mb-1">
            Current Contract Version
          </div>
          <div className="text-2xl font-mono font-bold text-emerald-400">
            {version !== null ? version : "Loading..."}
          </div>
        </div>

        {pendingHash && (
          <div className="bg-red-900/20 border border-red-700 p-4 rounded">
            <div className="flex items-start gap-3">
              <AlertTriangle
                className="text-red-400 flex-shrink-0 mt-1"
                size={20}
              />
              <div className="flex-1">
                <div className="text-sm text-red-100 font-semibold">
                  Pending Upgrade (Timelock Active)
                </div>
                <div className="text-xs font-mono text-red-300 mt-2 break-all">
                  {pendingHash}
                </div>
                <div className="text-xs text-red-200 mt-2">
                  {canExecuteUpgrade ? (
                    <span className="flex items-center gap-1">
                      <CheckCircle size={14} />
                      Ready to execute
                    </span>
                  ) : (
                    <span>
                      Execute available in ~{estimatedWaitMinutes.toFixed(0)}{" "}
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
        {(["schedule", "execute", "cancel"] as const).map((tab) => (
          <button
            key={tab}
            onClick={() => setActiveTab(tab)}
            disabled={
              (tab === "execute" && !canExecuteUpgrade && !pendingHash) ||
              (tab === "cancel" && !pendingHash)
            }
            className={`px-4 py-2 border-b-2 transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
              activeTab === tab
                ? "border-emerald-500 text-emerald-400"
                : "border-transparent text-slate-400 hover:text-slate-300"
            }`}
          >
            {tab === "schedule" && "Schedule"}
            {tab === "execute" && "Execute"}
            {tab === "cancel" && "Cancel"}
          </button>
        ))}
      </div>

      {activeTab === "schedule" && (
        <div className="space-y-4">
          <div>
            <label className="block text-sm font-medium mb-2">
              WASM Contract File
            </label>
            <input
              type="file"
              accept=".wasm"
              onChange={handleFileSelect}
              className="w-full px-3 py-2 bg-slate-800 border border-slate-700 rounded text-slate-100 text-sm"
            />
            <p className="text-xs text-slate-500 mt-1">
              Select compiled contract WASM binary
            </p>
          </div>

          {wasmFile && (
            <div className="bg-slate-800/50 p-4 rounded">
              <div className="text-sm text-slate-400 mb-2">File Selected</div>
              <div className="text-sm text-slate-100 mb-2">{wasmFile.name}</div>
              <div className="text-xs font-mono text-slate-500 break-all">
                {wasmHash}
              </div>
            </div>
          )}

          {showScheduleConfirm ? (
            <div className="bg-red-900/20 border border-red-700 rounded p-4">
              <div className="flex items-start gap-3 mb-4">
                <AlertTriangle
                  className="text-red-400 flex-shrink-0 mt-1"
                  size={20}
                />
                <div>
                  <p className="font-semibold text-red-300">
                    Confirm Upgrade Schedule
                  </p>
                  <p className="text-sm text-red-200 mt-1">
                    This will schedule a contract upgrade with a 24-hour
                    timelock.
                    <br />
                    <br />
                    After the timelock expires, you can execute the upgrade to
                    deploy the new code.
                  </p>
                </div>
              </div>
              <div className="flex gap-2">
                <button
                  onClick={handleScheduleUpgrade}
                  disabled={loading}
                  className="button-danger px-4 py-2 disabled:opacity-50"
                >
                  {loading ? "Processing..." : "Schedule Upgrade"}
                </button>
                <button
                  onClick={() => setShowScheduleConfirm(false)}
                  disabled={loading}
                  className="button-secondary px-4 py-2"
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <button
              onClick={() => setShowScheduleConfirm(true)}
              disabled={!wasmFile || loading}
              className="button-primary w-full py-3 disabled:opacity-50"
            >
              {loading ? "Processing..." : "Schedule Upgrade"}
            </button>
          )}
        </div>
      )}

      {activeTab === "execute" && pendingHash && (
        <div className="space-y-4">
          <div className="bg-emerald-900/20 border border-emerald-700 p-4 rounded">
            <p className="text-sm text-emerald-200 mb-4">
              Execute the scheduled upgrade
            </p>
            <button
              onClick={handleExecuteUpgrade}
              disabled={loading || !canExecuteUpgrade}
              className="button-primary w-full py-3 disabled:opacity-50"
            >
              {canExecuteUpgrade
                ? loading
                  ? "Processing..."
                  : "Execute Upgrade"
                : `Available in ~${estimatedWaitMinutes.toFixed(0)} minutes`}
            </button>
          </div>
        </div>
      )}

      {activeTab === "cancel" && pendingHash && (
        <div className="space-y-4">
          <div className="bg-slate-800/50 p-4 rounded">
            <p className="text-sm text-slate-300 mb-4">
              Cancel the scheduled upgrade
            </p>
            <button
              onClick={handleCancelUpgrade}
              disabled={loading}
              className="button-danger w-full py-3 disabled:opacity-50"
            >
              {loading ? "Processing..." : "Cancel Upgrade"}
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
