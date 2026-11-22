/**
 * KEK rotation for custodial wallet secrets (#944).
 *
 * `rotateWalletKeys` walks `whatsapp_wallets` in primary-key order and brings
 * every row up to the current KEK (see keyring.ts / docs/WHATSAPP_BOT.md):
 *
 *   - envelope rows wrapped by an older KEK: only the DEK is re-wrapped;
 *   - legacy rows (no envelope): decrypted once, re-encrypted into an envelope.
 *
 * It is idempotent and resumable (rows already on the current version are not
 * selected), safe to run while the bot is serving traffic (each UPDATE is
 * conditional on the row still holding the ciphertext that was read), and
 * never logs key material or secrets.
 */
import { query } from './db';
import { EncryptedSecret, rewrapSecret } from './cryptoUtils';
import { loadKeyring } from './keyring';

export type QueryFn = (text: string, params?: any[]) => Promise<{ rows: any[]; rowCount?: number | null }>;

export interface RotationOptions {
  batchSize?: number;
  /** Verify every row can be re-wrapped but write nothing. */
  dryRun?: boolean;
  queryFn?: QueryFn;
}

export interface RotationResult {
  targetVersion: string;
  rotated: number;
  /** Rows changed by someone else between read and write; picked up by a re-run. */
  skipped: number;
  /** Rows that could not be re-wrapped (e.g. their KEK is missing from the ring). */
  failed: number;
}

interface WalletRow {
  phone_hash: string;
  encrypted_data: string;
  iv: string;
  tag: string;
  wrapped_dek: string | null;
  dek_iv: string | null;
  dek_tag: string | null;
  key_version: string | null;
}

export async function rotateWalletKeys(options: RotationOptions = {}): Promise<RotationResult> {
  const { batchSize = 100, dryRun = false, queryFn = query as QueryFn } = options;
  const targetVersion = loadKeyring().current.version;
  const result: RotationResult = { targetVersion, rotated: 0, skipped: 0, failed: 0 };

  let cursor = '';
  for (;;) {
    const batch = await queryFn(
      `SELECT phone_hash, encrypted_data, iv, tag, wrapped_dek, dek_iv, dek_tag, key_version
         FROM whatsapp_wallets
        WHERE (key_version IS NULL OR key_version <> $1) AND phone_hash > $2
        ORDER BY phone_hash
        LIMIT $3`,
      [targetVersion, cursor, batchSize]
    );
    if (batch.rows.length === 0) break;

    for (const row of batch.rows as WalletRow[]) {
      cursor = row.phone_hash;

      let next;
      try {
        const stored: EncryptedSecret = {
          encryptedData: row.encrypted_data,
          iv: row.iv,
          tag: row.tag,
          wrappedDek: row.wrapped_dek ?? undefined,
          dekIv: row.dek_iv ?? undefined,
          dekTag: row.dek_tag ?? undefined,
          keyVersion: row.key_version ?? undefined,
        };
        next = rewrapSecret(stored);
      } catch {
        result.failed += 1;
        continue;
      }

      if (dryRun) {
        result.rotated += 1;
        continue;
      }

      const updated = await queryFn(
        `UPDATE whatsapp_wallets
            SET encrypted_data = $2, iv = $3, tag = $4,
                wrapped_dek = $5, dek_iv = $6, dek_tag = $7, key_version = $8
          WHERE phone_hash = $1
            AND encrypted_data = $9
            AND key_version IS NOT DISTINCT FROM $10`,
        [
          row.phone_hash,
          next.encryptedData,
          next.iv,
          next.tag,
          next.wrappedDek,
          next.dekIv,
          next.dekTag,
          next.keyVersion,
          row.encrypted_data,
          row.key_version,
        ]
      );
      if ((updated.rowCount ?? 0) > 0) result.rotated += 1;
      else result.skipped += 1;
    }
  }

  return result;
}
