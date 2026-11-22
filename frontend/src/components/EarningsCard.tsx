'use client';

import React, { useState } from 'react';
import { TrendingUp, Calendar } from 'lucide-react';
import { EarningsSummary } from '@/lib/database';

interface EarningsCardProps {
  earnings: EarningsSummary;
  isConnected: boolean;
}

export const EarningsCard: React.FC<EarningsCardProps> = ({ earnings, isConnected }) => {
  const [period, setPeriod] = useState<'today' | 'week' | 'month'>('today');

  const currentValue = isConnected ? earnings[period] : 0;

  return (
    <div className="glass-panel-interactive rounded-2xl p-6 relative overflow-hidden flex flex-col justify-between">
      <div>
        <div className="flex items-center justify-between mb-4">
          <span className="text-xs font-semibold text-slate-600 dark:text-slate-400 uppercase tracking-wider flex items-center gap-1.5">
            <TrendingUp size={14} className="text-indigo-500 dark:text-indigo-400" /> Yield Earnings
          </span>
          <div className="flex bg-slate-100 dark:bg-slate-900 p-1 rounded-lg border border-slate-200 dark:border-slate-800 text-xs">
            <button
              onClick={() => setPeriod('today')}
              className={`px-2.5 py-1 rounded-md transition-colors ${
                period === 'today' ? 'bg-indigo-600 text-white font-medium' : 'text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white'
              }`}
            >
              Today
            </button>
            <button
              onClick={() => setPeriod('week')}
              className={`px-2.5 py-1 rounded-md transition-colors ${
                period === 'week' ? 'bg-indigo-600 text-white font-medium' : 'text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white'
              }`}
            >
              Week
            </button>
            <button
              onClick={() => setPeriod('month')}
              className={`px-2.5 py-1 rounded-md transition-colors ${
                period === 'month' ? 'bg-indigo-600 text-white font-medium' : 'text-slate-600 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white'
              }`}
            >
              Month
            </button>
          </div>
        </div>

        <div className="mb-2">
          <div className="flex items-baseline gap-2">
            <span className="text-4xl font-extrabold tracking-tight text-slate-900 dark:text-white font-mono">
              +${currentValue.toFixed(2)}
            </span>
            <span className="text-xs font-semibold text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20">
              Auto-Compounded
            </span>
          </div>
          <p className="text-xs text-slate-600 dark:text-slate-400 mt-2 flex items-center gap-1">
            <Calendar size={12} /> Yield generated autonomously by AI agent strategy
          </p>
        </div>
      </div>

      <div className="pt-4 border-t border-slate-200 dark:border-slate-800/80 grid grid-cols-3 gap-2 text-center text-xs">
        <div className="bg-slate-100 dark:bg-slate-900/60 p-2 rounded-lg border border-slate-200 dark:border-slate-800">
          <div className="text-slate-600 dark:text-slate-400">Today</div>
          <div className="font-semibold text-emerald-600 dark:text-emerald-400 font-mono">+${earnings.today.toFixed(2)}</div>
        </div>
        <div className="bg-slate-100 dark:bg-slate-900/60 p-2 rounded-lg border border-slate-200 dark:border-slate-800">
          <div className="text-slate-600 dark:text-slate-400">7 Days</div>
          <div className="font-semibold text-emerald-600 dark:text-emerald-400 font-mono">+${earnings.week.toFixed(2)}</div>
        </div>
        <div className="bg-slate-100 dark:bg-slate-900/60 p-2 rounded-lg border border-slate-200 dark:border-slate-800">
          <div className="text-slate-600 dark:text-slate-400">30 Days</div>
          <div className="font-semibold text-emerald-600 dark:text-emerald-400 font-mono">+${earnings.month.toFixed(2)}</div>
        </div>
      </div>
    </div>
  );
};
