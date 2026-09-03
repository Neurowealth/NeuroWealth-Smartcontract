#!/usr/bin/env node
/**
 * generate-client.js
 *
 * Reads contract-spec.json and emits a fully-typed TypeScript client package
 * at packages/vault-client/src/generated/vault.ts.
 *
 * Usage:
 *   node scripts/generate-client.js [--spec <path>] [--out <path>]
 *
 * Defaults:
 *   --spec  contract-spec.json                            (repo root)
 *   --out   packages/vault-client/src/generated/vault.ts
 */

'use strict';

const fs   = require('fs');
const path = require('path');

// ---------------------------------------------------------------------------
// CLI argument parsing
// ---------------------------------------------------------------------------
const args = process.argv.slice(2);

function argValue(flag) {
  const idx = args.indexOf(flag);
  return idx !== -1 ? args[idx + 1] : undefined;
}

const repoRoot = path.resolve(__dirname, '..');
const specPath = path.resolve(argValue('--spec') ?? path.join(repoRoot, 'contract-spec.json'));
const outPath  = path.resolve(argValue('--out')  ?? path.join(repoRoot, 'packages/vault-client/src/generated/vault.ts'));

// ---------------------------------------------------------------------------
// Load spec
// ---------------------------------------------------------------------------
if (!fs.existsSync(specPath)) {
  console.error(`ERROR: spec file not found: ${specPath}`);
  process.exit(1);
}

/** @type {import('../contract-spec.json')} */
const spec = JSON.parse(fs.readFileSync(specPath, 'utf8'));

// ---------------------------------------------------------------------------
// Type-mapping helpers
// ---------------------------------------------------------------------------

/**
 * Map a Soroban/Rust type string to the appropriate TypeScript type.
 * @param {string} sorobanType
 * @returns {string}
 */
function toTsType(sorobanType) {
  if (!sorobanType) return 'void';

  // Strip outer whitespace
  const t = sorobanType.trim();

  // Exact matches first
  const exact = {
    'Env':           'never',          // not exposed in TS client
    'i128':          'bigint',
    'u32':           'number',
    'u64':           'bigint',
    'bool':          'boolean',
    'String':        'string',
    'Symbol':        'string',
    'Address':       'string',
    'BytesN<32>':    'Uint8Array',
    'UserInfo':      'UserInfo',
    'RateLimitConfig': 'RateLimitConfig',
    'RateLimitState':  'RateLimitState',
  };
  if (exact[t]) return exact[t];

  // Option<T>
  const optMatch = t.match(/^Option<(.+)>$/);
  if (optMatch) return `${toTsType(optMatch[1])} | null`;

  // Result<T, E>  — surface only the Ok type; errors throw
  const resultMatch = t.match(/^Result<([^,]+),/);
  if (resultMatch) return toTsType(resultMatch[1].trim());

  // Tuple<A, B, ...>  (rare, kept for completeness)
  const tupleMatch = t.match(/^Tuple<(.+)>$/);
  if (tupleMatch) {
    const parts = tupleMatch[1].split(',').map(s => toTsType(s.trim()));
    return `[${parts.join(', ')}]`;
  }

  // Fallback: unknown Soroban type → unknown
  return 'unknown';
}

/**
 * Convert a Soroban type to the `xdr.ScVal` factory call needed by stellar-sdk.
 * Returns a TS expression string for use inside generated invoke helpers.
 * @param {string} sorobanType
 * @param {string} varName  - name of the TS variable holding the value
 * @returns {string}
 */
function toScVal(sorobanType, varName) {
  if (!sorobanType) return `nativeToScVal(${varName})`;
  const t = sorobanType.trim();

  if (t === 'Address')      return `new StellarSdk.Address(${varName}).toScVal()`;
  if (t === 'i128')         return `nativeToScVal(${varName}, { type: 'i128' })`;
  if (t === 'u32')          return `nativeToScVal(${varName}, { type: 'u32' })`;
  if (t === 'u64')          return `nativeToScVal(${varName}, { type: 'u64' })`;
  if (t === 'bool')         return `nativeToScVal(${varName}, { type: 'bool' })`;
  if (t === 'Symbol')       return `nativeToScVal(${varName}, { type: 'symbol' })`;
  if (t === 'BytesN<32>')   return `nativeToScVal(${varName}, { type: 'bytes' })`;
  if (t === 'String')       return `nativeToScVal(${varName}, { type: 'string' })`;

  const optMatch = t.match(/^Option<(.+)>$/);
  if (optMatch) {
    return `${varName} == null ? StellarSdk.xdr.ScVal.scvVoid() : ${toScVal(optMatch[1], varName)}`;
  }

  return `nativeToScVal(${varName})`;
}

