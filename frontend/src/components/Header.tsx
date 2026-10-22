'use client';

import React from 'react';
import { Bot, Sparkles } from 'lucide-react';
import { WalletConnect } from './WalletConnect';
import { LanguageSwitcher } from './LanguageSwitcher';
import { ThemeToggle } from './ThemeToggle';

interface HeaderProps {
  publicKey: string | null;
  onConnect: () => void;
  onDisconnect: () => void;
}

export const Header: React.FC<HeaderProps> = ({ publicKey, onConnect, onDisconnect }) => {
  return (
    <header className="sticky top-0 z-50 w-full border-b border-slate-200 dark:border-slate-800/80 bg-white/80 dark:bg-[#080b11]/80 backdrop-blur-xl">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-20 flex items-center justify-between">
        {/* Logo */}
        <div className="flex items-center gap-3">
          <div className="h-10 w-10 rounded-xl bg-gradient-to-tr from-emerald-500 to-indigo-600 p-0.5 shadow-glow-emerald flex items-center justify-center">
            <div className="h-full w-full bg-slate-950 rounded-[10px] flex items-center justify-center">
              <Bot className="text-emerald-400" size={22} />
            </div>
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="font-bold text-xl tracking-tight bg-gradient-to-r from-slate-900 via-slate-800 to-slate-600 dark:from-white dark:via-slate-200 dark:to-slate-400 bg-clip-text text-transparent">
                NeuroWealth
              </span>
              <span className="text-[10px] font-semibold tracking-wider uppercase px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20 flex items-center gap-1">
                <Sparkles size={10} /> AI Agent
              </span>
            </div>
            <p className="text-xs text-slate-600 dark:text-slate-400">Autonomous DeFi Yield on Stellar</p>
          </div>
        </div>

        {/* Navigation */}
        <nav className="hidden md:flex items-center gap-8 text-sm font-medium text-slate-700 dark:text-slate-300">
          <a href="#dashboard" className="text-emerald-600 dark:text-emerald-400 hover:text-emerald-500 dark:hover:text-emerald-300 transition-colors">Dashboard</a>
          <a href="#strategies" className="hover:text-emerald-600 dark:hover:text-emerald-400 transition-colors">Strategies</a>
          <a href="#history" className="hover:text-emerald-600 dark:hover:text-emerald-400 transition-colors">Transactions</a>
          <a href="#whatsapp" className="hover:text-emerald-600 dark:hover:text-emerald-400 transition-colors">WhatsApp Bot</a>
        </nav>

        {/* Wallet Connection & Language */}
        <div className="flex items-center gap-4">
          <ThemeToggle />
          <LanguageSwitcher />
          <WalletConnect publicKey={publicKey} onConnect={onConnect} onDisconnect={onDisconnect} />
        </div>
      </div>
    </header>
  );
};
