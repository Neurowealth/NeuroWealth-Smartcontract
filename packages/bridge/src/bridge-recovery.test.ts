/**
 * Restart and reconciliation tests for durable bridge state (#848, #850).
 *
 * Each test drives a transfer to a workflow stage with one manager, throws
 * that manager away (process termination), and brings a new manager up on the
 * same store - exactly what a restart looks like to the durable records.
 */

import axios from "axios";
import { ethers } from "ethers";
import { BridgeManager } from "./bridge-manager";
import { InMemoryBridgeStore, isTerminalStatus } from "./bridge-store";
import { TransferReconciler } from "./bridge-recovery";
import { redactTransfer, sanitizeErrorMessage } from "./redaction";
import { BridgeConfig, StoredBridgeTransfer } from "./types";

jest.mock("axios");
jest.mock("pino", () => () => ({
  info: jest.fn(),
  warn: jest.fn(),
  error: jest.fn(),
  debug: jest.fn(),
}));
jest.mock("@stellar/stellar-sdk", () => ({
  SorobanRpc: { Server: jest.fn(() => ({ getTransaction: jest.fn() })) },
}));

const mockedAxios = axios as jest.Mocked<typeof axios>;

const config: BridgeConfig = {
  stellarRpcUrl: "https://soroban.test",
  stellarNetworkPassphrase: "Test SDF Network ; September 2015",
  stellarVaultContractId: "CVAULT",
  stellarUsdcTokenId: "CUSDC",
  ethereumRpcUrl: "http://127.0.0.1:8545",
  ethereumChainId: 1,
  ethereumVaultContractAddress: "0xvault",
  ethereumUsdcTokenAddress: "0xusdc",
  axelarApiUrl: "https://axelar.test",
  axelarChainName: "ethereum",
  axelarGasServiceAddress: "0xgas",
  bridgeFeePercentage: 0.5,
  minBridgeAmount: 1_000_000n,
  maxBridgeAmount: 10_000_000_000n,
  confirmationDepths: { stellar: 10, ethereum: 12 },
};

const STELLAR_USER = "GUSER";
const ETH_USER = "0x000000000000000000000000000000000000dEaD";

/**
 * Boots a "new process" against the same durable store: exactly the state a
 * restarted bridge has, an empty in-memory mirror and no live timers.
 */
async function restart(store: InMemoryBridgeStore): Promise<BridgeManager> {
  const manager = new BridgeManager(config, store);
  await manager.loadDurableState();
  return manager;
}

async function newDeposit(manager: BridgeManager): Promise<string> {
  const transfer = await manager.initiateEthereumDeposit(
    ETH_USER,
    100_000_000n,
    STELLAR_USER,
  );
  return transfer.id;
}

function mockEthereumReceipt(found: boolean): void {
  const provider = BridgeManager.prototype as unknown as {
    ethersProvider: ethers.Provider;
  };
  void provider;
  jest
    .spyOn(ethers.JsonRpcProvider.prototype, "getTransactionReceipt")
    .mockResolvedValue(
      found ? ({ hash: "0xsrc", status: 1 } as never) : (null as never),
    );
}

async function reconcilerFor(
  manager: BridgeManager,
  store: InMemoryBridgeStore,
): Promise<TransferReconciler> {
  return new TransferReconciler(manager, store);
}

beforeEach(() => {
  jest.clearAllMocks();
  mockedAxios.post.mockReset();
  mockedAxios.get.mockReset();
  jest.restoreAllMocks();
});

