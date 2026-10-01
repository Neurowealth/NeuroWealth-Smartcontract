'use client';

import React, { useEffect } from 'react';
import { AlertOctagon, RefreshCw, RotateCcw } from 'lucide-react';
import { getSanitizedErrorInfo } from '@/lib/errorSanitizer';
import { reportErrorToObservability } from '@/lib/observability';

interface GlobalErrorProps {
  error: Error & { digest?: string };
  reset: () => void;
}

export default function GlobalError({ error, reset }: GlobalErrorProps) {
  useEffect(() => {
    reportErrorToObservability(error, {
      surface: 'global-root',
      digest: error.digest,
    });
  }, [error]);

  const errorInfo = getSanitizedErrorInfo(error, 'global');

  return (
    <html lang="en">
      <body className="antialiased bg-[#080b11] text-slate-100 min-h-screen flex flex-col justify-center items-center px-4 py-12">
        <div
          role="alert"
          aria-live="assertive"
          className="w-full max-w-lg rounded-3xl p-8 border border-red-500/20 bg-slate-900/90 text-center space-y-6 shadow-2xl"
        >
          <div className="mx-auto w-16 h-16 rounded-2xl bg-red-500/10 border border-red-500/20 flex items-center justify-center text-red-400">
            <AlertOctagon size={32} />
          </div>

          <div className="space-y-2">
            <h1 className="text-2xl sm:text-3xl font-extrabold text-white">
              Critical System Error
            </h1>
            <p className="text-sm sm:text-base text-slate-300">
              {errorInfo.message}
            </p>
            {error.digest && (
              <p className="text-xs font-mono text-slate-400 bg-slate-950 py-1 px-3 rounded-lg border border-slate-800 inline-block mt-2">
                Reference: {error.digest}
              </p>
            )}
          </div>

          <div className="pt-2 flex flex-col sm:flex-row items-center justify-center gap-3">
            <button
              type="button"
              onClick={reset}
              className="w-full sm:w-auto min-w-[140px] flex items-center justify-center gap-2 bg-red-600 hover:bg-red-500 text-white font-bold px-5 py-3 rounded-xl text-sm transition-all cursor-pointer"
            >
              <RefreshCw size={16} />
              Try Again
            </button>

            <button
              type="button"
              onClick={() => window.location.reload()}
              className="w-full sm:w-auto min-w-[140px] flex items-center justify-center gap-2 bg-slate-800 hover:bg-slate-700 text-slate-200 font-semibold px-5 py-3 rounded-xl text-sm border border-slate-700 transition-all cursor-pointer"
            >
              <RotateCcw size={16} />
              Reload Page
            </button>
          </div>
        </div>
      </body>
    </html>
  );
}
