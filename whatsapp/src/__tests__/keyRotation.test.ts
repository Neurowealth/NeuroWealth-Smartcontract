/**
 * Envelope encryption and KEK rotation for custodial secrets (#944).
 */
import crypto from 'node:crypto';
import { test, describe, beforeEach } from 'node:test';
import assert from 'node:assert/strict';

const KEY_A = 'test-kek-version-a-0123456789-abcdefghij';
const KEY_B = 'test-kek-version-b-9876543210-jihgfedcba';
const KEY_C = 'test-kek-version-c-1357924680-klmnopqrst';

function useKeys(env: { key: string; version?: string; retired?: string }): void {
  process.env.ENCRYPTION_KEY = env.key;
  if (env.version === undefined) delete process.env.ENCRYPTION_KEY_VERSION;
  else process.env.ENCRYPTION_KEY_VERSION = env.version;
  if (env.retired === undefined) delete process.env.ENCRYPTION_KEY_RETIRED;
  else process.env.ENCRYPTION_KEY_RETIRED = env.retired;
}

process.env.PHONE_HASH_SALT = 'test-phone-salt-0123456789';
useKeys({ key: KEY_A });

import {
  decryptField,
  decryptSecretKey,
  encryptField,
  encryptSecretKey,
  isCurrentKeyVersion,
  rewrapSecret,
  EncryptedSecret,
} from '../cryptoUtils';
import { loadKeyring, parseRetiredKeys } from '../keyring';
import { rotateWalletKeys, QueryFn } from '../keyRotation';

const SECRET = 'test-wallet-secret-not-a-real-stellar-seed';

/** Pre-#944 encryption: AES key = scrypt(ENCRYPTION_KEY, 'salt'), no envelope. */
function legacyEncrypt(secret: string, material: string): EncryptedSecret {
  const iv = crypto.randomBytes(12);
  const cipher = crypto.createCipheriv('aes-256-gcm', crypto.scryptSync(material, 'salt', 32), iv);
  const encryptedData = cipher.update(secret, 'utf8', 'hex') + cipher.final('hex');
  return { encryptedData, iv: iv.toString('hex'), tag: cipher.getAuthTag().toString('hex') };
}

beforeEach(() => useKeys({ key: KEY_A }));

describe('envelope encryption', () => {
  test('round-trips and records the KEK version', () => {
    const enc = encryptSecretKey(SECRET);
    assert.equal(enc.keyVersion, '1');
    assert.equal(decryptSecretKey(enc.encryptedData, enc.iv, enc.tag, enc), SECRET);
  });

  test('uses a fresh data key per secret', () => {
    const a = encryptSecretKey(SECRET);
    const b = encryptSecretKey(SECRET);
    assert.notEqual(a.wrappedDek, b.wrappedDek);
    assert.notEqual(a.encryptedData, b.encryptedData);
  });

  test('the secret is not recoverable from the stored columns with the KEK alone', () => {
    const enc = encryptSecretKey(SECRET);
    // The old (pre-envelope) derivation must not open an envelope record.
    assert.throws(() => decryptSecretKey(enc.encryptedData, enc.iv, enc.tag));
  });

  test('a relabelled key version fails authentication', () => {
    useKeys({ key: KEY_B, version: '2', retired: `1=${KEY_A}` });
    const enc = encryptSecretKey(SECRET);
    assert.throws(() => decryptSecretKey(enc.encryptedData, enc.iv, enc.tag, { ...enc, keyVersion: '1' }));
  });

  test('tampering with the wrapped data key is detected', () => {
    const enc = encryptSecretKey(SECRET);
    const flipped = (enc.wrappedDek[0] === '0' ? '1' : '0') + enc.wrappedDek.slice(1);
    assert.throws(() => decryptSecretKey(enc.encryptedData, enc.iv, enc.tag, { ...enc, wrappedDek: flipped }));
  });
});

describe('key ring configuration', () => {
  test('parses retired keys and rejects malformed entries', () => {
    assert.deepEqual(parseRetiredKeys(undefined), []);
    assert.equal(parseRetiredKeys(`1=${KEY_A}, 2=${KEY_B}`).length, 2);
    assert.throws(() => parseRetiredKeys('no-equals-sign'), /version=key/);
    assert.throws(() => parseRetiredKeys('1=short'), /at least 32/);
    assert.throws(() => parseRetiredKeys(`1=${KEY_A},1=${KEY_B}`), /more than once/);
    assert.throws(() => parseRetiredKeys(`bad version=${KEY_A}`), /invalid version/);
  });

  test('refuses to reuse the current version as a retired one', () => {
    useKeys({ key: KEY_B, version: '2', retired: `2=${KEY_A}` });
    assert.throws(() => loadKeyring(), /must not reuse the current version/);
  });

  test('refuses a missing or short ENCRYPTION_KEY', () => {
    useKeys({ key: 'short' });
    assert.throws(() => loadKeyring(), /at least 32/);
  });
});

