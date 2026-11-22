import '@testing-library/jest-dom/vitest';
import { vi } from 'vitest';
import enMessages from '../../messages/en.json';

vi.mock('next-intl', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('next-intl');
  return {
    ...actual,
    useTranslations: (namespace?: string) => {
      const messages = namespace ? (enMessages as Record<string, any>)[namespace] || {} : enMessages;
      return (key: string, values?: Record<string, unknown>) => {
        let val = messages[key] || key;
        if (values && typeof val === 'string') {
          Object.entries(values).forEach(([k, v]) => {
            val = val.replace(`{${k}}`, String(v));
          });
        }
        return val;
      };
    },
    useLocale: () => 'en',
  };
});

process.env.NEXT_PUBLIC_SOROBAN_RPC_URL = process.env.NEXT_PUBLIC_SOROBAN_RPC_URL || 'https://soroban-testnet.stellar.org';
process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE = process.env.NEXT_PUBLIC_SOROBAN_NETWORK_PASSPHRASE || 'Test SDF Network ; September 2015';
process.env.NEXT_PUBLIC_VAULT_CONTRACT_ID = process.env.NEXT_PUBLIC_VAULT_CONTRACT_ID && process.env.NEXT_PUBLIC_VAULT_CONTRACT_ID.startsWith('C') && process.env.NEXT_PUBLIC_VAULT_CONTRACT_ID !== 'CDLZFC3SYJYD7M6LJEFAPCHRLHAFKP6WYTHRF3EGO5CYD3EP4GZGM37T'
  ? process.env.NEXT_PUBLIC_VAULT_CONTRACT_ID
  : 'CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4';

