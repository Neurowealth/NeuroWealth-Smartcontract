import crypto from 'crypto';
import { KeyEntry, loadKeyring } from './keyring';

const ALGORITHM = 'aes-256-gcm';

function getEnv(name: string): string {
  const value = process.env[name];
  if (!value) {
    throw new Error(`${name} must be set; refusing to use an unsafe production default`);
  }
  return value;
}

/** Validates the crypto-related environment; throws with a clear message when unusable. */
export function assertConfig(): void {
  loadKeyring();
  getEnv('PHONE_HASH_SALT');
}

/**
 * Hashes phone number to ensure PII privacy at rest.
 * Uses SHA-256 with a deployment-provided salt.
 */
export function hashPhoneNumber(phone: string): string {
  const salt = getEnv('PHONE_HASH_SALT');
  const hashedSalt = crypto.createHash('sha256').update(salt).digest('hex');
  return crypto.createHash('sha256').update(phone + hashedSalt).digest('hex');
}

/** Envelope fields persisted next to the ciphertext (#944). */
export interface EnvelopeFields {
  /** Per-wallet data-encryption key, AES-256-GCM-wrapped by the KEK. */
  wrappedDek: string;
  dekIv: string;
  dekTag: string;
  /** Version label of the KEK that wrapped the DEK. */
  keyVersion: string;
}

export interface EncryptedSecret extends Partial<EnvelopeFields> {
  encryptedData: string;
  iv: string;
  tag: string;
}

// ── KEK derivation / DEK wrapping ───────────────────────────────────────────
// wrapDek/unwrapDek are the KMS/HSM seam: a managed key service replaces just
// these two functions, with `keyVersion` carrying the provider's key id.

const kekCache = new Map<string, Buffer>();

function deriveKek(entry: KeyEntry): Buffer {
  const cacheKey = crypto.createHash('sha256').update(`${entry.version}\0${entry.material}`).digest('hex');
  let kek = kekCache.get(cacheKey);
  if (!kek) {
    kek = crypto.scryptSync(entry.material, `neurowealth-kek:${entry.version}`, 32);
    kekCache.set(cacheKey, kek);
  }
  return kek;
}

function wrapDek(dek: Buffer, entry: KeyEntry): Pick<EnvelopeFields, 'wrappedDek' | 'dekIv' | 'dekTag'> {
  const iv = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv(ALGORITHM, deriveKek(entry), iv);
  // Binding the version as AAD makes a swapped key_version label fail loudly.
  cipher.setAAD(Buffer.from(`kek:${entry.version}`));
  const wrapped = Buffer.concat([cipher.update(dek), cipher.final()]);
  return {
    wrappedDek: wrapped.toString('hex'),
    dekIv: iv.toString('hex'),
    dekTag: cipher.getAuthTag().toString('hex'),
  };
}

function unwrapDek(envelope: EnvelopeFields, entry: KeyEntry): Buffer {
  const decipher = crypto.createDecipheriv(ALGORITHM, deriveKek(entry), Buffer.from(envelope.dekIv, 'hex'));
  decipher.setAAD(Buffer.from(`kek:${entry.version}`));
  decipher.setAuthTag(Buffer.from(envelope.dekTag, 'hex'));
  return Buffer.concat([decipher.update(Buffer.from(envelope.wrappedDek, 'hex')), decipher.final()]);
}

function findKey(version: string): KeyEntry {
  const ring = loadKeyring();
  const entry = [ring.current, ...ring.retired].find((e) => e.version === version);
  if (!entry) {
    throw new Error(
      `No key configured for encryption key version "${version}"; add it to ENCRYPTION_KEY_RETIRED to decrypt this record`
    );
  }
  return entry;
}

function openWithDek(dek: Buffer, encryptedData: string, iv: string, tag: string): string {
  const decipher = crypto.createDecipheriv(ALGORITHM, dek, Buffer.from(iv, 'hex'));
  decipher.setAuthTag(Buffer.from(tag, 'hex'));
  let decrypted = decipher.update(encryptedData, 'hex', 'utf8');
  decrypted += decipher.final('utf8');
  return decrypted;
}

