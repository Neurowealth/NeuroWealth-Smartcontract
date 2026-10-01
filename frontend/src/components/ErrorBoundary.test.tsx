import React, { useState } from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ErrorBoundary } from './ErrorBoundary';

// Helper component that can throw on demand
function CrashingComponent({ shouldThrow, message }: { shouldThrow: boolean; message?: string }) {
  if (shouldThrow) {
    throw new Error(message || 'Render explosion in child');
  }
  return <div data-testid="child-content">Healthy Child Content</div>;
}

// Stateful parent component to test dynamic recovery
function RecoveryHarness({ initialThrow = true }: { initialThrow?: boolean }) {
  const [hasCrashingBug, setHasCrashingBug] = useState(initialThrow);

  return (
    <div>
      <button data-testid="fix-bug-btn" onClick={() => setHasCrashingBug(false)}>
        Fix Bug
      </button>
      <ErrorBoundary
        surfaceName="portfolio"
        onReset={() => {
          // In real usage, onReset might re-fetch data or clear local state
        }}
      >
        <CrashingComponent shouldThrow={hasCrashingBug} />
      </ErrorBoundary>
    </div>
  );
}

describe('ErrorBoundary', () => {
  beforeEach(() => {
    // Suppress console.error in tests since React logs caught boundary errors
    vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  it('renders children when no error occurs', () => {
    render(
      <ErrorBoundary surfaceName="portfolio">
        <CrashingComponent shouldThrow={false} />
      </ErrorBoundary>
    );

    expect(screen.getByTestId('child-content')).toHaveTextContent('Healthy Child Content');
  });

  it('catches render error and displays the default fallback UI with retry button', () => {
    render(
      <ErrorBoundary surfaceName="portfolio">
        <CrashingComponent shouldThrow={true} message="RPC endpoint failed" />
      </ErrorBoundary>
    );

    expect(screen.queryByTestId('child-content')).toBeNull();
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Unable to load portfolio data')).toBeInTheDocument();
    expect(screen.getByText('RPC endpoint failed')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /retry/i })).toBeInTheDocument();
  });

  it('resets error state when Retry is clicked and renders recovered content', () => {
    render(<RecoveryHarness initialThrow={true} />);

    // Initially in error state
    expect(screen.getByRole('alert')).toBeInTheDocument();

    // Fix the bug condition
    fireEvent.click(screen.getByTestId('fix-bug-btn'));

    // Click retry
    const retryBtn = screen.getByRole('button', { name: /retry/i });
    fireEvent.click(retryBtn);

    // Verify recovery without full page refresh
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByTestId('child-content')).toHaveTextContent('Healthy Child Content');
  });

  it('isolates failures with nested boundaries: inner crash does not unmount outer boundary', () => {
    render(
      <ErrorBoundary surfaceName="outer-dashboard">
        <div data-testid="outer-content">Outer Content Is Intact</div>

        <ErrorBoundary surfaceName="transactions" compact>
          <CrashingComponent shouldThrow={true} message="Transaction history corrupted" />
        </ErrorBoundary>

        <div data-testid="sibling-content">Sibling Component Is Live</div>
      </ErrorBoundary>
    );

    // Outer and sibling components remain fully mounted and rendered
    expect(screen.getByTestId('outer-content')).toBeInTheDocument();
    expect(screen.getByTestId('sibling-content')).toBeInTheDocument();

    // Inner boundary renders compact alert
    expect(screen.getByText('Unable to load transactions')).toBeInTheDocument();
    expect(screen.getByText('Transaction history corrupted')).toBeInTheDocument();
  });

  it('invokes onError callback when an error is caught', () => {
    const onErrorSpy = vi.fn();

    render(
      <ErrorBoundary surfaceName="strategies" onError={onErrorSpy}>
        <CrashingComponent shouldThrow={true} message="Strategy calc error" />
      </ErrorBoundary>
    );

    expect(onErrorSpy).toHaveBeenCalledTimes(1);
    expect(onErrorSpy.mock.calls[0][0].message).toBe('Strategy calc error');
  });

  it('supports custom functional fallback with reset trigger', () => {
    render(
      <ErrorBoundary
        fallback={({ error, reset }) => (
          <div data-testid="custom-fallback">
            <span>Custom: {error.message}</span>
            <button onClick={reset}>Custom Retry</button>
          </div>
        )}
      >
        <CrashingComponent shouldThrow={true} message="Custom failure" />
      </ErrorBoundary>
    );

    expect(screen.getByTestId('custom-fallback')).toHaveTextContent('Custom: Custom failure');
    expect(screen.getByRole('button', { name: 'Custom Retry' })).toBeInTheDocument();
  });

  it('redacts sensitive secret keys in fallback message', () => {
    const secretKey = 'SA4N7X3U5D2H67AB2Z2XNJWOUJMYZKLX2R5R32NKV7F6O2V7J4OQ5OOU';
    render(
      <ErrorBoundary surfaceName="portfolio">
        <CrashingComponent shouldThrow={true} message={`Network error with key ${secretKey}`} />
      </ErrorBoundary>
    );

    const alert = screen.getByRole('alert');
    expect(alert).not.toHaveTextContent(secretKey);
    expect(alert).toHaveTextContent('[REDACTED_SECRET_KEY]');
  });

  describe('viewport and layout responsiveness', () => {
    it('renders accessible mobile touch targets and clean layout in narrow containers (375px)', () => {
      render(
        <div style={{ width: '375px' }} data-testid="mobile-viewport">
          <ErrorBoundary surfaceName="portfolio">
            <CrashingComponent shouldThrow={true} message="Mobile network disconnect" />
          </ErrorBoundary>
        </div>
      );

      const alert = screen.getByRole('alert');
      expect(alert).toBeInTheDocument();
      expect(alert).toHaveClass('glass-panel');

      const retryBtn = screen.getByRole('button', { name: /retry surface/i });
      expect(retryBtn).toBeInTheDocument();
      // On mobile, the button has full-width responsive classes for touch targets
      expect(retryBtn).toHaveClass('w-full', 'sm:w-auto');
    });

    it('renders compact mode with full-width touch targets in narrow containers', () => {
      render(
        <div style={{ width: '320px' }} data-testid="compact-mobile-viewport">
          <ErrorBoundary surfaceName="transactions" compact>
            <CrashingComponent shouldThrow={true} message="Failed to parse ledger records" />
          </ErrorBoundary>
        </div>
      );

      const alert = screen.getByRole('alert');
      expect(alert).toBeInTheDocument();

      const retryBtn = screen.getByRole('button', { name: /retry/i });
      expect(retryBtn).toBeInTheDocument();
      expect(retryBtn).toHaveClass('w-full', 'sm:w-auto');
    });
  });
});