describe("durable workflow state (#848)", () => {
  it("persists a new transfer with its initial stage and attempt count", async () => {
    const store = new InMemoryBridgeStore();
    const manager = new BridgeManager(config, store);

    const id = await newDeposit(manager);
    const stored = await store.get(id);

    expect(stored).toMatchObject({
      stage: "observed",
      attemptCount: 0,
      status: "pending",
      retriesRemaining: 3,
    });
  });

  it("advances the durable stage when the bridge message is submitted", async () => {
    const store = new InMemoryBridgeStore();
    const manager = new BridgeManager(config, store);
    const id = await newDeposit(manager);

    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await manager.executeAxelarTransfer(id, "0xsrc");

    expect(await store.get(id)).toMatchObject({
      stage: "submitted",
      status: "confirming",
      bridgeTxHash: "0xbridge",
      sourceChainTxHash: "0xsrc",
      attemptCount: 1,
    });
  });

  it("marks the stage confirmed once the destination confirms", async () => {
    const store = new InMemoryBridgeStore();
    const manager = new BridgeManager(config, store);
    const id = await newDeposit(manager);

    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await manager.executeAxelarTransfer(id, "0xsrc");
    mockedAxios.get.mockResolvedValueOnce({
      data: { status: "executed", destinationTxHash: "0xdest", confirmationDepth: 12 },
    });
    await manager.pollTransferStatus(id);

    expect(await store.get(id)).toMatchObject({
      stage: "confirmed",
      status: "confirmed",
      destinationTxHash: "0xdest",
    });
  });

  it("persists a sanitised failure with a backoff deadline", async () => {
    const store = new InMemoryBridgeStore();
    const manager = new BridgeManager(config, store);
    const id = await newDeposit(manager);

    mockedAxios.post.mockRejectedValueOnce(
      new Error("bridge rejected: apiKey=SUPERSECRET1234567890"),
    );
    await expect(manager.executeAxelarTransfer(id, "0xsrc")).rejects.toThrow();

    const stored = await store.get(id);
    expect(stored?.status).toBe("failed");
    expect(stored?.attemptCount).toBe(1);
    expect(stored?.lastError).toContain("[redacted]");
    expect(stored?.lastError).not.toContain("SUPERSECRET");
    expect(stored?.nextAttemptAt).toBeGreaterThan(Date.now());
  });

  it("keeps secrets and payload data out of the durable record", async () => {
    const store = new InMemoryBridgeStore();
    const manager = new BridgeManager(config, store);
    const id = await newDeposit(manager);

    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await manager.executeAxelarTransfer(id, "0xsrc");

    const serialised = JSON.stringify(await store.get(id), (_, v) =>
      typeof v === "bigint" ? v.toString() : v,
    );
    // The GMP payload is never persisted, and neither are credentials.
    expect(serialised).not.toContain("payload");
    expect(serialised).not.toContain("SUPERSECRET");
  });

  it("does not alias the caller's object, so later mutations are not silently durable", async () => {
    const store = new InMemoryBridgeStore();
    const manager = new BridgeManager(config, store);
    const id = await newDeposit(manager);

    // Mutating the in-memory record without going through the manager must not
    // change what a restart would read back.
    const live = manager.getTransfer(id) as StoredBridgeTransfer;
    live.status = "confirmed";
    live.stage = "confirmed";

    const stored = await store.get(id);
    expect(stored?.status).toBe("pending");
    expect(stored?.stage).toBe("observed");
  });

  it("leaves the record untouched when a storage write fails mid-update", async () => {
    const store = new InMemoryBridgeStore();
    const manager = new BridgeManager(config, store);
    const id = await newDeposit(manager);

    const before = await store.get(id);
    const failingStore = Object.create(store) as InMemoryBridgeStore;
    failingStore.update = async () => {
      throw new Error("storage unavailable");
    };

    const failingManager = new BridgeManager(config, failingStore);
    await failingManager.loadDurableState();
    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });

    await expect(
      failingManager.executeAxelarTransfer(id, "0xsrc"),
    ).rejects.toThrow("storage unavailable");

    // The durable record is exactly as it was: a partial write is recoverable.
    expect(await store.get(id)).toEqual(before);
  });

  it("resolves a retry by idempotency key after a restart", async () => {
    const store = new InMemoryBridgeStore();
    const manager = new BridgeManager(config, store);

    const first = await manager.initiateEthereumDeposit(
      ETH_USER,
      100_000_000n,
      STELLAR_USER,
      "order-1",
    );

    const restarted = await restart(store);
    const retry = await restarted.initiateEthereumDeposit(
      ETH_USER,
      100_000_000n,
      STELLAR_USER,
      "order-1",
    );

    expect(retry.id).toBe(first.id);
    expect((await store.getAll()).length).toBe(1);
  });
});

