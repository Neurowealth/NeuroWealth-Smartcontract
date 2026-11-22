import pino from 'pino';
import { createHash } from 'node:crypto';
import type { Writable } from 'node:stream';

const SENSITIVE_FIELD = /authorization|secret|token|password|api[_-]?key|private[_-]?key|seed|credential/i;
const ADDRESS_FIELD = /^(?:stellarAddress|userAddress|walletAddress|publicKey|vaultAddress|address|user|userId)$/i;
const CONTENT_FIELD = /^(?:userMessage|externalMessage|prompt|content|body|text|message)$/i;

function cleanString(value: string): string {
  const configuredSecrets = Object.entries(process.env)
    .filter(([key, secret]) => secret && /secret|token|password|api.?key|private|seed|credential|database_url|dsn/i.test(key))
    .flatMap(([, secret]) => secret!.split(',').map((entry) => entry.trim()).filter(Boolean))
    .sort((left, right) => right.length - left.length);
  let cleaned = value;
  for (const secret of configuredSecrets) cleaned = cleaned.split(secret).join('[REDACTED]');
  return cleaned
    .replace(/(https?:\/\/[^:/\s]+:)[^@/\s]+(@)/gi, '$1[REDACTED]$2')
    .replace(/(authorization\s*[:=]\s*)(?:bearer\s+)?[^\s,;]+/gi, '$1[REDACTED]')
    .replace(/(bearer\s+)[a-z0-9._~+/-]+=*/gi, '$1[REDACTED]')
    .replace(/((?:api[_-]?key|access[_-]?token|refresh[_-]?token|password|secret|private[_-]?key|seed)\s*[:=]\s*["']?)[^"'\s,;}]+/gi, '$1[REDACTED]')
    .replace(/\b[GC][A-Z2-7]{55}\b/g, (address) => `${address.slice(0, 6)}...${address.slice(-4)}`);
}

function redactValue(value: unknown, fieldName = '', seen = new WeakSet<object>()): unknown {
  if (SENSITIVE_FIELD.test(fieldName)) return '[REDACTED]';
  if (typeof value === 'string') {
    if (ADDRESS_FIELD.test(fieldName) && value.length > 14) return `${value.slice(0, 6)}...${value.slice(-4)}`;
    if (CONTENT_FIELD.test(fieldName)) {
      return `sha256:${createHash('sha256').update(value).digest('hex').slice(0, 16)}`;
    }
    return cleanString(value);
  }
  if (value instanceof Error) {
    if (seen.has(value)) return '[Circular]';
    seen.add(value);
    return {
      ...Object.fromEntries(Object.entries(value)
        .filter(([key]) => !['name', 'message', 'stack', 'cause'].includes(key))
        .map(([key, entry]) => [key, redactValue(entry, key, seen)])),
      type: value.name,
      message: cleanString(value.message),
      ...(value.stack ? { stack: cleanString(value.stack) } : {}),
      ...('code' in value ? { code: redactValue(value.code, 'code') } : {}),
      ...(value.cause !== undefined ? { cause: redactValue(value.cause, 'cause') } : {}),
    };
  }
  if (Array.isArray(value)) {
    if (seen.has(value)) return '[Circular]';
    seen.add(value);
    return value.map((item) => redactValue(item, '', seen));
  }
  if (value && typeof value === 'object') {
    if (seen.has(value)) return '[Circular]';
    seen.add(value);
    return Object.fromEntries(Object.entries(value).map(([key, entry]) => [key, redactValue(entry, key, seen)]));
  }
  return value;
}

export function createLogger(stream?: Writable) {
  const options: pino.LoggerOptions = {
    level: process.env.LOG_LEVEL || 'info',
    transport: stream || process.env.NODE_ENV === 'production'
      ? undefined
      : { target: 'pino/file', options: { destination: 1 } },
    hooks: {
      logMethod(args, method) {
        const sanitizedArgs = args.map((argument) => redactValue(argument)) as [unknown, string?, ...unknown[]];
        method.apply(this, sanitizedArgs);
      },
    },
  };
  return stream ? pino(options, stream) : pino(options);
}

const logger = createLogger();

export default logger;
