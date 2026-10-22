import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  INPUT_LIMITS,
  SAFE_OUTBOUND_REPLY,
  boundOutbound,
  boundText,
  decodeFormComponent,
  decodeUtf8,
} from '../inputBounds';

test('boundText accepts the limit and rejects one byte past it', () => {
  const exact = 'a'.repeat(INPUT_LIMITS.commandText);
  const accepted = boundText(exact, INPUT_LIMITS.commandText);
  assert.equal(accepted.ok, true);
  if (accepted.ok) assert.equal(accepted.value, exact);

  const rejected = boundText(`${exact}a`, INPUT_LIMITS.commandText);
  assert.deepEqual(rejected, { ok: false, reason: 'oversize' });
});

test('boundText counts UTF-8 bytes and rejects a lone surrogate', () => {
  const exact = '🙂'.repeat(INPUT_LIMITS.strategy / 4);
  assert.equal(Buffer.byteLength(exact), INPUT_LIMITS.strategy);
  assert.equal(boundText(exact, INPUT_LIMITS.strategy).ok, true);
  assert.deepEqual(boundText(`${exact}a`, INPUT_LIMITS.strategy), { ok: false, reason: 'oversize' });
  assert.deepEqual(boundText('ok\u0000', INPUT_LIMITS.commandText), { ok: false, reason: 'malformed' });
  assert.deepEqual(boundText('bad\uD800', INPUT_LIMITS.commandText), { ok: false, reason: 'malformed' });
  assert.deepEqual(boundText(12, INPUT_LIMITS.commandText), { ok: false, reason: 'type' });
});

test('decodeUtf8 rejects invalid and overlong encodings without returning them', () => {
  const exact = Buffer.from('a'.repeat(INPUT_LIMITS.address));
  const accepted = decodeUtf8(exact, INPUT_LIMITS.address);
  assert.equal(accepted.ok, true);

  const over = Buffer.concat([exact, Buffer.from('b')]);
  assert.deepEqual(decodeUtf8(over, INPUT_LIMITS.address), { ok: false, reason: 'oversize' });
  assert.deepEqual(decodeUtf8(Buffer.from([0xff]), INPUT_LIMITS.address), { ok: false, reason: 'malformed' });
  assert.deepEqual(decodeUtf8(Buffer.from([0xc0, 0x80]), INPUT_LIMITS.address), { ok: false, reason: 'malformed' });
});

test('decodeFormComponent rejects broken percent-encoding', () => {
  assert.equal(decodeFormComponent('hello+world', 32).ok, true);
  const decoded = decodeFormComponent('hello+world', 32);
  if (decoded.ok) assert.equal(decoded.value, 'hello world');
  assert.deepEqual(decodeFormComponent('%ZZ', 32), { ok: false, reason: 'malformed' });
  assert.deepEqual(decodeFormComponent('%C0%80', 32), { ok: false, reason: 'malformed' });
  assert.deepEqual(decodeFormComponent('%', 32), { ok: false, reason: 'malformed' });
});

test('boundOutbound keeps a template at the limit and replaces one byte over', () => {
  const exact = 'x'.repeat(INPUT_LIMITS.outboundTemplate);
  assert.equal(boundOutbound(exact), exact);
  const over = `${exact}y`;
  const safe = boundOutbound(over);
  assert.equal(safe, SAFE_OUTBOUND_REPLY);
  assert.equal(safe.includes(over), false);
  assert.equal(boundOutbound('bad\uD800'), SAFE_OUTBOUND_REPLY);
});