// ---------------------------------------------------------------------------
// Doc-comment builder
// ---------------------------------------------------------------------------

/**
 * Build a JSDoc block from a function definition.
 * @param {object} fn  - function object from spec
 * @returns {string}
 */
function buildJsDoc(fn) {
  const lines = [];
  if (fn.description) lines.push(` * ${fn.description}`);
  if (fn.deprecated)  lines.push(' * @deprecated');

  const userParams = (fn.parameters ?? []).filter(p => p.type !== 'Env');
  for (const p of userParams) {
    const desc = p.description ? `  ${p.description}` : '';
    lines.push(` * @param ${p.name}${desc}`);
  }

  if (fn.returns) {
    const retDesc = typeof fn.returns === 'object' ? fn.returns.description ?? '' : '';
    if (retDesc) lines.push(` * @returns ${retDesc}`);
  }

  if (fn.constraints?.length) {
    lines.push(' * @remarks');
    lines.push(' * **Constraints:**');
    for (const c of fn.constraints) lines.push(` * - ${c}`);
  }

  if (fn.events?.length) {
    lines.push(` * @fires ${fn.events.join(', ')}`);
  }

  if (!lines.length) return '';
  return `/**\n${lines.join('\n')}\n */\n`;
}

// ---------------------------------------------------------------------------
// Parameter helpers
// ---------------------------------------------------------------------------

/** Return only user-visible parameters (skip 'Env'). */
function userParams(fn) {
  return (fn.parameters ?? []).filter(p => p.type !== 'Env');
}

/** Build a TS parameter list string: `name: TsType, ...` */
function paramList(fn) {
  return userParams(fn)
    .map(p => `${p.name}: ${toTsType(p.type)}`)
    .join(', ');
}

/** Build the return TS type for an invoke helper. */
function returnType(fn) {
  if (!fn.returns || fn.returns === null) return 'void';
  const raw = typeof fn.returns === 'object' ? fn.returns.type : fn.returns;
  if (!raw || raw === 'null') return 'void';
  return toTsType(raw);
}

// ---------------------------------------------------------------------------
// Code generators
// ---------------------------------------------------------------------------

function generateFileHeader() {
  return `// ============================================================
// AUTO-GENERATED — DO NOT EDIT
//
// Generated by scripts/generate-client.js
// Source: contract-spec.json  v${spec.version}
// Contract: ${spec.contract}
// Network:  ${spec.network}
//
// Regenerate with:
//   node scripts/generate-client.js
// ============================================================

/* eslint-disable */
// @ts-nocheck  (cast-heavy generated file — type-checked at the call site)

import * as StellarSdk from '@stellar/stellar-sdk';

const { nativeToScVal, scValToNative, xdr } = StellarSdk;

`;
}

function generateConstants() {
  const lines = ['// ----------------------------------------------------------------', '// Constants', '// ----------------------------------------------------------------', ''];

  for (const [name, def] of Object.entries(spec.constants ?? {})) {
    const tsType = toTsType(def.type ?? 'i128');
    const rawVal = String(def.value).replace(/_/g, '');
    const val = tsType === 'bigint' ? `${rawVal}n` : rawVal;
    lines.push(`/** ${def.description} */`);
    lines.push(`export const ${name}: ${tsType} = ${val};`);
    lines.push('');
  }

  return lines.join('\n');
}

function generateRateLimitTypes() {
  const configs = [
    ['RateLimitConfig', spec.types?.RateLimitConfig],
    ['RateLimitState', spec.types?.RateLimitState],
  ];
  const lines = [];
  for (const [name, def] of configs) {
    if (!def) continue;
    lines.push(`/** ${def.description} */`);
    lines.push(`export interface ${name} {`);
    for (const field of def.fields ?? []) {
      lines.push(`  /** ${field.description ?? ''} */`);
      lines.push(`  ${field.name}: ${toTsType(field.type)};`);
    }
    lines.push('}', '');
  }
  return lines.join('\n');
}

function generateUserInfoType() {
  const userInfoSpec = spec.types?.UserInfo;
  if (!userInfoSpec) return '';

  const lines = [
    '// ----------------------------------------------------------------',
    '// Types',
    '// ----------------------------------------------------------------',
    '',
    `/** ${userInfoSpec.description} */`,
    'export interface UserInfo {',
  ];

  for (const f of userInfoSpec.fields ?? []) {
    if (f.description) lines.push(`  /** ${f.description} */`);
    lines.push(`  ${f.name}: ${toTsType(f.type)};`);
  }

  lines.push('}', '');
  return lines.join('\n');
}