describe('rotation', () => {
  test('after rotating the KEK, old records stay readable via the retired key', () => {
    const old = encryptSecretKey(SECRET); // wrapped by KEY_A / "1"
    useKeys({ key: KEY_B, version: '2', retired: `1=${KEY_A}` });

    assert.equal(isCurrentKeyVersion(old), false);
    assert.equal(decryptSecretKey(old.encryptedData, old.iv, old.tag, old), SECRET);
  });

  test('without the retired key an old record is unreadable, with a pointed error', () => {
    const old = encryptSecretKey(SECRET);
    useKeys({ key: KEY_B, version: '2' });
    assert.throws(() => decryptSecretKey(old.encryptedData, old.iv, old.tag, old), /ENCRYPTION_KEY_RETIRED/);
  });

  test('rewrapSecret only re-wraps the data key; the ciphertext is untouched', () => {
    const old = encryptSecretKey(SECRET);
    useKeys({ key: KEY_B, version: '2', retired: `1=${KEY_A}` });

    const next = rewrapSecret(old);
    assert.equal(next.keyVersion, '2');
    assert.equal(next.encryptedData, old.encryptedData);
    assert.equal(next.iv, old.iv);
    assert.equal(next.tag, old.tag);
    assert.notEqual(next.wrappedDek, old.wrappedDek);
    assert.equal(isCurrentKeyVersion(next), true);

    // Once re-wrapped, the retired key is no longer needed.
    useKeys({ key: KEY_B, version: '2' });
    assert.equal(decryptSecretKey(next.encryptedData, next.iv, next.tag, next), SECRET);
  });

  test('rewrapSecret upgrades a legacy record into the envelope format', () => {
    const legacy = legacyEncrypt(SECRET, KEY_A);
    assert.equal(decryptSecretKey(legacy.encryptedData, legacy.iv, legacy.tag), SECRET);

    useKeys({ key: KEY_B, version: '2', retired: `1=${KEY_A}` });
    const upgraded = rewrapSecret(legacy);
    assert.equal(upgraded.keyVersion, '2');
    assert.ok(upgraded.wrappedDek);

    useKeys({ key: KEY_B, version: '2' });
    assert.equal(decryptSecretKey(upgraded.encryptedData, upgraded.iv, upgraded.tag, upgraded), SECRET);
  });
});

// ── rotateWalletKeys against an in-memory whatsapp_wallets table ────────────

interface Row {
  phone_hash: string;
  encrypted_data: string;
  iv: string;
  tag: string;
  wrapped_dek: string | null;
  dek_iv: string | null;
  dek_tag: string | null;
  key_version: string | null;
}

function toRow(phoneHash: string, enc: EncryptedSecret): Row {
  return {
    phone_hash: phoneHash,
    encrypted_data: enc.encryptedData,
    iv: enc.iv,
    tag: enc.tag,
    wrapped_dek: enc.wrappedDek ?? null,
    dek_iv: enc.dekIv ?? null,
    dek_tag: enc.dekTag ?? null,
    key_version: enc.keyVersion ?? null,
  };
}

function fromRow(row: Row): EncryptedSecret {
  return {
    encryptedData: row.encrypted_data,
    iv: row.iv,
    tag: row.tag,
    wrappedDek: row.wrapped_dek ?? undefined,
    dekIv: row.dek_iv ?? undefined,
    dekTag: row.dek_tag ?? undefined,
    keyVersion: row.key_version ?? undefined,
  };
}

function fakeTable(rows: Row[]): { query: QueryFn; writes: () => number } {
  let writes = 0;
  const query: QueryFn = async (sql, params = []) => {
    if (/^\s*SELECT/i.test(sql)) {
      const [version, cursor, limit] = params as [string, string, number];
      const picked = rows
        .filter((r) => (r.key_version === null || r.key_version !== version) && r.phone_hash > cursor)
        .sort((a, b) => (a.phone_hash < b.phone_hash ? -1 : 1))
        .slice(0, limit);
      return { rows: picked.map((r) => ({ ...r })) };
    }
    writes += 1;
    const [phoneHash, data, iv, tag, dek, dekIv, dekTag, version, oldData, oldVersion] = params as string[];
    const row = rows.find((r) => r.phone_hash === phoneHash);
    if (!row || row.encrypted_data !== oldData || row.key_version !== (oldVersion ?? null)) return { rows: [], rowCount: 0 };
    Object.assign(row, { encrypted_data: data, iv, tag, wrapped_dek: dek, dek_iv: dekIv, dek_tag: dekTag, key_version: version });
    return { rows: [], rowCount: 1 };
  };
  return { query, writes: () => writes };
}

