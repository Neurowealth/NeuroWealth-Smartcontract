/**
 * Shared test bootstrap for WhatsApp unit tests.
 *
 * Sets dummy environment variables when unset and replaces `./db` in the
 * CommonJS require cache with an in-memory `whatsapp_wallets` store so tests
 * run in a clean checkout without a live PostgreSQL database or deployment
 * secrets.
 */

process.env.ENCRYPTION_KEY ??= 'test-encryption-key-at-least-32-chars!!';
process.env.PHONE_HASH_SALT ??= 'test-phone-salt';
process.env.SOROBAN_RPC_URL ??= 'https://soroban-testnet.stellar.org';
process.env.VAULT_CONTRACT_ID ??= 'CTEST_VAULT_CONTRACT_ID';

interface WalletRow {
  phone_hash: string;
  public_key: string;
  encrypted_data: string;
  iv: string;
  tag: string;
  created_at: string;
  wrapped_dek: string | null;
  dek_iv: string | null;
  dek_tag: string | null;
  key_version: string | null;
}

const wallets = new Map<string, WalletRow>();
const deliveries = new Map<string, { status: string }>();

async function inMemoryQuery(sql: string, params: unknown[] = []): Promise<{ rows: unknown[]; rowCount?: number }> {
  const normalized = sql.replace(/\s+/g, ' ').trim().toUpperCase();

  if (normalized.startsWith('INSERT INTO WHATSAPP_WALLETS')) {
    const [
      phoneHash,
      publicKey,
      encryptedData,
      iv,
      tag,
      createdAtMs,
      wrappedDek,
      dekIv,
      dekTag,
      keyVersion,
    ] = params as [
      string,
      string,
      string,
      string,
      string,
      number,
      string | undefined,
      string | undefined,
      string | undefined,
      string | undefined,
    ];

    if (wallets.has(phoneHash)) {
      return { rows: [] };
    }

    const row: WalletRow = {
      phone_hash: phoneHash,
      public_key: publicKey,
      encrypted_data: encryptedData,
      iv,
      tag,
      created_at: new Date(typeof createdAtMs === 'number' ? createdAtMs : Date.now()).toISOString(),
      wrapped_dek: wrappedDek ?? null,
      dek_iv: dekIv ?? null,
      dek_tag: dekTag ?? null,
      key_version: keyVersion ?? null,
    };
    wallets.set(phoneHash, row);
    return { rows: [row] };
  }

  if (normalized.startsWith('SELECT * FROM WHATSAPP_WALLETS')) {
    const [phoneHash] = params as [string];
    const row = wallets.get(phoneHash);
    return { rows: row ? [row] : [] };
  }

  if (normalized.startsWith('SELECT STATUS FROM WHATSAPP_MESSAGE_DELIVERIES')) {
    const [messageSid] = params as [string];
    const row = deliveries.get(messageSid);
    return { rows: row ? [row] : [] };
  }

  if (normalized.startsWith('INSERT INTO WHATSAPP_MESSAGE_DELIVERIES')) {
    const [messageSid, , , , , status] = params as [string, unknown, unknown, unknown, unknown, string];
    if (deliveries.has(messageSid)) {
      return { rows: [], rowCount: 0 };
    }
    deliveries.set(messageSid, { status });
    return { rows: [], rowCount: 1 };
  }

  throw new Error(`inMemoryQuery: unrecognized SQL: ${sql}`);
}

const dbModulePath = require.resolve('./db');
require.cache[dbModulePath] = {
  id: dbModulePath,
  filename: dbModulePath,
  loaded: true,
  exports: { query: inMemoryQuery },
  parent: null,
  children: [],
  paths: [],
} as unknown as NodeModule;
