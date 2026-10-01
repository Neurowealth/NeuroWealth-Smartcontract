'use client';

import React, { useState, useEffect, useCallback } from 'react';
import { Header } from '@/components/Header';
import { BalanceCard } from '@/components/BalanceCard';
import { EarningsCard } from '@/components/EarningsCard';
import { StrategyBadge } from '@/components/StrategyBadge';
import { PortfolioChart } from '@/components/PortfolioChart';
import { TransactionHistory } from '@/components/TransactionHistory';
import { ActionModal } from '@/components/ActionModal';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { MessageSquare, Bot, ArrowRight, ShieldCheck, Zap, Layers, Loader2, AlertTriangle, RefreshCw } from 'lucide-react';
import { useTranslations } from 'next-intl';
import { connectFreighterWallet } from '@/lib/freighter';
import { fetchVaultState, VaultState } from '@/lib/stellar';
import {
  getEarningsSummary,
  getPortfolioValueHistory,
  getRecentTransactions,
  EarningsSummary,
  ChartDataPoint,
  TransactionRecord
} from '@/lib/database';

export default function DashboardPage() {
  const t = useTranslations('Index');
  const [publicKey, setPublicKey] = useState<string | null>(null);
  const [vaultState, setVaultState] = useState<VaultState>({
    balance: 0,
    strategy: 'Balanced',
    exchangeRate: 1.042,
    apy: 8.4
  });
  const [earnings, setEarnings] = useState<EarningsSummary>({ today: 0, week: 0, month: 0 });
  const [chartData, setChartData] = useState<ChartDataPoint[]>([]);
  const [transactions, setTransactions] = useState<TransactionRecord[]>([]);
  const [loading, setLoading] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  const [isModalOpen, setIsModalOpen] = useState<boolean>(false);
  const [modalType, setModalType] = useState<'deposit' | 'withdraw'>('deposit');

  const handleConnect = async () => {
    const key = await connectFreighterWallet();
    if (key) {
      setPublicKey(key);
    }
  };

  const handleDisconnect = () => {
    setPublicKey(null);
    setError(null);
  };

  const loadData = useCallback(async () => {
    if (!publicKey) return;

    setLoading(true);
    setError(null);

    try {
      const [state, earnData, chart, txs] = await Promise.all([
        fetchVaultState(publicKey),
        getEarningsSummary(publicKey),
        getPortfolioValueHistory(publicKey),
        getRecentTransactions(publicKey),
      ]);

      setVaultState(state);
      setEarnings(earnData);
      setChartData(chart);
      setTransactions(txs);
    } catch (err) {
      console.error('Failed to load dashboard data:', err);
      setError('Failed to load dashboard data. Please try again.');
    } finally {
      setLoading(false);
    }
  }, [publicKey]);

  useEffect(() => {
    if (publicKey) {
      loadData();
    } else {
      setVaultState({ balance: 0, strategy: 'Balanced', exchangeRate: 1.042, apy: 8.4 });
      setEarnings({ today: 0, week: 0, month: 0 });
      setChartData([]);
      setTransactions([]);
      setError(null);
    }
  }, [publicKey, loadData]);

  const openModal = (type: 'deposit' | 'withdraw') => {
    setModalType(type);
    setIsModalOpen(true);
  };

  return (
    <div className="min-h-screen bg-[#080b11] text-slate-100 flex flex-col justify-between">
      <div>
        <Header publicKey={publicKey} onConnect={handleConnect} onDisconnect={handleDisconnect} />

        <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-8">
          {!publicKey && (
            <div className="glass-panel rounded-3xl p-8 relative overflow-hidden border border-emerald-500/20 bg-gradient-to-r from-emerald-950/30 via-slate-900/60 to-indigo-950/30 shadow-glow-emerald">
              <div className="max-w-2xl relative z-10">
                <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-emerald-500/10 text-emerald-400 text-xs font-semibold border border-emerald-500/20 mb-4">
                  <Zap size={14} /> AI-Powered Autonomous Yield Engine
                </div>
                <h1 className="text-3xl sm:text-4xl font-extrabold tracking-tight text-white mb-3">
                  {t('title')}
                </h1>
                <p className="text-slate-300 text-sm sm:text-base leading-relaxed mb-6">
                  {t('description')}
                </p>
                <button
                  onClick={handleConnect}
                  className="flex items-center gap-2 bg-gradient-to-r from-emerald-500 to-teal-600 hover:from-emerald-400 hover:to-teal-500 text-slate-950 font-bold px-6 py-3 rounded-full transition-all shadow-glow-emerald"
                >
                  <span>{t('getStarted')}</span>
                  <ArrowRight size={18} />
                </button>
              </div>
            </div>
          )}

          {error && (
            <div className="flex items-center gap-3 p-4 rounded-xl bg-red-500/10 border border-red-500/20 text-red-400">
              <AlertTriangle size={18} />
              <span className="text-sm flex-1">{error}</span>
              <button
                onClick={loadData}
                className="flex items-center gap-1.5 text-xs font-semibold px-3 py-1.5 rounded-lg bg-red-500/20 hover:bg-red-500/30 transition-colors"
              >
                <RefreshCw size={12} />
                Retry
              </button>
            </div>
          )}

          <ErrorBoundary surfaceName="portfolio" onReset={loadData}>
            <section id="dashboard" className="grid grid-cols-1 md:grid-cols-3 gap-6">
              {loading ? (
                <>
                  <SkeletonCard />
                  <SkeletonCard />
                  <SkeletonCard />
                </>
              ) : (
                <>
                  <BalanceCard
                    balance={vaultState.balance}
                    usdEquivalent={vaultState.balance * 1.0}
                    exchangeRate={vaultState.exchangeRate}
                    onOpenDeposit={() => openModal('deposit')}
                    onOpenWithdraw={() => openModal('withdraw')}
                    isConnected={!!publicKey}
                  />
                  <EarningsCard earnings={earnings} isConnected={!!publicKey} />
                  <StrategyBadge
                    strategy={vaultState.strategy}
                    apy={vaultState.apy}
                    onSelectStrategy={(newSt) => setVaultState((prev) => ({ ...prev, strategy: newSt }))}
                  />
                </>
              )}
            </section>
          </ErrorBoundary>

          <ErrorBoundary surfaceName="strategies" onReset={loadData}>
            <section id="strategies">
              {loading ? (
                <div className="glass-panel rounded-2xl p-6 h-64 flex items-center justify-center">
                  <Loader2 className="animate-spin text-emerald-400" size={24} />
                </div>
              ) : (
                <PortfolioChart data={chartData.length > 0 ? chartData : [
                  { date: 'Jul 21', value: 1000, yield: 0 },
                  { date: 'Jul 24', value: 1200, yield: 15 },
                  { date: 'Jul 28', value: 1450, yield: 45 }
                ]} />
              )}
            </section>
          </ErrorBoundary>

          <section id="history" className="grid grid-cols-1 lg:grid-cols-3 gap-6">
            <div className="lg:col-span-2">
              <ErrorBoundary surfaceName="transactions" onReset={loadData}>
                {loading ? (
                  <div className="glass-panel rounded-2xl p-6 h-48 flex items-center justify-center">
                    <Loader2 className="animate-spin text-emerald-400" size={24} />
                  </div>
                ) : (
                  <TransactionHistory transactions={transactions} />
                )}
              </ErrorBoundary>
            </div>

            <div id="whatsapp" className="glass-panel-interactive rounded-2xl p-6 relative overflow-hidden flex flex-col justify-between border border-emerald-500/20">
              <div>
                <div className="flex items-center justify-between mb-4">
                  <span className="text-xs font-semibold text-slate-400 uppercase tracking-wider flex items-center gap-1.5">
                    <MessageSquare size={14} className="text-emerald-400" /> WhatsApp Integration
                  </span>
                  <span className="text-[10px] font-bold uppercase px-2 py-0.5 rounded bg-emerald-500/20 text-emerald-400 border border-emerald-500/30">
                    Live Bot
                  </span>
                </div>
                <h3 className="text-lg font-bold text-white mb-2">
                  Interact via WhatsApp Chat
                </h3>
                <p className="text-xs text-slate-300 leading-relaxed mb-4">
                  No browser or wallet needed! Simply text our Twilio bot to verify with OTP, check balance, deposit, or withdraw on the go.
                </p>
                <div className="bg-slate-950 p-3 rounded-xl border border-slate-800 font-mono text-xs text-emerald-400 space-y-1 mb-4">
                  <div>User: deposit 100 USDC</div>
                  <div className="text-slate-300">Agent: Got it! Deposited 100 USDC into Balanced strategy. ✅</div>
                </div>
              </div>
              <div className="pt-4 border-t border-slate-800">
                <span className="text-xs text-slate-400 block mb-2">Webhook URL:</span>
                <code className="text-[11px] bg-slate-900 px-2 py-1 rounded border border-slate-800 text-slate-300 block truncate">
                  /api/whatsapp/webhook
                </code>
              </div>
            </div>
          </section>
        </main>
      </div>

      <ActionModal
        isOpen={isModalOpen}
        onClose={() => setIsModalOpen(false)}
        type={modalType}
        userPublicKey={publicKey}
        balance={vaultState.balance}
        exchangeRate={vaultState.exchangeRate}
      />

      <footer className="border-t border-slate-800/80 bg-[#06080e] py-6 mt-12 text-center text-xs text-slate-400">
        <div className="max-w-7xl mx-auto px-4 flex flex-col sm:flex-row items-center justify-between gap-4">
          <div className="flex items-center gap-2">
            <Bot size={16} className="text-emerald-400" />
            <span className="font-semibold text-slate-300">NeuroWealth AI Vault</span>
            <span>— Soroban Smart Contract Architecture</span>
          </div>
          <div className="flex items-center gap-6">
            <a href="https://stellar.org" target="_blank" rel="noreferrer" className="hover:text-emerald-400 transition-colors">Stellar Network</a>
            <a href="https://soroban.stellar.org" target="_blank" rel="noreferrer" className="hover:text-emerald-400 transition-colors">Soroban SDK</a>
            <a href="https://freighter.app" target="_blank" rel="noreferrer" className="hover:text-emerald-400 transition-colors">Freighter Wallet</a>
          </div>
        </div>
      </footer>
    </div>
  );
}

function SkeletonCard() {
  return (
    <div className="glass-panel rounded-2xl p-6 animate-pulse space-y-4">
      <div className="h-4 bg-slate-700/50 rounded w-1/3" />
      <div className="h-8 bg-slate-700/50 rounded w-1/2" />
      <div className="h-3 bg-slate-700/50 rounded w-2/3" />
    </div>
  );
}
