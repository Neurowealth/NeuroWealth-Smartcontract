process.env.ENCRYPTION_KEY = "test-encryption-key-at-least-32-chars!!";
process.env.PHONE_HASH_SALT = "test-phone-salt-0123456789";
process.env.SOROBAN_RPC_URL = "https://soroban-testnet.stellar.org";
process.env.VAULT_CONTRACT_ID = "CTEST_VAULT";

const walletRows = new Map<string, Record<string, unknown>>();
const dbPath = require.resolve("./db");
require.cache[dbPath] = {
  id: dbPath,
  filename: dbPath,
  loaded: true,
  exports: {
    query: async (sql: string, params: unknown[] = []) => {
      if (/^\s*INSERT INTO whatsapp_wallets/i.test(sql)) {
        const [phoneHash, publicKey, encryptedData, iv, tag, createdAt, wrappedDek, dekIv, dekTag, keyVersion] = params as [
          string, string, string, string, string, number, string, string, string, string,
        ];
        if (!walletRows.has(phoneHash)) {
          walletRows.set(phoneHash, {
            phone_hash: phoneHash,
            public_key: publicKey,
            encrypted_data: encryptedData,
            iv,
            tag,
            created_at: new Date(createdAt),
            wrapped_dek: wrappedDek,
            dek_iv: dekIv,
            dek_tag: dekTag,
            key_version: keyVersion,
          });
        }
        return { rows: [] };
      }

      if (/^\s*SELECT \* FROM whatsapp_wallets/i.test(sql)) {
        const row = walletRows.get(params[0] as string);
        return { rows: row ? [{ ...row }] : [] };
      }

      throw new Error(`Unexpected webhook test query: ${sql}`);
    },
  },
  parent: null,
  children: [],
  paths: [],
} as any;

const deliveryPath = require.resolve("./messageDeliveryService");
require.cache[deliveryPath] = {
  id: deliveryPath,
  filename: deliveryPath,
  loaded: true,
  exports: {
    applyDeliveryStatus: async () => ({ applied: true, created: true }),
    retryFailedDelivery: async () => ({ retried: false, escalated: false }),
  },
  parent: null,
  children: [],
  paths: [],
} as any;

import { describe, it, beforeEach, afterEach } from "node:test";
import assert from "node:assert";
import { VaultClient } from "@neurowealth/vault-client";
import { hashPhoneNumber } from "./cryptoUtils";
import { getWallet } from "./walletService";
import { getSession, updateState, UserState } from "./stateManager";
import { InMemoryStateStore, setStateStore } from "./stateStore";
import { smsClient } from "./smsService";

setStateStore(new InMemoryStateStore());

const TEST_TX_HASH = "b".repeat(64);
VaultClient.prototype.get_balance = async () => 1_000_0000000n;
VaultClient.prototype.get_user_strategy = async () => "balanced";
VaultClient.prototype.get_user_apy = async () => 850n;
VaultClient.prototype.set_user_strategy = async () => ({ hash: TEST_TX_HASH });
VaultClient.prototype.deposit = async () => ({ hash: TEST_TX_HASH });
VaultClient.prototype.withdraw = async () => ({ hash: TEST_TX_HASH });
VaultClient.prototype.withdraw_all = async () => ({ hash: TEST_TX_HASH, result: 0n });

const { handleWhatsAppWebhook } =
  require("./webhook") as typeof import("./webhook");

// Webhook state machine (#691). Each test uses its own phone number, because the
// session, wallet and OTP stores are module-level maps with no reset hook - and
// the rate limiter counts per phone, so sharing one would couple the tests.
//
// OTP codes are delivered via SMS only, never echoed in the chat reply
// (#942), so tests monkey-patch smsClient.send to capture the code instead of
// hitting the real Twilio API.

let counter = 0;
const freshFrom = () =>
  "whatsapp:+1555" + String((counter += 1)).padStart(7, "0");

let lastSmsCode: string | null = null;
const realSmsSend = smsClient.send;

beforeEach(() => {
  lastSmsCode = null;
  smsClient.send = async (_to: string, body: string) => {
    const match = body.match(/[0-9]{6}/);
    lastSmsCode = match ? match[0] : null;
    return "test-message-sid";
  };
});

afterEach(() => {
  smsClient.send = realSmsSend;
});

interface Captured {
  status?: number;
  type?: string;
  body: string;
}

async function send(from: string, body: string): Promise<Captured> {
  const captured: Captured = { body: "" };
  const res: any = {
    status(code: number) {
      captured.status = code;
      return res;
    },
    type(value: string) {
      captured.type = value;
      return res;
    },
    send(value: string) {
      captured.body = value;
      return res;
    },
  };
  await handleWhatsAppWebhook({ body: { From: from, Body: body } } as any, res);
  return captured;
}