describe('rotateWalletKeys', () => {
  function seed(): Row[] {
    const rows: Row[] = [];
    // two envelope rows under KEK "1" and one legacy row
    for (const id of ['p1', 'p2']) rows.push(toRow(id, encryptSecretKey(`${SECRET}-${id}`)));
    rows.push(toRow('p3', legacyEncrypt(`${SECRET}-p3`, KEY_A)));
    return rows;
  }

  test('rotates every row to the current KEK, in batches, and is idempotent', async () => {
    const rows = seed();
    useKeys({ key: KEY_B, version: '2', retired: `1=${KEY_A}` });
    const table = fakeTable(rows);

    const first = await rotateWalletKeys({ queryFn: table.query, batchSize: 2 });
    assert.deepEqual(first, { targetVersion: '2', rotated: 3, skipped: 0, failed: 0 });

    // Retire the old key entirely: every secret must still decrypt.
    useKeys({ key: KEY_B, version: '2' });
    for (const row of rows) {
      const enc = fromRow(row);
      assert.equal(row.key_version, '2');
      assert.equal(decryptSecretKey(enc.encryptedData, enc.iv, enc.tag, enc), `${SECRET}-${row.phone_hash}`);
    }

    const second = await rotateWalletKeys({ queryFn: table.query });
    assert.deepEqual(second, { targetVersion: '2', rotated: 0, skipped: 0, failed: 0 });
  });

  test('dry run verifies every row but writes nothing', async () => {
    const rows = seed();
    useKeys({ key: KEY_B, version: '2', retired: `1=${KEY_A}` });
    const table = fakeTable(rows);

    const result = await rotateWalletKeys({ queryFn: table.query, dryRun: true });
    assert.equal(result.rotated, 3);
    assert.equal(table.writes(), 0);
    assert.ok(rows.every((r) => r.key_version !== '2'));
  });

  test('rows whose old key is missing are reported as failed and left untouched', async () => {
    const rows = seed();
    const before = JSON.stringify(rows);
    useKeys({ key: KEY_C, version: '3' }); // KEY_A is not retained
    const table = fakeTable(rows);

    const result = await rotateWalletKeys({ queryFn: table.query });
    assert.equal(result.failed, 3);
    assert.equal(result.rotated, 0);
    assert.equal(JSON.stringify(rows), before);
  });

  test('a row changed concurrently is skipped, not overwritten', async () => {
    const rows = seed();
    useKeys({ key: KEY_B, version: '2', retired: `1=${KEY_A}` });
    const table = fakeTable(rows);
    const racing: QueryFn = async (sql, params) => {
      const res = await table.query(sql, params);
      // Another worker rotates p2 between our SELECT and UPDATE.
      if (/^\s*SELECT/i.test(sql)) {
        const p2 = rows.find((r) => r.phone_hash === 'p2');
        if (p2 && p2.key_version === '1') Object.assign(p2, toRow('p2', rewrapSecret(fromRow(p2))), {});
      }
      return res;
    };

    const result = await rotateWalletKeys({ queryFn: racing });
    assert.equal(result.skipped >= 1, true);
    assert.equal(result.failed, 0);
  });
});

describe('field encryption (message delivery recipients)', () => {
  test('round-trips and is versioned', () => {
    const enc = encryptField('whatsapp:+15550001111');
    assert.match(enc.encryptedData, /^1:[0-9a-f]+$/);
    assert.equal(decryptField(enc.encryptedData, enc.iv, enc.tag), 'whatsapp:+15550001111');
  });

  test('stays readable after rotation while the old key is retired, and not without it', () => {
    const enc = encryptField('whatsapp:+15550001111');
    useKeys({ key: KEY_B, version: '2', retired: `1=${KEY_A}` });
    assert.equal(decryptField(enc.encryptedData, enc.iv, enc.tag), 'whatsapp:+15550001111');
    useKeys({ key: KEY_B, version: '2' });
    assert.throws(() => decryptField(enc.encryptedData, enc.iv, enc.tag), /ENCRYPTION_KEY_RETIRED/);
  });

  test('reads values written before versioning (legacy format)', () => {
    const legacy = legacyEncrypt('whatsapp:+15550002222', KEY_A);
    assert.equal(decryptField(legacy.encryptedData, legacy.iv, legacy.tag), 'whatsapp:+15550002222');
  });
});
