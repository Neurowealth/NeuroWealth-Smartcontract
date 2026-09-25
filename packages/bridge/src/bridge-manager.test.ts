/**
 * Unit tests for BridgeManager state transitions (#775).
 *
 * Network edges are mocked: the Soroban RPC client (never called by the
 * manager's state logic) and axios (Axelar API). ethers runs for real so
 * signature verification is exercised end to end.
 */

import axios from "axios";
import { ethers } from "ethers";
import {
  ALLOWED_TRANSITIONS,
  BridgeManager,
  canTransition,
} from "./bridge-manager";
import { BridgeConfig, BridgeStatus, BridgeTransfer } from "./types";

jest.mock("axios");
jest.mock("pino", () => () => ({
  info: jest.fn(),
  warn: jest.fn(),
  error: jest.fn(),
  debug: jest.fn(),
}));
jest.mock("@stellar/stellar-sdk", () => ({
  SorobanRpc: { Server: jest.fn() },
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
  confirmationDepths: {
    stellar: 10,
    ethereum: 12,
  },
};

const ALL_STATUSES: BridgeStatus[] = [
  "pending",
  "confirming",
  "confirmed",
  "failed",
  "cancelled",
];

const STELLAR_USER = "GUSER";
const ETH_USER = "0x000000000000000000000000000000000000dEaD";

function newManager(): BridgeManager {
  return new BridgeManager(config);
}

async function newDeposit(
  manager: BridgeManager,
  amount = 100_000_000n,
): Promise<BridgeTransfer> {
  return manager.initiateEthereumDeposit(ETH_USER, amount, STELLAR_USER);
}

function axelarAccepts(txHash = "0xbridge"): void {
  mockedAxios.post.mockResolvedValueOnce({ data: { transactionHash: txHash } });
}

/**
 * Reports a bridge status. `confirmationDepth` defaults to a depth that
 * satisfies the configured requirement (#851) so the suite exercises the
 * confirmation path; pass an explicit depth to test a shallower window.
 */
function axelarReports(
  status: string,
  destinationTxHash?: string,
  confirmationDepth = 12,
): void {
  mockedAxios.get.mockResolvedValueOnce({
    data: { status, destinationTxHash, confirmationDepth },
  });
}

/** Drives a fresh transfer into the requested status via the public API. */
async function transferIn(
  manager: BridgeManager,
  status: BridgeStatus,
): Promise<string> {
  const { id } = await newDeposit(manager);
  if (status === "pending") return id;
  if (status === "cancelled") {
    await manager.cancelTransfer(id);
    return id;
  }
  if (status === "failed") {
    mockedAxios.post.mockRejectedValueOnce(new Error("axelar down"));
    await expect(manager.executeAxelarTransfer(id, "0xsrc")).rejects.toThrow();
    return id;
  }
  axelarAccepts();
  await manager.executeAxelarTransfer(id, "0xsrc");
  if (status === "confirming") return id;
  axelarReports("executed", "0xdest");
  await manager.pollTransferStatus(id);
  return id;
}

beforeEach(() => {
  jest.clearAllMocks();
  mockedAxios.post.mockReset();
  mockedAxios.get.mockReset();
});

describe("transition table", () => {
  it("treats confirmed and cancelled as terminal", () => {
    expect(ALLOWED_TRANSITIONS.confirmed).toEqual([]);
    expect(ALLOWED_TRANSITIONS.cancelled).toEqual([]);
  });

  it("allows exactly the documented edges", () => {
    const allowed = new Set([
      "pending->confirming",
      "pending->failed",
      "pending->cancelled",
      "confirming->confirmed",
      "confirming->failed",
      "failed->pending",
    ]);
    for (const from of ALL_STATUSES) {
      for (const to of ALL_STATUSES) {
        expect(canTransition(from, to)).toBe(allowed.has(`${from}->${to}`));
      }
    }
  });
});

describe("initiation", () => {
  it("creates a pending Ethereum -> Stellar deposit with fee applied", async () => {
    const manager = newManager();
    const transfer = await newDeposit(manager, 100_000_000n);

    expect(transfer).toMatchObject({
      status: "pending",
      direction: "deposit",
      sourceChain: "ethereum",
      destinationChain: "stellar",
      user: STELLAR_USER,
      amount: 100_000_000n,
      bridgeFee: 500_000n, // 0.5%
      netAmount: 99_500_000n,
    });
    expect(manager.getTransfer(transfer.id)).toMatchObject({
      retriesRemaining: 3,
    });
  });

  it("creates a pending Stellar -> Ethereum withdrawal with an ETA", async () => {
    const manager = newManager();
    const transfer = await manager.initiateStellarWithdraw(
      STELLAR_USER,
      2_000_000n,
      ETH_USER,
    );

    expect(transfer).toMatchObject({
      status: "pending",
      direction: "withdraw",
      sourceChain: "stellar",
      destinationChain: "ethereum",
      user: STELLAR_USER,
      bridgeFee: 10_000n,
      netAmount: 1_990_000n,
    });
    expect(transfer.estimatedArrivalTime).toBeGreaterThan(transfer.createdAt);
  });

  it.each([
    ["below minimum", config.minBridgeAmount - 1n, /below minimum/],
    ["above maximum", config.maxBridgeAmount + 1n, /above maximum/],
  ])("rejects amounts %s without storing a transfer", async (_, amount, msg) => {
    const manager = newManager();
    await expect(newDeposit(manager, amount)).rejects.toThrow(msg);
    await expect(
      manager.initiateStellarWithdraw(STELLAR_USER, amount, ETH_USER),
    ).rejects.toThrow(msg);
    expect(manager.getStatistics().totalTransfers).toBe(0);
  });

  it("accepts the exact min and max bounds", async () => {
    const manager = newManager();
    await expect(newDeposit(manager, config.minBridgeAmount)).resolves.toBeDefined();
    await expect(newDeposit(manager, config.maxBridgeAmount)).resolves.toBeDefined();
  });
});

describe("executeAxelarTransfer", () => {
  it("records a durable stage once the bridge message is in flight", async () => {
    const manager = newManager();
    const { id } = await newDeposit(manager);
    axelarAccepts("0xbridge");

    await manager.executeAxelarTransfer(id, "0xsrc");

    expect(manager.getTransfer(id)).toMatchObject({
      stage: "submitted",
      attemptCount: 1,
    });
  });

  it("refuses to submit the same transfer twice", async () => {
    const manager = newManager();
    const { id } = await newDeposit(manager);
    axelarAccepts("0xbridge");
    await manager.executeAxelarTransfer(id, "0xsrc");
    mockedAxios.post.mockClear();

    // The durable record is authoritative: a resubmission attempt is refused
    // instead of putting a second bridge message in flight (#848).
    await expect(manager.executeAxelarTransfer(id, "0xsrc-again")).rejects.toThrow();
    expect(mockedAxios.post).not.toHaveBeenCalled();
    expect(manager.getTransfer(id)).toMatchObject({ stage: "submitted" });
  });

  it("moves pending -> confirming and records hashes", async () => {
    const manager = newManager();
    const { id, netAmount } = await newDeposit(manager);
    axelarAccepts("0xbridge");

    await expect(manager.executeAxelarTransfer(id, "0xsrc")).resolves.toBe(
      "0xbridge",
    );

    expect(manager.getTransfer(id)).toMatchObject({
      status: "confirming",
      sourceChainTxHash: "0xsrc",
      bridgeTxHash: "0xbridge",
    });
    expect(mockedAxios.post).toHaveBeenCalledWith(
      "https://axelar.test/transfers",
      expect.objectContaining({
        sourceChain: "ethereum",
        destinationChain: "stellar",
        amount: netAmount.toString(),
      }),
    );
  });

  it("moves pending -> failed and rethrows when Axelar rejects", async () => {
    const manager = newManager();
    const { id } = await newDeposit(manager);
    mockedAxios.post.mockRejectedValueOnce(new Error("axelar down"));

    await expect(manager.executeAxelarTransfer(id, "0xsrc")).rejects.toThrow(
      "axelar down",
    );
    expect(manager.getTransfer(id)).toMatchObject({
      status: "failed",
      errorMessage: "axelar down",
    });
  });

  it.each<BridgeStatus>(["confirming", "confirmed", "failed", "cancelled"])(
    "refuses to send funds for a %s transfer",
    async (status) => {
      const manager = newManager();
      const id = await transferIn(manager, status);
      mockedAxios.post.mockClear();

      await expect(manager.executeAxelarTransfer(id, "0xsrc")).rejects.toThrow(
        /Invalid transfer status transition/,
      );
      expect(mockedAxios.post).not.toHaveBeenCalled();
      expect(manager.getTransfer(id)?.status).toBe(status);
    },
  );

  it("throws for an unknown transfer", async () => {
    await expect(
      newManager().executeAxelarTransfer("missing", "0xsrc"),
    ).rejects.toThrow("Transfer not found: missing");
  });
});

describe("pollTransferStatus", () => {
  it("moves confirming -> confirmed when Axelar reports executed", async () => {
    const manager = newManager();
    const id = await transferIn(manager, "confirming");
    axelarReports("executed", "0xdest");

    await expect(manager.pollTransferStatus(id)).resolves.toBe("confirmed");
    expect(manager.getTransfer(id)?.destinationTxHash).toBe("0xdest");
    expect(mockedAxios.get).toHaveBeenCalledWith(
      "https://axelar.test/transfers/0xbridge",
    );
  });

  it("moves confirming -> failed when Axelar reports failed", async () => {
    const manager = newManager();
    const id = await transferIn(manager, "confirming");
    axelarReports("failed");

    await expect(manager.pollTransferStatus(id)).resolves.toBe("failed");
  });

  it("stays confirming while Axelar is still processing", async () => {
    const manager = newManager();
    const id = await transferIn(manager, "confirming");
    axelarReports("approved");

    await expect(manager.pollTransferStatus(id)).resolves.toBe("confirming");
  });

  it("keeps the current status when the Axelar API errors", async () => {
    const manager = newManager();
    const id = await transferIn(manager, "confirming");
    mockedAxios.get.mockRejectedValueOnce(new Error("timeout"));

    await expect(manager.pollTransferStatus(id)).resolves.toBe("confirming");
  });

  it.each<BridgeStatus>(["pending", "confirmed", "failed", "cancelled"])(
    "does not query Axelar or change a %s transfer",
    async (status) => {
      const manager = newManager();
      const id = await transferIn(manager, status);
      mockedAxios.get.mockClear();

      await expect(manager.pollTransferStatus(id)).resolves.toBe(status);
      expect(mockedAxios.get).not.toHaveBeenCalled();
    },
  );

  it("throws for an unknown transfer", async () => {
    await expect(newManager().pollTransferStatus("missing")).rejects.toThrow(
      "Transfer not found: missing",
    );
  });
});

describe("retryTransfer", () => {
  it("moves failed -> pending and consumes a retry", async () => {
    const manager = newManager();
    const id = await transferIn(manager, "failed");

    await manager.retryTransfer(id);

    const stored = manager.getTransfer(id) as BridgeTransfer & {
      retriesRemaining: number;
      lastRetryTime?: number;
    };
    expect(stored.status).toBe("pending");
    expect(stored.retriesRemaining).toBe(2);
    expect(stored.lastRetryTime).toBeDefined();
  });

  it("stops after three retries", async () => {
    const manager = newManager();
    const id = await transferIn(manager, "failed");

    for (let i = 0; i < 3; i++) {
      await manager.retryTransfer(id);
      mockedAxios.post.mockRejectedValueOnce(new Error("axelar down"));
      await expect(manager.executeAxelarTransfer(id, "0xsrc")).rejects.toThrow();
    }

    await expect(manager.retryTransfer(id)).rejects.toThrow(
      `No retries remaining for transfer ${id}`,
    );
    expect(manager.getTransfer(id)?.status).toBe("failed");
  });

  it("recovers a failed transfer through to confirmed", async () => {
    const manager = newManager();
    const id = await transferIn(manager, "failed");

    await manager.retryTransfer(id);
    axelarAccepts();
    await manager.executeAxelarTransfer(id, "0xsrc2");
    axelarReports("executed", "0xdest");

    await expect(manager.pollTransferStatus(id)).resolves.toBe("confirmed");
    expect(manager.getTransfer(id)?.sourceChainTxHash).toBe("0xsrc2");
  });

  it.each<BridgeStatus>(["pending", "confirming", "confirmed", "cancelled"])(
    "rejects retrying a %s transfer without consuming a retry",
    async (status) => {
      const manager = newManager();
      const id = await transferIn(manager, status);

      await expect(manager.retryTransfer(id)).rejects.toThrow(
        /Invalid transfer status transition/,
      );
      expect(manager.getTransfer(id)).toMatchObject({
        status,
        retriesRemaining: 3,
      });
    },
  );

  it("throws for an unknown transfer", async () => {
    await expect(newManager().retryTransfer("missing")).rejects.toThrow(
      "Transfer not found: missing",
    );
  });
});

describe("cancelTransfer", () => {
  it("moves pending -> cancelled", async () => {
    const manager = newManager();
    const { id } = await newDeposit(manager);

    await manager.cancelTransfer(id);

    expect(manager.getTransfer(id)?.status).toBe("cancelled");
  });

  it.each<BridgeStatus>(["confirming", "confirmed", "failed", "cancelled"])(
    "rejects cancelling a %s transfer",
    async (status) => {
      const manager = newManager();
      const id = await transferIn(manager, status);

      await expect(manager.cancelTransfer(id)).rejects.toThrow(
        `Cannot cancel transfer in status: ${status}`,
      );
      expect(manager.getTransfer(id)?.status).toBe(status);
    },
  );

  it("throws for an unknown transfer", async () => {
    await expect(newManager().cancelTransfer("missing")).rejects.toThrow(
      "Transfer not found: missing",
    );
  });
});

describe("updatedAt bookkeeping", () => {
  it("advances updatedAt on every transition", async () => {
    const nowSpy = jest.spyOn(Date, "now").mockReturnValue(1_000);
    try {
      const manager = newManager();
      const { id } = await newDeposit(manager);
      expect(manager.getTransfer(id)?.updatedAt).toBe(1_000);

      nowSpy.mockReturnValue(2_000);
      axelarAccepts();
      await manager.executeAxelarTransfer(id, "0xsrc");
      expect(manager.getTransfer(id)?.updatedAt).toBe(2_000);

      nowSpy.mockReturnValue(3_000);
      axelarReports("executed", "0xdest");
      await manager.pollTransferStatus(id);
      expect(manager.getTransfer(id)?.updatedAt).toBe(3_000);
    } finally {
      nowSpy.mockRestore();
    }
  });
});

describe("pending list and statistics", () => {
  it("reflect each transfer's current status", async () => {
    const manager = newManager();
    const pendingId = await transferIn(manager, "pending");
    const confirmingId = await transferIn(manager, "confirming");
    const confirmedId = await transferIn(manager, "confirmed");
    await transferIn(manager, "failed");
    await transferIn(manager, "cancelled");

    expect(
      manager
        .getPendingTransfers()
        .map((t) => t.id)
        .sort(),
    ).toEqual([pendingId, confirmingId].sort());

    const confirmed = manager.getTransfer(confirmedId) as BridgeTransfer;
    expect(manager.getStatistics()).toEqual({
      totalTransfers: 5,
      pendingTransfers: 2,
      confirmedTransfers: 1,
      failedTransfers: 1,
      totalVolume: confirmed.netAmount,
    });
  });
});

describe("verifyAxelarSignature", () => {
  it("accepts a message signed by the relayer (case-insensitive)", async () => {
    const relayer = ethers.Wallet.createRandom();
    const signature = await relayer.signMessage("transfer-1");

    await expect(
      newManager().verifyAxelarSignature(
        "transfer-1",
        signature,
        relayer.address.toLowerCase(),
      ),
    ).resolves.toBe(true);
  });

  it("rejects a signature from another key", async () => {
    const relayer = ethers.Wallet.createRandom();
    const attacker = ethers.Wallet.createRandom();
    const signature = await attacker.signMessage("transfer-1");

    await expect(
      newManager().verifyAxelarSignature("transfer-1", signature, relayer.address),
    ).resolves.toBe(false);
  });

  it("returns false for a malformed signature", async () => {
    await expect(
      newManager().verifyAxelarSignature("transfer-1", "0xdeadbeef", ETH_USER),
    ).resolves.toBe(false);
  });
});
