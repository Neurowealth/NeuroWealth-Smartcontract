'use client';

import React, { useState } from 'react';
import { X, ArrowDownLeft, ArrowUpRight, Loader2, CheckCircle2, ShieldAlert } from 'lucide-react';
import { signWithFreighter, WalletSigningError, type WalletErrorKind } from '@/lib/freighter';
import {
  rpc,
  Contract,
  Address,
  nativeToScVal,
  TransactionBuilder,
  BASE_FEE,
} from '@stellar/stellar-sdk';

const RPC_URL = process.env.NEXT_PUBLIC_SOROBAN_RPC_URL || 'https://soroban-testnet.stellar.org';
const NETWORK_PASSPHRASE = process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE || 'Test SDF Network ; September 2015';
const VAULT_CONTRACT_ID = process.env.NEXT_PUBLIC_VAULT_CONTRACT_ID || 'CDLZFC3SYJYD7M6LJEFAPCHRLHAFKP6WYTHRF3EGO5CYD3EP4GZGM37T';

type TxErrorKind = WalletErrorKind | 'submission_failed';

interface TxError {
  kind: TxErrorKind;
  message: string;
}

const TX_ERROR_COPY: Record<TxErrorKind, string> = {
  wrong_network: 'Wrong network. Switch Freighter to the required Stellar network, then try again.',
  wallet_disconnected: 'Wallet not connected. Reconnect Freighter and grant this app access, then try again.',
  submission_failed: 'Transaction failed. Please try again.',
  unknown: 'Transaction failed. Please try again.',
};

interface ActionModalProps {
  isOpen: boolean;
  onClose: () => void;
  type: 'deposit' | 'withdraw';
  userPublicKey: string | null;
  balance: number;
  exchangeRate: number;
}