function generateEventInterfaces() {
  const lines = [
    '// ----------------------------------------------------------------',
    '// Event payload types',
    '// ----------------------------------------------------------------',
    '',
  ];

  for (const ev of spec.events ?? []) {
    if (ev.description) lines.push(`/** ${ev.description} */`);
    if (!ev.fields?.length) {
      lines.push(`export type ${ev.name} = Record<string, never>;`);
    } else {
      lines.push(`export interface ${ev.name} {`);
      for (const f of ev.fields) {
        if (f.description) lines.push(`  /** ${f.description} */`);
        lines.push(`  ${f.name}: ${toTsType(f.type)};`);
      }
      lines.push('}');
    }
    lines.push('');
  }

  return lines.join('\n');
}

function generateErrorEnum() {
  const lines = [
    '// ----------------------------------------------------------------',
    '// Error codes',
    '// ----------------------------------------------------------------',
    '',
    '/** Numeric error codes returned by the NeuroWealth Vault contract. */',
    'export const VaultErrorCode = {',
  ];

  for (const [name, def] of Object.entries(spec.errors ?? {})) {
    const safeName = name
      .replace('VaultError::', '')
      .replace(/[^a-zA-Z0-9_]/g, '_');
    lines.push(`  /** ${def.description} */`);
    lines.push(`  ${safeName}: ${def.code},`);
  }

  lines.push('} as const;', '');
  lines.push('export type VaultErrorCode = typeof VaultErrorCode[keyof typeof VaultErrorCode];', '');
  lines.push('');
  lines.push('/**');
  lines.push(' * Typed error wrapping contract-level VaultError responses.');
  lines.push(' *');
  lines.push(' * Provides a `.code` property with the numeric error code and');
  lines.push(' * preserves the original message.');
  lines.push(' */');
  lines.push('export class VaultError extends Error {');
  lines.push('  readonly code: VaultErrorCode;');
  lines.push('');
  lines.push('  constructor(code: VaultErrorCode, message: string) {');
  lines.push('    super(message);');
  lines.push('    this.name = "VaultError";');
  lines.push('    this.code = code;');
  lines.push('  }');
  lines.push('');
  lines.push('  /**');
  lines.push('   * Build a VaultError from a raw contract error object.');
  lines.push('   * Handles both `{code, message}` shapes and raw string errors.');
  lines.push('   */');
  lines.push('  static fromContractError(error: unknown): VaultError {');
  lines.push('    if (error && typeof error === "object" && "code" in error) {');
  lines.push('      const obj = error as { code: number; message?: string };');
  lines.push('      return new VaultError(obj.code as VaultErrorCode, obj.message ?? String(obj.code));');
  lines.push('    }');
  lines.push('    if (typeof error === "string") {');
  lines.push('      return new VaultError(0 as VaultErrorCode, error);');
  lines.push('    }');
  lines.push('    return new VaultError(0 as VaultErrorCode, String(error));');
  lines.push('  }');
  lines.push('}');
  lines.push('');

  return lines.join('\n');
}

/**
 * Generate the main client class with one method per contract function.
 */