async function verifiedPhone(): Promise<string> {
  const from = freshFrom();
  await send(from, "hi");
  assert.ok(lastSmsCode, "the greeting must issue an OTP via SMS");
  const confirmation = await send(from, lastSmsCode!);
  assert.match(confirmation.body, /verified/i);
  return from;
}

describe("webhook rejected input (#691)", () => {
  it("rejects a payload with no sender", async () => {
    const captured = await send("", "hi");
    assert.strictEqual(captured.status, 400);
    assert.match(captured.body, /Missing sender phone number/);
  });
});

describe("webhook verification flow (#691)", () => {
  it("answers a greeting with an OTP sent via SMS and moves the session to AWAITING_OTP", async () => {
    const from = freshFrom();
    const captured = await send(from, "hi");

    assert.strictEqual(captured.type, "text/xml");
    assert.doesNotMatch(
      captured.body,
      /[0-9]{6}/,
      "the OTP code must never appear in the chat reply",
    );
    assert.match(
      captured.body,
      /sent a 6-digit verification code to your phone via SMS/,
    );
    assert.ok(lastSmsCode, "the OTP must be delivered via SMS");
    assert.strictEqual(
      (await getSession(hashPhoneNumber(from))).state,
      UserState.AWAITING_OTP,
    );
  });

  it("asks an unverified user to greet first when they send something else", async () => {
    const captured = await send(freshFrom(), "what is my balance");
    assert.match(captured.body, /Send "hi" to begin/);
  });

  it("creates a wallet once the OTP is accepted", async () => {
    const from = freshFrom();
    await send(from, "hi");
    const issued = lastSmsCode!;
    const captured = await send(from, issued);

    assert.match(captured.body, /Public Key: G/);
    assert.strictEqual(
      (await getSession(hashPhoneNumber(from))).state,
      UserState.VERIFIED,
    );
    assert.ok(
      getWallet(hashPhoneNumber(from)),
      "a wallet should exist after verification",
    );
  });

  it("rejects a wrong OTP and says how many attempts remain", async () => {
    const from = freshFrom();
    await send(from, "hi");
    const issued = lastSmsCode!;
    const wrong = issued === "000000" ? "111111" : "000000";
    const captured = await send(from, wrong);

    assert.match(captured.body, /attempts remaining/);
    assert.strictEqual(
      (await getSession(hashPhoneNumber(from))).state,
      UserState.AWAITING_OTP,
    );
  });

  it("prompts again when the awaiting-OTP user sends something that is not a code", async () => {
    const from = freshFrom();
    await send(from, "hi");
    const captured = await send(from, "what now");

    assert.match(captured.body, /new verification code.*SMS/i);
  });
});

describe("webhook verified flows (#691)", () => {
  it("answers a greeting with the menu", async () => {
    const captured = await send(await verifiedPhone(), "hi");
    assert.match(captured.body, /How can I assist your portfolio today/);
  });

  it("answers a balance question with the portfolio", async () => {
    const captured = await send(await verifiedPhone(), "balance");
    assert.match(captured.body, /NeuroWealth Portfolio/);
    assert.match(captured.body, /Current APY/);
  });

  it("handles a deposit and echoes the amount", async () => {
    const captured = await send(await verifiedPhone(), "deposit 100 usdc");
    assert.match(captured.body, /Deposited 100 USDC/);
    assert.match(captured.body, /Transaction Hash/);
  });

  it("handles a partial withdrawal", async () => {
    const captured = await send(await verifiedPhone(), "withdraw 50");
    assert.match(captured.body, /Withdrew 50 USDC/);
  });

  it("handles a full withdrawal", async () => {
    const captured = await send(await verifiedPhone(), "withdraw all");
    assert.match(captured.body, /all funds/);
  });

  it("confirms a strategy switch in upper case", async () => {
    const captured = await send(await verifiedPhone(), "switch to growth");
    assert.match(captured.body, /Strategy updated to GROWTH/);
  });

  it("offers examples when it does not understand", async () => {
    const captured = await send(await verifiedPhone(), "what is the weather");
    assert.match(captured.body, /didn't quite catch that/);
  });

  it("resets a verified session whose wallet is missing", async () => {
    const from = freshFrom();
    const phoneHash = hashPhoneNumber(from);
    await updateState(phoneHash, UserState.VERIFIED);

    const captured = await send(from, "balance");
    assert.match(captured.body, /Session expired/);
    assert.strictEqual(
      (await getSession(phoneHash)).state,
      UserState.UNVERIFIED,
    );
  });
});

describe("webhook rate limiting (#691)", () => {
  it("answers the eleventh message in a minute with a rate-limit notice", async () => {
    const from = freshFrom();
    for (let i = 0; i < 10; i += 1) {
      await send(from, "hi");
    }
    const captured = await send(from, "hi");

    assert.match(captured.body, /Rate limit exceeded/);
  });
});