describe("startup reconciliation (#850)", () => {
  it("resumes a transfer interrupted before submission, without a second submission", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    const id = await newDeposit(first);

    // Source-chain transaction landed, process died before the bridge call.
    await store.update(id, { sourceChainTxHash: "0xsrc" });
    expect(mockedAxios.post).not.toHaveBeenCalled();

    const restarted = await restart(store);
    mockEthereumReceipt(true);
    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });

    const summary = await (await reconcilerFor(restarted, store)).reconcileOnStartup();

    expect(summary.byAction.resumed).toBe(1);
    expect(mockedAxios.post).toHaveBeenCalledTimes(1);
    expect((await store.get(id))?.stage).toBe("submitted");

    // A second reconciliation pass must not submit again.
    mockEthereumReceipt(true);
    mockedAxios.get.mockRejectedValue(new Error("request timeout"));
    const second = await (await reconcilerFor(restarted, store)).reconcileOnStartup();
    expect(second.byAction.resumed).toBe(0);
    expect(second.byAction.awaiting_external).toBe(1);
    expect(mockedAxios.post).toHaveBeenCalledTimes(1);
  });

  it("polls but never resubmits a transfer interrupted in flight", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    const id = await newDeposit(first);

    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await first.executeAxelarTransfer(id, "0xsrc");

    const restarted = await restart(store);
    mockEthereumReceipt(true);
    const postsBefore = mockedAxios.post.mock.calls.length;
    mockedAxios.get.mockResolvedValueOnce({
      data: { status: "executed", destinationTxHash: "0xdest", confirmationDepth: 12 },
    });

    const summary = await (await reconcilerFor(restarted, store)).reconcileOnStartup();

    expect(summary.byAction.confirmed).toBe(1);
    expect(mockedAxios.post).toHaveBeenCalledTimes(postsBefore);
    expect((await store.get(id))?.status).toBe("confirmed");
  });

  it("confirms a record whose destination already executed before the crash", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    const id = await newDeposit(first);

    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await first.executeAxelarTransfer(id, "0xsrc");

    const restarted = await restart(store);
    mockEthereumReceipt(true);
    const postsBefore = mockedAxios.post.mock.calls.length;
    mockedAxios.get.mockResolvedValueOnce({
      data: { status: "executed", destinationTxHash: "0xdest", confirmationDepth: 12 },
    });

    await (await reconcilerFor(restarted, store)).reconcileOnStartup();

    expect(mockedAxios.post).toHaveBeenCalledTimes(postsBefore);
    const stored = await store.get(id);
    expect(stored?.status).toBe("confirmed");
    expect(stored?.destinationTxHash).toBe("0xdest");
  });

  it("waits for the confirmation depth instead of confirming early", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    const id = await newDeposit(first);

    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await first.executeAxelarTransfer(id, "0xsrc");

    const restarted = await restart(store);
    mockEthereumReceipt(true);
    mockedAxios.get.mockResolvedValueOnce({
      data: { status: "executed", destinationTxHash: "0xdest", confirmationDepth: 3 },
    });

    const summary = await (await reconcilerFor(restarted, store)).reconcileOnStartup();

    expect(summary.byAction.confirmed).toBe(0);
    expect(summary.byAction.awaiting_external).toBe(1);
    expect((await store.get(id))?.status).toBe("confirming");
  });

  it("never retries a terminal transfer and keeps it queryable", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    const id = await newDeposit(first);

    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await first.executeAxelarTransfer(id, "0xsrc");
    mockedAxios.get.mockResolvedValueOnce({
      data: { status: "executed", destinationTxHash: "0xdest", confirmationDepth: 12 },
    });
    await first.pollTransferStatus(id);

    const restarted = await restart(store);
    const postsBefore = mockedAxios.post.mock.calls.length;
    mockedAxios.get.mockClear();
    const summary = await (await reconcilerFor(restarted, store)).reconcileOnStartup();

    expect(summary.scanned).toBe(0);
    expect(summary.byAction.skipped_terminal).toBe(0);
    expect(mockedAxios.post).toHaveBeenCalledTimes(postsBefore);
    expect(mockedAxios.get).not.toHaveBeenCalled();

    // Still queryable by the user.
    expect((await store.getByUser(STELLAR_USER)).map((t) => t.id)).toEqual([id]);
    expect(isTerminalStatus((await store.get(id))!.status)).toBe(true);
  });

  it("keeps an unknown external state retryable behind a bounded backoff", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    const id = await newDeposit(first);

    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await first.executeAxelarTransfer(id, "0xsrc");

    const restarted = await restart(store);
    mockEthereumReceipt(true);
    mockedAxios.get.mockRejectedValue(new Error("request timeout"));

    const summary = await (await reconcilerFor(restarted, store)).reconcileOnStartup();
    const stored = await store.get(id);

    expect(summary.byAction.awaiting_external).toBe(1);
    expect(summary.byErrorClass.timeout).toBe(1);
    expect(stored?.status).toBe("confirming");
    expect(stored?.nextAttemptAt).toBeGreaterThan(Date.now());

    // A restart inside the backoff window leaves the record alone.
    mockedAxios.get.mockClear();
    mockedAxios.post.mockClear();
    const again = await (await reconcilerFor(restarted, store)).reconcileOnStartup();
    expect(again.byAction.backoff).toBe(1);
    expect(mockedAxios.get).not.toHaveBeenCalled();
    expect(mockedAxios.post).not.toHaveBeenCalled();
  });

  it("fails a record whose source-chain transaction never landed", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    const id = await newDeposit(first);
    await store.update(id, { sourceChainTxHash: "0xmissing" });

    const restarted = await restart(store);
    mockEthereumReceipt(false);

    const summary = await (await reconcilerFor(restarted, store)).reconcileOnStartup();

    expect(summary.byAction.failed).toBe(1);
    expect(summary.byErrorClass.not_found).toBe(1);
    expect((await store.get(id))?.status).toBe("failed");
    expect(mockedAxios.post).not.toHaveBeenCalled();
  });

  it("waits when the source-chain transaction is not known yet", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    await newDeposit(first);

    const restarted = await restart(store);
    mockEthereumReceipt(true);

    const summary = await (await reconcilerFor(restarted, store)).reconcileOnStartup();

    expect(summary.byAction.awaiting_external).toBe(1);
    expect(summary.results[0].reason).toBe("awaiting_source_hash");
    expect(mockedAxios.post).not.toHaveBeenCalled();
  });

  it("reports counts by terminal state and error class for the whole pass", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);

    const confirmedId = await newDeposit(first);
    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge" },
    });
    await first.executeAxelarTransfer(confirmedId, "0xsrc");
    mockedAxios.get.mockResolvedValueOnce({
      data: { status: "executed", destinationTxHash: "0xdest", confirmationDepth: 12 },
    });
    await first.pollTransferStatus(confirmedId);

    const inflightId = await newDeposit(first);
    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge2" },
    });
    await first.executeAxelarTransfer(inflightId, "0xsrc2");

    const pendingId = await newDeposit(first);
    await store.update(pendingId, { sourceChainTxHash: "0xsrc3" });

    const restarted = await restart(store);
    mockEthereumReceipt(true);
    mockedAxios.post.mockResolvedValueOnce({
      data: { transactionHash: "0xbridge3" },
    });
    mockedAxios.get.mockRejectedValue(new Error("connection reset by peer"));

    const summary = await (await reconcilerFor(restarted, store)).reconcileOnStartup();

    // The terminal record is not scanned at all.
    expect(summary.scanned).toBe(2);
    expect(summary.byStatus.confirming).toBe(2);
    expect(summary.byErrorClass.network).toBe(1);
    expect(summary.byAction.resumed).toBe(1);
    expect(summary.byAction.awaiting_external).toBe(1);
    expect(summary.results.map((r) => r.transferId)).toEqual(
      expect.arrayContaining([inflightId, pendingId]),
    );
    expect(summary.results.every((r) => r.transferId !== confirmedId)).toBe(true);
  });

  it("honours the batch limit and defers the rest", async () => {
    const store = new InMemoryBridgeStore();
    const first = new BridgeManager(config, store);
    await newDeposit(first);
    await newDeposit(first);
    await newDeposit(first);

    const restarted = await restart(store);
    const summary = await new TransferReconciler(restarted, store, {
      maxRecords: 2,
    }).reconcileOnStartup();

    expect(summary.scanned).toBe(2);
  });
});