function generateClientClass() {
  const lines = [
    '// ----------------------------------------------------------------',
    '// VaultClient',
    '// ----------------------------------------------------------------',
    '',
    '/** Options passed to every invoke helper. */',
    'export interface InvokeOptions {',
    '  /** Source account public key (G...). */',
    '  sourcePublicKey: string;',
    '  /** Soroban RPC server URL (e.g. https://soroban-testnet.stellar.org). */',
    '  rpcUrl?: string;',
    '  /** Network passphrase. Defaults to Testnet. */',
    '  networkPassphrase?: string;',
    '}',
    '',
    '/** Result of a simulation or submitted transaction. */',
    'export interface TxResult<T> {',
    '  /** Parsed return value (undefined for void functions). */',
    '  result?: T;',
    '  /** Raw transaction hash once submitted. */',
    '  hash?: string;',
    '  /** Raw simulation response (useful for debugging). */',
    '  simulation?: StellarSdk.SorobanRpc.Api.SimulateTransactionResponse;',
    '}',
    '',
    '/**',
    ` * Typed client for the ${spec.contract} contract.`,
    ' *',
    ' * @example',
    ' * ```typescript',
    ' * import { VaultClient } from \'@neurowealth/vault-client\';',
    ' *',
    ' * const client = new VaultClient({',
    ' *   contractId: \'C...\',',
    ' *   rpcUrl: \'https://soroban-testnet.stellar.org\',',
    ' *   networkPassphrase: StellarSdk.Networks.TESTNET,',
    ' * });',
    ' *',
    ' * // Read-only query',
    ' * const balance = await client.get_balance(\'G...\');',
    ' * console.log(\'USDC balance:\', balance);',
    ' *',
    ' * // State-changing call (requires signing)',
    ' * const { hash } = await client.deposit(userKeypair, \'G...\', 10_000_000n);',
    ' * console.log(\'deposit tx:\', hash);',
    ' * ```',
    ' */',
    'export class VaultClient {',
    '  private readonly contractId: string;',
    '  private readonly server: StellarSdk.SorobanRpc.Server;',
    '  private readonly networkPassphrase: string;',
    '',
    '  constructor(options: {',
    '    /** Deployed contract address (C...). */',
    '    contractId: string;',
    '    /** Soroban RPC URL. */',
    '    rpcUrl?: string;',
    '    /** Network passphrase. Defaults to Testnet. */',
    '    networkPassphrase?: string;',
    '  }) {',
    '    this.contractId = options.contractId;',
    '    const rpcUrl = options.rpcUrl ?? \'https://soroban-testnet.stellar.org\';',
    '    this.networkPassphrase = options.networkPassphrase ?? StellarSdk.Networks.TESTNET;',
    '    this.server = new StellarSdk.SorobanRpc.Server(rpcUrl, { allowHttp: rpcUrl.startsWith(\'http://\') });',
    '  }',
    '',
    '  // ------------------------------------------------------------------',
    '  // Internal helpers',
    '  // ------------------------------------------------------------------',
    '',
    '  private throwVaultError(method: string, error: unknown): never {',
    '    if (error instanceof VaultError) {',
    '      throw error;',
    '    }',
    '',
    '    const vaultError = VaultError.fromContractError(error);',
    '    if (vaultError.message === `Vault contract error ${vaultError.code}`) {',
    '      vaultError.message = `Vault contract error for ${method}`;',
    '    }',
    '    throw vaultError;',
    '  }',
    '',
    '  /**',
    '   * Build a contract invocation operation.',
    '   * @internal',
    '   */',
    '  private buildOperation(',
    '    method: string,',
    '    args: StellarSdk.xdr.ScVal[],',
    '  ): StellarSdk.xdr.Operation {',
    '    return StellarSdk.Operation.invokeContractFunction({',
    '      contract: this.contractId,',
    '      function: method,',
    '      args,',
    '    });',
    '  }',
    '',
    '  /**',
    '   * Simulate a read-only call and return the decoded result.',
    '   * @internal',
    '   */',
    '  private async simulate<T>(',
    '    method: string,',
    '    args: StellarSdk.xdr.ScVal[],',
    '    sourcePublicKey: string,',
    '  ): Promise<T> {',
    '    const account  = await this.server.getAccount(sourcePublicKey);',
    '    const tx = new StellarSdk.TransactionBuilder(account, {',
    '      fee: \'100\',',
    '      networkPassphrase: this.networkPassphrase,',
    '    })',
    '      .addOperation(this.buildOperation(method, args))',
    '      .setTimeout(30)',
    '      .build();',
    '',
    '    const sim = await this.server.simulateTransaction(tx);',
    '    if (StellarSdk.SorobanRpc.Api.isSimulationError(sim)) {',
    '      this.throwVaultError(method, sim.error ?? sim);',
    '    }',
    '    const resultEntry = (sim as StellarSdk.SorobanRpc.Api.SimulateTransactionSuccessResponse).result;',
    '    if (!resultEntry) return undefined as unknown as T;',
    '    return scValToNative(resultEntry.retval) as T;',
    '  }',
    '',
    '  /**',
    '   * Prepare, sign, and submit a state-changing transaction.',
    '   * @internal',
    '   */',
    '  private async invoke<T>(',
    '    method: string,',
    '    args: StellarSdk.xdr.ScVal[],',
    '    signer: StellarSdk.Keypair,',
    '  ): Promise<TxResult<T>> {',
    '    const account = await this.server.getAccount(signer.publicKey());',
    '    const tx = new StellarSdk.TransactionBuilder(account, {',
    '      fee: \'100\',',
    '      networkPassphrase: this.networkPassphrase,',
    '    })',
    '      .addOperation(this.buildOperation(method, args))',
    '      .setTimeout(30)',
    '      .build();',
    '',
    '    const sim = await this.server.simulateTransaction(tx);',
    '    if (StellarSdk.SorobanRpc.Api.isSimulationError(sim)) {',
    '      this.throwVaultError(method, sim.error ?? sim);',
    '    }',
    '',
    '    const prepared = StellarSdk.SorobanRpc.assembleTransaction(tx, sim).build();',
    '    prepared.sign(signer);',
    '',
    '    const sendResp = await this.server.sendTransaction(prepared);',
    '    if (sendResp.status === \'ERROR\') {',
    '      this.throwVaultError(method, sendResp.errorResult ?? sendResp);',
    '    }',
    '',
    '    // Poll until confirmed',
    '    let getResp = await this.server.getTransaction(sendResp.hash);',
    '    const deadline = Date.now() + 30_000;',
    '    while (getResp.status === StellarSdk.SorobanRpc.Api.GetTransactionStatus.NOT_FOUND',
    '        && Date.now() < deadline) {',
    '      await new Promise(r => setTimeout(r, 1000));',
    '      getResp = await this.server.getTransaction(sendResp.hash);',
    '    }',
    '    if (getResp.status === StellarSdk.SorobanRpc.Api.GetTransactionStatus.FAILED) {',
    '      this.throwVaultError(method, getResp);',
    '    }',
    '',
    '    const retval = (getResp as StellarSdk.SorobanRpc.Api.GetSuccessfulTransactionResponse).returnValue;',
    '    return {',
    '      hash: sendResp.hash,',
    '      result: retval ? (scValToNative(retval) as T) : undefined,',
    '      simulation: sim,',
    '    };',
    '  }',
    '',
    '  // ------------------------------------------------------------------',
    '  // Contract methods',
    '  // ------------------------------------------------------------------',
  ];

  for (const fn of spec.functions ?? []) {
    const uParams = userParams(fn);
    const retTs   = returnType(fn);
    const isQuery = !fn.state_changing;

    // -- JSDoc
    lines.push('');
    const doc = buildJsDoc(fn);
    if (doc) lines.push('  ' + doc.trimEnd().replace(/\n/g, '\n  '));

    if (isQuery) {
      // Query: simulate, require sourcePublicKey
      const hasUserParams = uParams.length > 0;
      const paramStr = [
        ...(hasUserParams ? uParams.map(p => `${p.name}: ${toTsType(p.type)}`) : []),
        'sourcePublicKey: string',
      ].join(', ');

      lines.push(`  async ${fn.name}(${paramStr}): Promise<${retTs}> {`);
      lines.push(`    const args: StellarSdk.xdr.ScVal[] = [${uParams.map(p => toScVal(p.type, p.name)).join(', ')}];`);
      lines.push(`    return this.simulate<${retTs}>('${fn.name}', args, sourcePublicKey);`);
      lines.push('  }');
    } else {
      // Mutating: invoke, require Keypair signer
      // For functions where 'user' or 'owner' is an Address param we still accept it
      // so callers can specify a different payer from the signer.
      const paramStr = [
        'signer: StellarSdk.Keypair',
        ...uParams.map(p => `${p.name}: ${toTsType(p.type)}`),
      ].join(', ');

      lines.push(`  async ${fn.name}(${paramStr}): Promise<TxResult<${retTs === 'void' ? 'void' : retTs}>> {`);
      lines.push(`    const args: StellarSdk.xdr.ScVal[] = [${uParams.map(p => toScVal(p.type, p.name)).join(', ')}];`);
      lines.push(`    return this.invoke<${retTs === 'void' ? 'void' : retTs}>('${fn.name}', args, signer);`);
      lines.push('  }');
    }
  }

  lines.push('}', '');
  return lines.join('\n');
}

// ---------------------------------------------------------------------------
// Assemble output
// ---------------------------------------------------------------------------

function generate() {
  return [
    generateFileHeader(),
    generateConstants(),
    generateUserInfoType(),
    generateRateLimitTypes(),
    generateEventInterfaces(),
    generateErrorEnum(),
    generateClientClass(),
  ].join('\n');
}

// ---------------------------------------------------------------------------
// Write output
// ---------------------------------------------------------------------------

const output = generate();

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, output, 'utf8');

console.log(`✅ Generated ${path.relative(repoRoot, outPath)}`);
console.log(`   Functions : ${(spec.functions ?? []).length}`);
console.log(`   Events    : ${(spec.events ?? []).length}`);
console.log(`   Errors    : ${Object.keys(spec.errors ?? {}).length}`);
console.log(`   Constants : ${Object.keys(spec.constants ?? {}).length}`);
