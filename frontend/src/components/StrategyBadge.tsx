'use client';

import React from 'react';
import { Zap, TrendingUp, ShieldCheck, Flame, Loader2 } from 'lucide-react';

interface StrategyBadgeProps {
  strategy: 'Conservative' | 'Balanced' | 'Growth';
  apy: number;
  onSelectStrategy?: (strategy: 'Conservative' | 'Balanced' | 'Growth') => void;
  isUpdating?: boolean;
  error?: string | null;
}

export const StrategyBadge: React.FC<StrategyBadgeProps> = ({
  strategy,
  apy,
  onSelectStrategy,
  isUpdating = false,
  error = null,
}) => {
  const getStrategyMeta = (s: string) => {
    switch (s) {
      case 'Conservative':
        return {
          icon: ShieldCheck,
          color: 'text-blue-500 dark:text-blue-400',
          bg: 'bg-blue-500/10',
          border: 'border-blue-500/30',
          desc: 'Blend stablecoin lending (Low risk)'
        };
      case 'Growth':
        return {
          icon: Flame,
          color: 'text-amber-500 dark:text-amber-400',
          bg: 'bg-amber-500/10',
          border: 'border-amber-500/30',
          desc: 'Multi-protocol DEX LP deployment (Higher yield)'
        };
      default:
        return {
          icon: Zap,
          color: 'text-emerald-500 dark:text-emerald-400',
          bg: 'bg-emerald-500/10',
          border: 'border-emerald-500/30',
          desc: 'Optimal lending + DEX liquidity mix (Recommended)'
        };
    }
  };

  const meta = getStrategyMeta(strategy);
  const IconComponent = meta.icon;

  return (
    <div className="glass-panel-interactive rounded-2xl p-6 relative overflow-hidden flex flex-col justify-between">
      <div>
        <div className="flex items-center justify-between mb-4">
          <span className="text-xs font-semibold text-slate-600 dark:text-slate-400 uppercase tracking-wider">
            Current Strategy
          </span>
          <span className={`text-xs font-semibold px-3 py-1 rounded-full border ${meta.bg} ${meta.color} ${meta.border} flex items-center gap-1.5`}>
            <IconComponent size={14} />
            {strategy}
          </span>
        </div>

        <div className="mb-4">
          <div className="flex items-baseline gap-2">
            <span className="text-4xl font-extrabold tracking-tight text-slate-900 dark:text-white font-mono">
              {apy.toFixed(1)}%
            </span>
            <span className="text-sm font-semibold text-slate-600 dark:text-slate-400 uppercase tracking-wider">
              Net APY
            </span>
            <span className="text-xs font-medium text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20 flex items-center gap-1 ml-auto">
              <TrendingUp size={12} /> +0.4% this week
            </span>
          </div>
          <p className="text-xs text-slate-600 dark:text-slate-400 mt-2">{meta.desc}</p>
        </div>
      </div>

      <div className="pt-4 border-t border-slate-200 dark:border-slate-800/80">
        <div className="flex items-center justify-between">
          <span className="text-xs text-slate-600 dark:text-slate-400">Want to optimize risk?</span>
          <div className="flex gap-1.5">
            {(['Conservative', 'Balanced', 'Growth'] as const).map((st) => (
              <button
                key={st}
                disabled={isUpdating}
                onClick={() => onSelectStrategy && onSelectStrategy(st)}
                className={`text-xs px-2.5 py-1 rounded-md border transition-all disabled:opacity-50 disabled:cursor-not-allowed ${
                  strategy === st
                    ? 'bg-slate-900 text-white border-slate-700 dark:bg-slate-800 dark:text-white dark:border-slate-600 font-medium'
                    : 'bg-slate-100 text-slate-700 border-slate-300 hover:text-slate-900 hover:bg-slate-200 dark:bg-slate-900/50 dark:text-slate-400 dark:border-slate-800 dark:hover:text-white dark:hover:bg-slate-800'
                }`}
              >
                {st}
              </button>
            ))}
          </div>
        </div>
        {isUpdating && (
          <p className="text-xs text-indigo-500 dark:text-indigo-400 mt-2 flex items-center gap-1.5">
            <Loader2 className="animate-spin" size={12} /> Updating strategy on-chain...
          </p>
        )}
        {error && (
          <p role="alert" className="text-xs text-rose-500 dark:text-rose-400 mt-2">
            {error}
          </p>
        )}
      </div>
    </div>
  );
};
