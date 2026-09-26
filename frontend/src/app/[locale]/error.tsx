'use client';

import React, { useEffect } from 'react';
import Link from 'next/link';
import { AlertOctagon, RefreshCw, Home, RotateCcw } from 'lucide-react';
import { getSanitizedErrorInfo } from '@/lib/errorSanitizer';
import { reportErrorToObservability } from '@/lib/observability';

interface RouteErrorProps {
  error: Error & { digest?: string };
  reset: () => void;
}

export default function RouteError({ error, reset }: RouteErrorProps) {
  useEffect(() => {
    reportErrorToObservability(error, {
      surface: 'route-primary',
      digest: error.digest,
    });
  }, [error]);

  const errorInfo = getSanitizedErrorInfo(error, 'route');

  return (
    <div className="min-h-screen bg-[#080b11] text-slate-100 flex flex-col justify-center items-center px-4 py-12 sm:px-6 lg:px-8 relative overflow-hidden">
      {/* Background ambient radial glow */}
      <div className="fixed inset-0 bg-[radial-gradient(ellipse_80%_80%_at_50%_-20%,rgba(239,68,68,0.12),rgba(255,255,255,0))] pointer-events-none z-0" />

      <div
        role="alert"
        aria-live="assertive"
        className="w-full max-w-lg glass-panel rounded-3xl p-6 sm:p-10 border border-red-500/20 bg-gradient-to-b from-red-950/20 via-slate-900/80 to-slate-950/90 shadow-2xl relative z-10 space-y-6 text-center"
      >
        <div className="mx-auto w-16 h-16 rounded-2xl bg-red-500/10 border border-red-500/20 flex items-center justify-center text-red-400 shadow-glow-red">
          <AlertOctagon size={32} />
        </div>

        <div className="space-y-2">
          <h1 className="text-2xl sm:text-3xl font-extrabold text-white tracking-tight">
            Something went wrong
          </h1>
          <p className="text-sm sm:text-base text-slate-300 leading-relaxed">
            {errorInfo.message}
          </p>
          {error.digest && (
            <p className="text-xs font-mono text-slate-400 bg-slate-950/60 py-1.5 px-3 rounded-lg border border-slate-800/80 inline-block mt-2 break-all max-w-full">
              Error Digest: {error.digest}
            </p>
          )}
        </div>

        <div className="pt-2 flex flex-col sm:flex-row items-center justify-center gap-3">
          <button
            type="button"
            onClick={reset}
            className="w-full sm:w-auto min-w-[140px] flex items-center justify-center gap-2 bg-gradient-to-r from-red-500 to-rose-600 hover:from-red-400 hover:to-rose-500 text-white font-bold px-5 py-3 rounded-xl text-sm transition-all shadow-lg active:scale-95 cursor-pointer"
          >
            <RefreshCw size={16} />
            Try Again
          </button>

          <button
            type="button"
            onClick={() => window.location.reload()}
            className="w-full sm:w-auto min-w-[140px] flex items-center justify-center gap-2 bg-slate-800/80 hover:bg-slate-700/80 text-slate-200 font-semibold px-5 py-3 rounded-xl text-sm border border-slate-700/60 transition-all active:scale-95 cursor-pointer"
          >
            <RotateCcw size={16} />
            Reload Page
          </button>
        </div>

        <div className="pt-4 border-t border-slate-800/60">
          <Link
            href="/"
            className="inline-flex items-center gap-2 text-xs font-medium text-slate-400 hover:text-emerald-400 transition-colors"
          >
            <Home size={14} />
            Back to Dashboard
          </Link>
        </div>
      </div>
    </div>
  );
}
