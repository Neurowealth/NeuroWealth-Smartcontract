'use client';

import React from 'react';
import { DollarSign, ArrowUpRight, ArrowDownLeft, ShieldCheck } from 'lucide-react';

interface BalanceCardProps {
  balance: number;
  usdEquivalent: number;
  exchangeRate: number;
  onOpenDeposit: () => void;
  onOpenWithdraw: () => void;
  isConnected: boolean;
}

export const BalanceCard: React.FC<BalanceCardProps> = ({
  balance,
  usdEquivalent,
  exchangeRate,
  onOpenDeposit,
  onOpenWithdraw,
  isConnected
}) => {
  return (
    <div className="glass-panel-interactive rounded-2xl p-6 relative overflow-hidden">
      <div className="absolute top-0 right-0 w-48 h-48 bg-emerald-500/10 rounded-full blur-3xl pointer-events-none" />
      
      <div className="flex items-center justify-between mb-4">
        <span className="text-xs font-semibold text-slate-600 dark:text-slate-400 uppercase tracking-wider flex items-center gap-1.5">
          <DollarSign size={14} className="text-emerald-500 dark:text-emerald-400" /> Total Portfolio Value
        </span>
        <span className="text-xs font-medium text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-2.5 py-1 rounded-full border border-emerald-500/20 flex items-center gap-1">
          <ShieldCheck size={12} /> Soroban Vault Protected
        </span>
      </div>

      <div className="mb-6">
        <div className="flex items-baseline gap-2">
          <span className="text-4xl font-extrabold tracking-tight text-slate-900 dark:text-white font-mono">
            {isConnected ? balance.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 }) : '0.00'}
          </span>
          <span className="text-lg font-bold text-emerald-500 dark:text-emerald-400">USDC</span>
        </div>
        <p className="text-sm text-slate-600 dark:text-slate-400 mt-1 font-mono">
          ≈ ${isConnected ? usdEquivalent.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 }) : '0.00'} USD
          <span className="ml-3 text-xs text-slate-600 dark:text-slate-400">Rate: {exchangeRate.toFixed(4)} USDC/Share</span>
        </p>
      </div>

      <div className="grid grid-cols-2 gap-3 pt-2">
        <button
          onClick={onOpenDeposit}
          disabled={!isConnected}
          className="flex items-center justify-center gap-2 bg-emerald-500 hover:bg-emerald-400 text-slate-950 font-semibold py-3 px-4 rounded-xl transition-all shadow-glow-emerald disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <ArrowDownLeft size={18} />
          <span>Deposit</span>
        </button>

        <button
          onClick={onOpenWithdraw}
          disabled={!isConnected}
          className="flex items-center justify-center gap-2 bg-slate-100 hover:bg-slate-200 text-slate-800 border border-slate-300 dark:bg-slate-800 dark:hover:bg-slate-700 dark:text-slate-200 dark:border-slate-700 font-semibold py-3 px-4 rounded-xl transition-all disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <ArrowUpRight size={18} />
          <span>Withdraw</span>
        </button>
      </div>
    </div>
  );
};
