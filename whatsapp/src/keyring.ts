/**
 * Key-encryption-key (KEK) ring for custodial wallet secrets (#944).
 *
 * Each wallet secret is encrypted with its own random data-encryption key
 * (DEK); the DEK is wrapped by a KEK taken from this ring. Rotating the KEK
 * therefore only re-wraps small DEKs (see keyRotation.ts) and never exposes
 * or re-encrypts the Stellar secrets themselves.
 *
 *   ENCRYPTION_KEY          current KEK material (>= 32 chars, `openssl rand -hex 32`)
 *   ENCRYPTION_KEY_VERSION  label stored next to every wrapped DEK (default "1")
 *   ENCRYPTION_KEY_RETIRED  `version=key,version=key` of previous KEKs that may
 *                           still wrap not-yet-rotated rows
 *
 * This module is the only place that reads KEK material from the environment,
 * so swapping it for a KMS/HSM client (wrap/unwrap by key id) later touches
 * nothing else.
 */

export const MIN_KEY_LENGTH = 32;
export const DEFAULT_KEY_VERSION = '1';
export const VERSION_PATTERN = /^[A-Za-z0-9_.-]{1,32}$/;

export interface KeyEntry {
  version: string;
  material: string;
}

export interface Keyring {
  current: KeyEntry;
  retired: KeyEntry[];
}

/** Parses `version=key,version=key`. Throws on any malformed or duplicate entry. */
export function parseRetiredKeys(raw: string | undefined): KeyEntry[] {
  if (raw === undefined || raw.trim() === '') return [];

  const entries: KeyEntry[] = [];
  for (const part of raw.split(',')) {
    const trimmed = part.trim();
    const eq = trimmed.indexOf('=');
    if (eq <= 0) {
      throw new Error('ENCRYPTION_KEY_RETIRED entries must look like version=key');
    }
    const version = trimmed.slice(0, eq);
    const material = trimmed.slice(eq + 1);
    if (!VERSION_PATTERN.test(version)) {
      throw new Error(`ENCRYPTION_KEY_RETIRED has an invalid version label "${version}"`);
    }
    if (material.length < MIN_KEY_LENGTH) {
      throw new Error(`ENCRYPTION_KEY_RETIRED key for version "${version}" must be at least ${MIN_KEY_LENGTH} characters`);
    }
    if (entries.some((e) => e.version === version)) {
      throw new Error(`ENCRYPTION_KEY_RETIRED lists version "${version}" more than once`);
    }
    entries.push({ version, material });
  }
  return entries;
}

export function loadKeyring(env: NodeJS.ProcessEnv = process.env): Keyring {
  const material = env.ENCRYPTION_KEY;
  if (!material) {
    throw new Error('ENCRYPTION_KEY must be set; refusing to use an unsafe production default');
  }
  if (material.length < MIN_KEY_LENGTH) {
    throw new Error(`ENCRYPTION_KEY must be at least ${MIN_KEY_LENGTH} characters`);
  }

  const version = env.ENCRYPTION_KEY_VERSION?.trim() || DEFAULT_KEY_VERSION;
  if (!VERSION_PATTERN.test(version)) {
    throw new Error('ENCRYPTION_KEY_VERSION may only contain letters, digits, ".", "_" and "-" (max 32 chars)');
  }

  const retired = parseRetiredKeys(env.ENCRYPTION_KEY_RETIRED);
  if (retired.some((e) => e.version === version)) {
    throw new Error(`ENCRYPTION_KEY_RETIRED must not reuse the current version "${version}"`);
  }

  return { current: { version, material }, retired };
}
