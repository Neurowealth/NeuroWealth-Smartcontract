/**
 * Frontend Observability Dispatcher
 *
 * Dispatches sanitized error reports to the configured telemetry / observability path.
 * Supports remote webhook URLs, window error handlers, or safe fallback logging.
 */

import { getSanitizedErrorInfo, redactSensitiveData } from './errorSanitizer';

export interface ObservabilityPayload {
  timestamp: string;
  surface: string;
  title: string;
  message: string;
  digest?: string;
  context?: Record<string, unknown>;
  environment: string;
}

export function reportErrorToObservability(
  error: unknown,
  context?: { surface?: string; digest?: string; [key: string]: unknown }
): ObservabilityPayload {
  const surface = context?.surface || 'unspecified';
  const info = getSanitizedErrorInfo(error, surface);

  // Sanitize any extra context values so secrets aren't smuggled in metadata
  const sanitizedContext: Record<string, unknown> = {};
  if (context) {
    for (const [key, val] of Object.entries(context)) {
      if (typeof val === 'string') {
        sanitizedContext[key] = redactSensitiveData(val);
      } else if (typeof val === 'number' || typeof val === 'boolean') {
        sanitizedContext[key] = val;
      }
    }
  }

  const payload: ObservabilityPayload = {
    timestamp: new Date().toISOString(),
    surface,
    title: info.title,
    message: info.message,
    digest: context?.digest || info.digest,
    context: sanitizedContext,
    environment: process.env.NODE_ENV || 'development',
  };

  // 1. Dispatch to remote observability URL if configured
  const endpoint = process.env.NEXT_PUBLIC_OBSERVABILITY_URL;
  if (endpoint && typeof window !== 'undefined') {
    try {
      const serialized = JSON.stringify(payload);
      if (typeof navigator !== 'undefined' && typeof navigator.sendBeacon === 'function') {
        const blob = new Blob([serialized], { type: 'application/json' });
        navigator.sendBeacon(endpoint, blob);
      } else {
        fetch(endpoint, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: serialized,
          keepalive: true,
        }).catch(() => {
          // Degrade silently if network drop
        });
      }
    } catch {
      // Degrade silently
    }
  }

  // 2. Dispatch to custom window handler if attached
  if (typeof window !== 'undefined') {
    const customHandler = (window as any).__OBSERVABILITY_HANDLER__;
    if (typeof customHandler === 'function') {
      try {
        customHandler(payload);
      } catch {
        // Do not let handler failures bubble
      }
    }
  }

  // 3. Fallback safe console log
  if (process.env.NODE_ENV !== 'production' || process.env.NEXT_PUBLIC_DEBUG_ERRORS === 'true') {
    console.error(`[Observability:${surface}] ${payload.title} — ${payload.message}`, payload.digest ? `(digest: ${payload.digest})` : '');
  }

  return payload;
}
