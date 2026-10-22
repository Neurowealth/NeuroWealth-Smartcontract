'use client';

import React, { useState, useMemo } from 'react';
import { ArrowDownLeft, ArrowUpRight, RefreshCw, ExternalLink, Download, Filter, ChevronDown } from 'lucide-react';
import { TransactionRecord } from '@/lib/database';
import { useTranslations } from 'next-intl';

interface TransactionHistoryProps {
  transactions: TransactionRecord[];
}

type SortField = 'timestamp' | 'amount';
type SortDirection = 'asc' | 'desc';
type FilterType = 'all' | 'deposit' | 'withdrawal' | 'rebalance';
type EarningsPeriod = 'daily' | 'weekly' | 'monthly' | 'yearly' | 'all';

function filterTransactions(transactions: TransactionRecord[], filter: FilterType): TransactionRecord[] {
  if (filter === 'all') return transactions;
  return transactions.filter(tx => tx.type === filter);
}

function sortTransactions(transactions: TransactionRecord[], field: SortField, direction: SortDirection): TransactionRecord[] {
  return [...transactions].sort((a, b) => {
    let comparison = 0;
    if (field === 'timestamp') {
      comparison = new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime();
    } else {
      comparison = a.amount - b.amount;
    }
    return direction === 'asc' ? comparison : -comparison;
  });
}

function calculateEarnings(transactions: TransactionRecord[], period: EarningsPeriod): number {
  const now = new Date();
  let cutoff: Date;

  switch (period) {
    case 'daily':
      cutoff = new Date(now.getTime() - 24 * 60 * 60 * 1000);
      break;
    case 'weekly':
      cutoff = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
      break;
    case 'monthly':
      cutoff = new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000);
      break;
    case 'yearly':
      cutoff = new Date(now.getTime() - 365 * 24 * 60 * 60 * 1000);
      break;
    case 'all':
    default:
      return transactions
        .filter(tx => tx.type === 'rebalance' || tx.type === 'withdrawal')
        .reduce((sum, tx) => sum + tx.amount, 0);
  }

  return transactions
    .filter(tx => {
      const txDate = new Date(tx.timestamp);
      return txDate >= cutoff && (tx.type === 'rebalance' || tx.type === 'withdrawal');
    })
    .reduce((sum, tx) => sum + tx.amount, 0);
}

function exportToCSV(transactions: TransactionRecord[]): void {
  const headers = ['Type', 'Amount', 'Asset', 'Tx Hash', 'Timestamp', 'Status'];
  const rows = transactions.map(tx => [
    tx.type,
    tx.amount.toString(),
    tx.asset || 'USDC',
    tx.txHash,
    tx.timestamp,
    tx.status,
  ]);

  const csvContent = [headers, ...rows].map(row => row.join(',')).join('\n');
  const blob = new Blob([csvContent], { type: 'text/csv;charset=utf-8;' });
  const link = document.createElement('a');
  link.href = URL.createObjectURL(blob);
  link.download = `neurowealth-transactions-${new Date().toISOString().split('T')[0]}.csv`;
  link.click();
}