/** Pre-#944 records: AES key = scrypt(ENCRYPTION_KEY, 'salt'), no per-record DEK. */
function decryptLegacy(encryptedData: string, iv: string, tag: string): string {
  const ring = loadKeyring();
  let lastError: unknown;
  // Legacy rows carry no version label, so try the current key, then retired ones.
  for (const entry of [ring.current, ...ring.retired]) {
    try {
      return openWithDek(crypto.scryptSync(entry.material, 'salt', 32), encryptedData, iv, tag);
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError instanceof Error ? lastError : new Error('Unable to decrypt legacy secret');
}

/**
 * Encrypts sensitive custodial Stellar secret key before saving to storage.
 * A fresh DEK encrypts the secret and the current KEK wraps the DEK.
 */
export function encryptSecretKey(secretKey: string): EncryptedSecret & EnvelopeFields {
  const { current } = loadKeyring();
  const dek = crypto.randomBytes(32);
  const iv = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv(ALGORITHM, dek, iv);

  let encrypted = cipher.update(secretKey, 'utf8', 'hex');
  encrypted += cipher.final('hex');
  const tag = cipher.getAuthTag().toString('hex');

  return {
    encryptedData: encrypted,
    iv: iv.toString('hex'),
    tag,
    ...wrapDek(dek, current),
    keyVersion: current.version,
  };
}

/**
 * Decrypts encrypted custodial Stellar secret key for transaction signing.
 * Pass the envelope fields for records written since #944; omit them for
 * legacy records. Never log or expose the output of this function.
 */
export function decryptSecretKey(
  encryptedData: string,
  iv: string,
  tag: string,
  envelope?: Partial<EnvelopeFields>
): string {
  if (envelope?.wrappedDek && envelope.dekIv && envelope.dekTag && envelope.keyVersion) {
    const dek = unwrapDek(envelope as EnvelopeFields, findKey(envelope.keyVersion));
    return openWithDek(dek, encryptedData, iv, tag);
  }
  return decryptLegacy(encryptedData, iv, tag);
}

/** True when the record is already wrapped by the current KEK. */
export function isCurrentKeyVersion(secret: EncryptedSecret): boolean {
  return Boolean(secret.wrappedDek) && secret.keyVersion === loadKeyring().current.version;
}

/**
 * Brings a stored secret up to date with the current KEK. Envelope records
 * only get their DEK re-wrapped (ciphertext untouched); legacy records are
 * decrypted once and re-encrypted into the envelope format.
 */
export function rewrapSecret(secret: EncryptedSecret): EncryptedSecret & EnvelopeFields {
  const { current } = loadKeyring();

  if (secret.wrappedDek && secret.dekIv && secret.dekTag && secret.keyVersion) {
    const dek = unwrapDek(secret as EnvelopeFields, findKey(secret.keyVersion));
    return {
      encryptedData: secret.encryptedData,
      iv: secret.iv,
      tag: secret.tag,
      ...wrapDek(dek, current),
      keyVersion: current.version,
    };
  }

  return encryptSecretKey(decryptLegacy(secret.encryptedData, secret.iv, secret.tag));
}

// ── Short-lived field encryption (no per-record envelope columns) ───────────
// Used for values stored in a plain (data, iv, tag) triple, such as the
// recipient of a pending message delivery. The key is derived from the KEK of
// the version prefixed to the ciphertext, so these rows survive rotation as
// long as that version stays in ENCRYPTION_KEY_RETIRED.

const FIELD_PREFIX = /^([A-Za-z0-9_.-]{1,32}):([0-9a-f]+)$/;

function fieldKey(entry: KeyEntry): Buffer {
  return crypto.createHmac('sha256', deriveKek(entry)).update('neurowealth-field-encryption').digest();
}

export function encryptField(plaintext: string): { encryptedData: string; iv: string; tag: string } {
  const { current } = loadKeyring();
  const iv = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv(ALGORITHM, fieldKey(current), iv);
  const encrypted = cipher.update(plaintext, 'utf8', 'hex') + cipher.final('hex');
  return {
    encryptedData: `${current.version}:${encrypted}`,
    iv: iv.toString('hex'),
    tag: cipher.getAuthTag().toString('hex'),
  };
}

export function decryptField(encryptedData: string, iv: string, tag: string): string {
  const match = FIELD_PREFIX.exec(encryptedData);
  if (!match) return decryptLegacy(encryptedData, iv, tag);
  return openWithDek(fieldKey(findKey(match[1])), match[2], iv, tag);
}