export const ActionModal: React.FC<ActionModalProps> = ({
  isOpen,
  onClose,
  type,
  userPublicKey,
  balance,
  exchangeRate
}) => {
  const [amount, setAmount] = useState<string>('');
  const [loading, setLoading] = useState<boolean>(false);
  const [txSuccess, setTxSuccess] = useState<boolean>(false);
  const [txHash, setTxHash] = useState<string>('');
  const [txError, setTxError] = useState<TxError | null>(null);

  if (!isOpen) return null;

  const numAmount = parseFloat(amount) || 0;
  const estimatedShares = (numAmount / exchangeRate).toFixed(4);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!userPublicKey || numAmount <= 0) return;

    setLoading(true);
    setTxSuccess(false);
    setTxError(null);

    try {
      const server = new rpc.Server(RPC_URL);
      const account = await server.getAccount(userPublicKey);
      const contract = new Contract(VAULT_CONTRACT_ID);
      const amountInBaseUnits = BigInt(Math.round(numAmount * 1e7));

      const txBuilder = new TransactionBuilder(account, {
        fee: BASE_FEE,
        networkPassphrase: NETWORK_PASSPHRASE,
      });

      const operation = contract.call(
        type === 'deposit' ? 'deposit' : 'withdraw',
        new Address(userPublicKey).toScVal(),
        nativeToScVal(amountInBaseUnits, { type: 'i128' }),
      );

      const transaction = txBuilder
        .addOperation(operation)
        .setTimeout(300)
        .build();

      const preparedTx = await server.prepareTransaction(transaction);
      const xdr = preparedTx.toXDR();

      let signedXdr: string;
      try {
        signedXdr = await signWithFreighter(xdr, NETWORK_PASSPHRASE);
      } catch (err) {
        if (err instanceof WalletSigningError && err.kind === 'user_rejected') {
          // A cancelled signature is a normal, recoverable state: reset quietly
          // and never fabricate a failure transaction hash for it.
          return;
        }
        const kind: WalletErrorKind = err instanceof WalletSigningError ? err.kind : 'unknown';
        setTxError({ kind, message: TX_ERROR_COPY[kind] });
        return;
      }

      const sendResult = await server.sendTransaction(TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE));

      if (sendResult.status === 'ERROR' || sendResult.status === 'TRY_AGAIN_LATER') {
        setTxError({ kind: 'submission_failed', message: TX_ERROR_COPY.submission_failed });
        return;
      }

      // The network has now observed this transaction - the hash is real and must
      // be preserved even while confirmation is still pending, so a later polling
      // step can't relabel a genuine submission as a failure.
      setTxHash(sendResult.hash);

      const confirmation = await server.pollTransaction(sendResult.hash, { attempts: 10 });

      if (confirmation.status === 'SUCCESS') {
        setTxSuccess(true);
      } else {
        setTxError({ kind: 'submission_failed', message: TX_ERROR_COPY.submission_failed });
      }
    } catch (err) {
      console.error('Transaction execution failed:', err);
      setTxError({ kind: 'unknown', message: TX_ERROR_COPY.unknown });
    } finally {
      setLoading(false);
    }
  };

  const handleResetAndClose = () => {
    setAmount('');
    setTxSuccess(false);
    setTxHash('');
    setTxError(null);

    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-slate-950/80 backdrop-blur-md">
      <div className="glass-panel w-full max-w-md rounded-2xl p-6 relative border border-slate-700 shadow-2xl animate-in fade-in zoom-in duration-200">
        <button
          onClick={handleResetAndClose}
          className="absolute top-4 right-4 text-slate-400 hover:text-white p-1 rounded-lg hover:bg-slate-800"
        >
          <X size={20} />
        </button>

        {txSuccess ? (
          <div className="text-center py-6">
            <div className="h-16 w-16 bg-emerald-500/20 text-emerald-400 rounded-full flex items-center justify-center mx-auto mb-4 border border-emerald-500/30">
              <CheckCircle2 size={36} />
            </div>
            <h3 className="text-xl font-bold text-white mb-2">
              {type === 'deposit' ? 'Deposit Successful!' : 'Withdrawal Successful!'}
            </h3>
            <p className="text-sm text-slate-400 mb-4 font-mono">
              Transaction Hash: {txHash.substring(0, 12)}...{txHash.substring(txHash.length - 8)}
            </p>
            <p className="text-xs text-emerald-400 bg-emerald-500/10 py-2 px-3 rounded-lg border border-emerald-500/20 mb-6">
              Confirmed on Stellar network
            </p>
            <button
              onClick={handleResetAndClose}
              className="w-full bg-emerald-500 hover:bg-emerald-400 text-slate-950 font-bold py-3 rounded-xl transition-all"
            >
              Done
            </button>
          </div>
        ) : (
          <form onSubmit={handleSubmit}>
            <div className="flex items-center gap-2 mb-6">
              {type === 'deposit' ? (
                <div className="h-10 w-10 bg-emerald-500/20 text-emerald-400 rounded-xl flex items-center justify-center">
                  <ArrowDownLeft size={22} />
                </div>
              ) : (
                <div className="h-10 w-10 bg-indigo-500/20 text-indigo-400 rounded-xl flex items-center justify-center">
                  <ArrowUpRight size={22} />
                </div>
              )}
              <div>
                <h3 className="text-lg font-bold text-white capitalize">
                  {type} USDC
                </h3>
                <p className="text-xs text-slate-400">
                  {type === 'deposit' ? 'Mint shares in Soroban vault' : 'Burn shares & withdraw USDC'}
                </p>
              </div>
            </div>

            <div className="mb-4">
              <div className="flex justify-between text-xs text-slate-400 mb-1.5 font-medium">
                <span>Amount (USDC)</span>
                <span>Available: {balance.toFixed(2)} USDC</span>
              </div>
              <div className="relative">
                <input
                  type="number"
                  step="0.01"
                  min="1"
                  max={type === 'withdraw' ? balance : 10000}
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  placeholder="0.00"
                  className="w-full bg-slate-900 border border-slate-700 rounded-xl py-3 px-4 text-white font-mono text-lg focus:outline-none focus:border-emerald-500 transition-colors"
                  required
                />
                <button
                  type="button"
                  onClick={() => setAmount(type === 'withdraw' ? balance.toString() : '100')}
                  className="absolute right-3 top-3 text-xs font-semibold text-emerald-400 hover:text-emerald-300 bg-emerald-500/10 px-2 py-1 rounded"
                >
                  MAX
                </button>
              </div>
            </div>

            <div className="bg-slate-900/80 p-3 rounded-xl border border-slate-800 text-xs space-y-2 mb-6 font-mono text-slate-300">
              <div className="flex justify-between">
                <span className="text-slate-400">Exchange Rate:</span>
                <span>{exchangeRate.toFixed(4)} USDC / Share</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Estimated Shares:</span>
                <span className="text-emerald-400">{estimatedShares} NV-SHARES</span>
              </div>
              <div className="flex justify-between">
                <span className="text-slate-400">Network Fee:</span>
                <span className="text-slate-400">&lt; 0.00001 XLM</span>
              </div>
            </div>

            {txError && (
              <div
                role="alert"
                className="flex items-start gap-2 bg-rose-500/10 border border-rose-500/20 text-rose-300 text-xs rounded-xl p-3 mb-4"
              >
                <ShieldAlert size={16} className="shrink-0 mt-0.5" />
                <div>
                  <p>{txError.message}</p>
                  {txHash && (
                    <p className="mt-1 font-mono text-rose-400/80">
                      Transaction Hash: {txHash.substring(0, 12)}...{txHash.substring(txHash.length - 8)}
                    </p>
                  )}
                </div>
              </div>
            )}

            <button
              type="submit"
              disabled={loading || numAmount <= 0}
              className="w-full flex items-center justify-center gap-2 bg-gradient-to-r from-emerald-500 to-teal-600 hover:from-emerald-400 hover:to-teal-500 text-slate-950 font-bold py-3.5 rounded-xl transition-all shadow-glow-emerald disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {loading ? (
                <>
                  <Loader2 className="animate-spin" size={18} />
                  <span>Signing with Freighter...</span>
                </>
              ) : (
                <span>Confirm {type === 'deposit' ? 'Deposit' : 'Withdrawal'}</span>
              )}
            </button>
          </form>
        )}
      </div>
    </div>
  );
};