export const TransactionHistory: React.FC<TransactionHistoryProps> = ({ transactions }) => {
  const t = useTranslations('Index');
  const [typeFilter, setTypeFilter] = useState<FilterType>('all');
  const [sortField, setSortField] = useState<SortField>('timestamp');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');
  const [earningsPeriod, setEarningsPeriod] = useState<EarningsPeriod>('all');

  const filteredTransactions = useMemo(() => {
    const filtered = filterTransactions(transactions, typeFilter);
    return sortTransactions(filtered, sortField, sortDirection);
  }, [transactions, typeFilter, sortField, sortDirection]);

  const earnings = useMemo(() => {
    return calculateEarnings(transactions, earningsPeriod);
  }, [transactions, earningsPeriod]);

  if (transactions.length === 0) {
    return (
      <div className="glass-panel rounded-2xl p-6 text-center text-slate-600 dark:text-slate-400 text-sm">
        {t('noTransactionHistory')}
      </div>
    );
  }

  return (
    <div className="glass-panel rounded-2xl p-6">
      <div className="flex items-center justify-between mb-4">
        <div>
          <h3 className="text-lg font-bold text-slate-900 dark:text-white tracking-tight">{t('transactionHistory')}</h3>
          <p className="text-xs text-slate-600 dark:text-slate-400">{t('onChainEvents')}</p>
        </div>
        <button
          onClick={() => exportToCSV(transactions)}
          className="flex items-center gap-2 px-3 py-1.5 text-xs font-medium text-emerald-600 dark:text-emerald-400 hover:text-emerald-500 dark:hover:text-emerald-300 bg-emerald-500/10 rounded-lg transition-colors"
        >
          <Download size={14} />
          {t('exportCsv')}
        </button>
      </div>

      <div className="flex flex-wrap items-center gap-3 mb-4 p-3 bg-slate-100 dark:bg-slate-800/30 rounded-xl">
        <div className="flex items-center gap-2">
          <Filter size={14} className="text-slate-600 dark:text-slate-400" />
          <select
            value={typeFilter}
            onChange={(e) => setTypeFilter(e.target.value as FilterType)}
            aria-label="Filter transactions by type"
            className="bg-white dark:bg-slate-800 border border-slate-300 dark:border-slate-700 rounded-lg px-2 py-1 text-xs text-slate-800 dark:text-slate-200 focus:outline-none focus:border-emerald-500"
          >
            <option value="all">{t('allTypes')}</option>
            <option value="deposit">{t('deposits')}</option>
            <option value="withdrawal">{t('withdrawals')}</option>
            <option value="rebalance">{t('rebalances')}</option>
          </select>
        </div>

        <div className="flex items-center gap-2">
          <span className="text-xs text-slate-600 dark:text-slate-400">{t('sort')}</span>
          <select
            value={`${sortField}-${sortDirection}`}
            onChange={(e) => {
              const [field, dir] = e.target.value.split('-');
              setSortField(field as SortField);
              setSortDirection(dir as SortDirection);
            }}
            aria-label="Sort transactions"
            className="bg-white dark:bg-slate-800 border border-slate-300 dark:border-slate-700 rounded-lg px-2 py-1 text-xs text-slate-800 dark:text-slate-200 focus:outline-none focus:border-emerald-500"
          >
            <option value="timestamp-desc">{t('newestFirst')}</option>
            <option value="timestamp-asc">{t('oldestFirst')}</option>
            <option value="amount-desc">{t('highestAmount')}</option>
            <option value="amount-asc">{t('lowestAmount')}</option>
          </select>
        </div>

        <div className="ml-auto flex items-center gap-2">
          <span className="text-xs text-slate-600 dark:text-slate-400">{t('earnings')}</span>
          <select
            value={earningsPeriod}
            onChange={(e) => setEarningsPeriod(e.target.value as EarningsPeriod)}
            aria-label="Filter earnings by time period"
            className="bg-white dark:bg-slate-800 border border-slate-300 dark:border-slate-700 rounded-lg px-2 py-1 text-xs text-slate-800 dark:text-slate-200 focus:outline-none focus:border-emerald-500"
          >
            <option value="daily">24h</option>
            <option value="weekly">7d</option>
            <option value="monthly">30d</option>
            <option value="yearly">1y</option>
            <option value="all">All Time</option>
          </select>
          <span className="text-sm font-semibold text-emerald-600 dark:text-emerald-400">
            {earnings.toLocaleString()} {t('usdc')}
          </span>
        </div>
      </div>

      <div className="overflow-x-auto">
        <table className="w-full text-left border-collapse text-sm">
          <thead>
            <tr className="border-b border-slate-200 dark:border-slate-800 text-xs font-semibold text-slate-600 dark:text-slate-400 uppercase tracking-wider">
              <th scope="col" className="py-3 px-4">{t('type')}</th>
              <th scope="col" className="py-3 px-4">{t('amount')}</th>
              <th scope="col" className="py-3 px-4">{t('txHash')}</th>
              <th scope="col" className="py-3 px-4">{t('dateTime')}</th>
              <th scope="col" className="py-3 px-4 text-right">{t('status')}</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-200 dark:divide-slate-800/60 font-mono text-xs">
            {filteredTransactions.map((tx) => (
              <tr key={tx.id} className="hover:bg-slate-50 dark:hover:bg-slate-900/40 transition-colors">
                <td className="py-3 px-4">
                  <div className="flex items-center gap-2">
                    {tx.type === 'deposit' && (
                      <span className="p-1.5 rounded-lg bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
                        <ArrowDownLeft size={14} />
                      </span>
                    )}
                    {tx.type === 'withdrawal' && (
                      <span className="p-1.5 rounded-lg bg-indigo-500/10 text-indigo-600 dark:text-indigo-400">
                        <ArrowUpRight size={14} />
                      </span>
                    )}
                    {tx.type === 'rebalance' && (
                      <span className="p-1.5 rounded-lg bg-amber-500/10 text-amber-600 dark:text-amber-400">
                        <RefreshCw size={14} />
                      </span>
                    )}
                    <span className="capitalize font-sans font-medium text-slate-800 dark:text-slate-200">{tx.type}</span>
                  </div>
                </td>
                <td className="py-3 px-4 font-semibold text-slate-900 dark:text-white">
                  {tx.amount.toLocaleString()} {t('usdc')}
                </td>
                <td className="py-3 px-4 text-slate-600 dark:text-slate-400">
                  <a
                    href={`https://stellar.expert/explorer/testnet/tx/${tx.txHash}`}
                    target="_blank"
                    rel="noreferrer"
                    className="flex items-center gap-1 hover:text-emerald-500 dark:hover:text-emerald-400 transition-colors"
                  >
                    <span>{tx.txHash}</span>
                    <ExternalLink size={12} />
                  </a>
                </td>
                <td className="py-3 px-4 text-slate-600 dark:text-slate-400">{tx.timestamp}</td>
                <td className="py-3 px-4 text-right">
                  <span className="inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-semibold bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20">
                    {tx.status}
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {filteredTransactions.length === 0 && (
        <div className="text-center py-8 text-slate-600 dark:text-slate-400 text-sm" role="status" aria-live="polite">
          {t('noTransactionsMatch')}
        </div>
      )}
    </div>
  );
};