describe("redaction helpers (#848)", () => {
  it("redacts credential-looking substrings", () => {
    const message = sanitizeErrorMessage(
      "POST failed apiKey=abcd1234efgh5678 authorization: Bearer tok_abcdefgh12345678",
    );
    expect(message).not.toContain("abcd1234efgh5678");
    expect(message).not.toContain("tok_abcdefgh12345678");
    expect(message).toContain("[redacted]");
  });

  it("truncates long messages and collapses newlines", () => {
    const message = sanitizeErrorMessage("line one\nline two " + "x".repeat(400), 32);
    expect(message.length).toBeLessThanOrEqual(32);
    expect(message).not.toContain("\n");
  });

  it("logs identifiers and amounts but never free-form errors", () => {
    const redacted = redactTransfer({
      id: "t1",
      status: "failed",
      stage: "observed",
      direction: "deposit",
      sourceChain: "ethereum",
      destinationChain: "stellar",
      user: "GUSER",
      amount: 100n,
      bridgeFee: 1n,
      netAmount: 99n,
      createdAt: 1,
      updatedAt: 2,
      retriesRemaining: 3,
      attemptCount: 2,
      lastError: "apiKey=SUPERSECRET",
    });

    expect(redacted).toMatchObject({ id: "t1", hasError: true, attemptCount: 2 });
    expect(JSON.stringify(redacted)).not.toContain("SUPERSECRET");
  });
});
