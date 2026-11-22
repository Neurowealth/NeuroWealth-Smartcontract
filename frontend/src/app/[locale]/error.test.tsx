import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import RouteError from './error';

describe('RouteError component', () => {
  beforeEach(() => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
  });

  it('renders a usable recovery screen with title, sanitized message, and actions', () => {
    const error = Object.assign(new Error('Failed to load locale chunks'), { digest: 'chunk-998877' });
    const reset = vi.fn();

    render(<RouteError error={error} reset={reset} />);

    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByText('Something went wrong')).toBeInTheDocument();
    expect(screen.getByText('Failed to load locale chunks')).toBeInTheDocument();
    expect(screen.getByText(/Error Digest: chunk-998877/)).toBeInTheDocument();

    const tryAgainBtn = screen.getByRole('button', { name: /try again/i });
    expect(tryAgainBtn).toBeInTheDocument();

    const reloadBtn = screen.getByRole('button', { name: /reload page/i });
    expect(reloadBtn).toBeInTheDocument();

    const dashboardLink = screen.getByRole('link', { name: /back to dashboard/i });
    expect(dashboardLink).toBeInTheDocument();
    expect(dashboardLink).toHaveAttribute('href', '/');
  });

  it('invokes the reset callback when Try Again is clicked', () => {
    const error = new Error('Database connection failed');
    const reset = vi.fn();

    render(<RouteError error={error} reset={reset} />);

    const tryAgainBtn = screen.getByRole('button', { name: /try again/i });
    fireEvent.click(tryAgainBtn);

    expect(reset).toHaveBeenCalledTimes(1);
  });

  it('redacts sensitive credentials and secret keys from the error message on the route recovery screen', () => {
    const secretKey = 'SA4N7X3U5D2H67AB2Z2XNJWOUJMYZKLX2R5R32NKV7F6O2V7J4OQ5OOU';
    const bearerToken = 'Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.xyz';
    const error = new Error(`Authentication failed with key=${secretKey} and ${bearerToken}`);
    const reset = vi.fn();

    render(<RouteError error={error} reset={reset} />);

    const alert = screen.getByRole('alert');
    expect(alert).not.toHaveTextContent(secretKey);
    expect(alert).not.toHaveTextContent(bearerToken);
    expect(alert).toHaveTextContent('[REDACTED_SECRET_KEY]');
    expect(alert).toHaveTextContent('[REDACTED_CREDENTIAL]');
  });
});
