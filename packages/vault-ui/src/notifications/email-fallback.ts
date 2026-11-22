import type { EmailAdapter, EmailPayload } from './types';

export interface EmailAdapterConfig {
  provider: 'resend' | 'sendgrid' | 'console';
  apiKey?: string;
  from?: string;
  /** Override the provider URL (use a backend proxy in production). */
  endpoint?: string;
  fetchImpl?: typeof fetch;
}

export function createEmailAdapter(config: EmailAdapterConfig): EmailAdapter {
  const fetchImpl = config.fetchImpl ?? (typeof fetch === 'function' ? fetch.bind(globalThis) : undefined);

  return {
    async send(payload: EmailPayload) {
      if (config.provider === 'console' || !fetchImpl || !config.apiKey) {
        return Boolean(payload.to && payload.subject);
      }

      if (config.provider === 'resend') {
        const response = await fetchImpl(config.endpoint ?? 'https://api.resend.com/emails', {
          method: 'POST',
          headers: {
            Authorization: `Bearer ${config.apiKey}`,
            'Content-Type': 'application/json',
          },
          body: JSON.stringify({
            from: config.from,
            to: [payload.to],
            subject: payload.subject,
            text: payload.text,
          }),
        });
        return response.ok;
      }

      const response = await fetchImpl(config.endpoint ?? 'https://api.sendgrid.com/v3/mail/send', {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${config.apiKey}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          personalizations: [{ to: [{ email: payload.to }] }],
          from: { email: config.from },
          subject: payload.subject,
          content: [{ type: 'text/plain', value: payload.text }],
        }),
      });
      return response.ok;
    },
  };
}
