import { Keypair } from '@stellar/stellar-sdk';
import { encryptSecretKey, decryptSecretKey } from './cryptoUtils';
import { query } from './db';

export interface UserWallet {
  publicKey: string;
  encryptedSecret: {
    encryptedData: string;
    iv: string;
    tag: string;
  };
  createdAt: number;
}

export interface CustodialWalletResult {
  wallet: UserWallet;
  /** False when phoneHash already had a wallet; the fresh keypair generated
   * this call was discarded and the pre-existing stored wallet is returned
   * instead. */
  isNew: boolean;
}

/**
 * Returns the custodial Stellar wallet for a verified user, creating one if
 * none exists yet.
 *
 * A fresh keypair is always generated up front, but `INSERT ... ON CONFLICT
 * DO NOTHING RETURNING *` means the generated keypair is only ever persisted
 * (and only ever returned) when no row already existed for phoneHash. For a
 * returning user - e.g. a session that timed out and re-verified - the
 * insert is a no-op and the pre-existing stored wallet is read back via
 * getWallet instead, so the public key returned always matches a secret key
 * that was actually saved (#1000). The fresh keypair generated for a no-op
 * insert is discarded in memory and never shown to the caller.
 */
export async function createCustodialWallet(phoneHash: string): Promise<CustodialWalletResult> {
  const pair = Keypair.random();
  const publicKey = pair.publicKey();
  const secretKey = pair.secret();

  const encryptedSecret = encryptSecretKey(secretKey);
  const createdAt = Date.now();

  const result = await query(
    `INSERT INTO whatsapp_wallets (phone_hash, public_key, encrypted_data, iv, tag, created_at)
     VALUES ($1, $2, $3, $4, $5, to_timestamp($6 / 1000.0))
     ON CONFLICT (phone_hash) DO NOTHING
     RETURNING public_key, encrypted_data, iv, tag, created_at`,
    [
      phoneHash,
      publicKey,
      encryptedSecret.encryptedData,
      encryptedSecret.iv,
      encryptedSecret.tag,
      createdAt
    ]
  );

  if (result.rows.length > 0) {
    return {
      wallet: { publicKey, encryptedSecret, createdAt },
      isNew: true
    };
  }

  const existing = await getWallet(phoneHash);
  if (!existing) {
    // The insert found a conflicting row (so it did not insert), but that
    // row is now gone - a concurrent delete between the two queries.
    // Extremely unlikely, but surfacing it as an error is safer than
    // silently handing back the discarded fresh keypair as if it were saved.
    throw new Error(`createCustodialWallet: no wallet found for phoneHash after a no-op insert`);
  }
  return { wallet: existing, isNew: false };
}

/**
 * Retrieves a user's wallet info (public key and encrypted secret).
 */
export async function getWallet(phoneHash: string): Promise<UserWallet | undefined> {
  const res = await query('SELECT * FROM whatsapp_wallets WHERE phone_hash = $1', [phoneHash]);
  if (res.rows.length === 0) return undefined;

  const row = res.rows[0];
  return {
    publicKey: row.public_key,
    encryptedSecret: {
      encryptedData: row.encrypted_data,
      iv: row.iv,
      tag: row.tag
    },
    createdAt: new Date(row.created_at).getTime()
  };
}

/**
 * Decrypts secret key for transaction execution. Key is never saved in plaintext or output to chat.
 */
export async function getDecryptedSecretKey(phoneHash: string): Promise<string | null> {
  const wallet = await getWallet(phoneHash);
  if (!wallet) return null;

  return decryptSecretKey(
    wallet.encryptedSecret.encryptedData,
    wallet.encryptedSecret.iv,
    wallet.encryptedSecret.tag
  );
}
