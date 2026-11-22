'use client';

import React from 'react';
import { Sun, Moon, Monitor } from 'lucide-react';
import { useTheme } from './ThemeProvider';

export function ThemeToggle() {
  const { theme, setTheme } = useTheme();

  const options = [
    { value: 'light' as const, icon: Sun, label: 'Light' },
    { value: 'dark' as const, icon: Moon, label: 'Dark' },
    { value: 'system' as const, icon: Monitor, label: 'System' },
  ];

  return (
    <div className="flex items-center gap-1 p-1 rounded-xl bg-white/5 dark:bg-white/5 bg-slate-100 border border-slate-200 dark:border-white/10">
      {options.map(({ value, icon: Icon, label }) => (
        <button
          key={value}
          onClick={() => setTheme(value)}
          className={`p-2 rounded-lg transition-all duration-200 ${
            theme === value
              ? 'bg-emerald-500/20 text-emerald-400 shadow-sm'
              : 'text-slate-400 hover:text-slate-200 hover:bg-white/5'
          }`}
          aria-label={`${label} mode`}
          title={`${label} mode`}
        >
          <Icon size={16} />
        </button>
      ))}
    </div>
  );
}
