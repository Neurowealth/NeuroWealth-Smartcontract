'use client';

import React, { Component, type ErrorInfo, type ReactNode } from 'react';
import { AlertTriangle, RefreshCw } from 'lucide-react';
import { getSanitizedErrorInfo } from '@/lib/errorSanitizer';
import { reportErrorToObservability } from '@/lib/observability';

export interface ErrorBoundaryFallbackProps {
  error: Error;
  reset: () => void;
  surfaceName?: string;
}

export interface ErrorBoundaryProps {
  children: ReactNode;
  fallback?: ReactNode | ((props: ErrorBoundaryFallbackProps) => ReactNode);
  title?: string;
  description?: string;
  surfaceName?: string;
  compact?: boolean;
  onReset?: () => void;
  onError?: (error: Error, errorInfo: ErrorInfo) => void;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = {
      hasError: false,
      error: null,
    };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return {
      hasError: true,
      error,
    };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    const { surfaceName = 'unknown-surface', onError } = this.props;

    reportErrorToObservability(error, {
      surface: surfaceName,
      componentStack: errorInfo.componentStack,
    });

    if (onError) {
      onError(error, errorInfo);
    }
  }

  reset = (): void => {
    this.setState({
      hasError: false,
      error: null,
    });

    if (this.props.onReset) {
      this.props.onReset();
    }
  };

  render(): ReactNode {
    const { hasError, error } = this.state;
    const { children, fallback, surfaceName, compact, title: customTitle, description: customDescription } = this.props;

    if (!hasError || !error) {
      return children;
    }

    if (typeof fallback === 'function') {
      return fallback({ error, reset: this.reset, surfaceName });
    }

    if (fallback) {
      return fallback;
    }

    const errorInfo = getSanitizedErrorInfo(error, surfaceName);
    const displayTitle = customTitle || errorInfo.title;
    const displayMessage = customDescription || errorInfo.message;

    if (compact) {
      return (
        <div
          role="alert"
          aria-live="assertive"
          className="glass-panel rounded-xl p-3 sm:p-4 border border-amber-500/30 bg-slate-900/80 text-slate-100 flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 shadow-md"
        >
          <div className="flex items-center gap-2.5 min-w-0 w-full sm:w-auto">
            <AlertTriangle className="text-amber-400 shrink-0" size={18} />
            <div className="min-w-0 flex-1">
              <p className="text-xs font-semibold text-white truncate">{displayTitle}</p>
              <p className="text-[11px] text-slate-300 break-words line-clamp-2">{displayMessage}</p>
            </div>
          </div>
          <button
            type="button"
            onClick={this.reset}
            className="w-full sm:w-auto inline-flex items-center justify-center gap-1.5 text-xs font-semibold px-3 py-2 rounded-lg bg-amber-500/20 hover:bg-amber-500/30 text-amber-300 border border-amber-500/30 transition-all shrink-0 active:scale-95 cursor-pointer"
          >
            <RefreshCw size={12} />
            Retry
          </button>
        </div>
      );
    }

    return (
      <div
        role="alert"
        aria-live="assertive"
        className="glass-panel rounded-2xl p-5 sm:p-8 border border-amber-500/20 bg-gradient-to-r from-amber-950/20 via-slate-900/60 to-slate-950/40 text-slate-100 shadow-glow-amber my-4"
      >
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
          <div className="flex items-start gap-3 sm:gap-4 min-w-0">
            <div className="p-2.5 sm:p-3 rounded-xl bg-amber-500/10 text-amber-400 border border-amber-500/20 shrink-0">
              <AlertTriangle size={22} className="sm:w-6 sm:h-6" />
            </div>
            <div className="space-y-1 min-w-0 flex-1">
              <h3 className="text-base sm:text-lg font-bold text-white tracking-tight">
                {displayTitle}
              </h3>
              <p className="text-xs sm:text-sm text-slate-300 leading-relaxed max-w-xl break-words">
                {displayMessage}
              </p>
              {errorInfo.digest && (
                <p className="text-[11px] text-slate-400 font-mono pt-1 break-all max-w-full">
                  Reference: {errorInfo.digest}
                </p>
              )}
            </div>
          </div>

          <div className="w-full sm:w-auto flex flex-col sm:flex-row gap-2 shrink-0 pt-2 sm:pt-0">
            <button
              type="button"
              onClick={this.reset}
              className="w-full sm:w-auto inline-flex items-center justify-center gap-2 bg-gradient-to-r from-amber-500 to-amber-600 hover:from-amber-400 hover:to-amber-500 text-slate-950 font-bold px-5 py-2.5 rounded-xl text-xs sm:text-sm transition-all shadow-md active:scale-95"
            >
              <RefreshCw size={14} />
              Retry Surface
            </button>
          </div>
        </div>
      </div>
    );
  }
}
