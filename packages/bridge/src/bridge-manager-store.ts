/**
 * Main bridge manager - orchestrates cross-chain transfers via Axelar
 */

import pino from "pino";
import { v4 as uuidv4 } from "uuid";
import * as StellarSdk from "@stellar/stellar-sdk";
import { ethers } from "ethers";
import axios from "axios";
import {
  BridgeConfig,
  BridgeTransfer,
  BridgeQuote,
  BridgeStatus,
  StoredBridgeTransfer,
  BridgeChain,
  INITIAL_STAGE,
} from "./types";

/**
 * Allowed status transitions. `confirmed` and `cancelled` are terminal;
 * `failed` can only go back to `pending` via an explicit retry. Once a
 * transfer is `confirming` its Axelar message is in flight, so it can no
 * longer be cancelled locally.
 */
export const ALLOWED_TRANSITIONS: Record<BridgeStatus, readonly BridgeStatus[]> = {
  pending: ["confirming", "failed", "cancelled"],
  confirming: ["confirmed", "failed"],
  failed: ["pending"],
  confirmed: [],
  cancelled: [],
};

export function canTransition(from: BridgeStatus, to: BridgeStatus): boolean {
  return ALLOWED_TRANSITIONS[from].includes(to);
}

export class BridgeManager {
  private logger = pino();
  private stellarServer: StellarSdk.SorobanRpc.Server;
  private ethersProvider: ethers.Provider;
  private bridgeTransfers: Map<string, StoredBridgeTransfer> = new Map();

  constructor(private config: BridgeConfig) {
    this.stellarServer = new StellarSdk.SorobanRpc.Server(config.stellarRpcUrl);
    this.ethersProvider = new ethers.JsonRpcProvider(config.ethereumRpcUrl);

    // #851 - Validate confirmation depth configuration
    this.validateConfirmationDepths();
  }

  /**
   * #851 - Validate confirmation depth configuration at startup
   */
  private validateConfirmationDepths(): void {
    const requiredChains: BridgeChain[] = ["stellar", "ethereum"];

    for (const chain of requiredChains) {
      const depth = this.config.confirmationDepths?.[chain];
      if (depth === undefined || depth === null) {
        throw new Error(
          `Confirmation depth not configured for chain: ${chain}. Please set confirmationDepths.${chain} in config.`,
        );
      }
      if (depth < 0) {
        throw new Error(
          `Confirmation depth must be non-negative for chain: ${chain}. Got: ${depth}`,
        );
      }
      if (depth === 0) {
        this.logger.warn(
          { chain, depth },
          "Confirmation depth set to 0 - transfers will be confirmed immediately without waiting for confirmations",
        );
      }
    }

    this.logger.info(
      { confirmationDepths: this.config.confirmationDepths },
      "Confirmation depth configuration validated",
    );
  }

  /**
   * #851 - Get the required confirmation depth for a destination chain
   */
  private getConfirmationDepth(chain: BridgeChain): number {
    const depth = this.config.confirmationDepths?.[chain];
    if (depth === undefined) {
      throw new Error(`No confirmation depth configured for chain: ${chain}`);
    }
    return depth;
  }

  /**
   * Initiate a deposit from Ethereum → Stellar via Axelar
   */
  async initiateEthereumDeposit(
    ethereumUserAddress: string,
    usdcAmount: bigint,
    destinationStellarAddress: string,
    idempotencyKey?: string, // #849 - Optional idempotency key for safe retries
  ): Promise<BridgeTransfer> {
    this.logger.info(
      {
        ethereumUserAddress,
        usdcAmount: usdcAmount.toString(),
        destinationStellarAddress,
        idempotencyKey,
      },
      "Initiating Ethereum → Stellar deposit",
    );

    // Validate amount
    if (usdcAmount < this.config.minBridgeAmount) {
      throw new Error(
        `Amount below minimum: ${this.config.minBridgeAmount.toString()}`,
      );
    }
    if (usdcAmount > this.config.maxBridgeAmount) {
      throw new Error(
        `Amount above maximum: ${this.config.maxBridgeAmount.toString()}`,
      );
    }

    // #849 - Check for existing transfer with same idempotency key
    if (idempotencyKey) {
      const existingTransfer = Array.from(this.bridgeTransfers.values()).find(
        (t) => t.idempotencyKey === idempotencyKey,
      );

      if (existingTransfer) {
        // Verify that the payload matches the original request
        if (
          existingTransfer.user !== destinationStellarAddress ||
          existingTransfer.amount !== usdcAmount ||
          existingTransfer.sourceChain !== "ethereum" ||
          existingTransfer.destinationChain !== "stellar"
        ) {
          throw new Error(
            "Idempotency key already used with different parameters. Use a new key.",
          );
        }

        // Return the existing transfer for safe retry
        this.logger.info(
          { transferId: existingTransfer.id, idempotencyKey },
          "Returning existing transfer for idempotent retry",
        );
        return {
          ...existingTransfer,
          status: existingTransfer.status,
        };
      }
    }

    // Calculate bridge fee
    const bridgeFee =
      (usdcAmount * BigInt(Math.floor(this.config.bridgeFeePercentage * 100))) /
      BigInt(10000);
    const netAmount = usdcAmount - bridgeFee;

    const transfer: BridgeTransfer = {
      id: uuidv4(),
      status: "pending",
      direction: "deposit",
      sourceChain: "ethereum",
      destinationChain: "stellar",
      user: destinationStellarAddress,
      amount: usdcAmount,
      bridgeFee,
      netAmount,
      createdAt: Date.now(),
      updatedAt: Date.now(),
      idempotencyKey, // #849 - Store the idempotency key
      requiredConfirmationDepth: this.getConfirmationDepth("stellar"), // #851 - Store required depth
    };

    // Store transfer
    this.bridgeTransfers.set(transfer.id, {
      ...transfer,
      retriesRemaining: 3,
      stage: INITIAL_STAGE,
      attemptCount: 0,
    });

    this.logger.info(
      { transferId: transfer.id, status: transfer.status, idempotencyKey },
      "Bridge transfer initiated",
    );

    return transfer;
  }

  /**
   * Initiate a withdrawal from Stellar → Ethereum via Axelar
   */
  async initiateStellarWithdraw(
    stellarUserAddress: string,
    usdcAmount: bigint,
    destinationEthereumAddress: string,
    idempotencyKey?: string, // #849 - Optional idempotency key for safe retries
  ): Promise<BridgeTransfer> {
    this.logger.info(
      {
        stellarUserAddress,
        usdcAmount: usdcAmount.toString(),
        destinationEthereumAddress,
        idempotencyKey,
      },
      "Initiating Stellar → Ethereum withdrawal",
    );

    // Validate amount
    if (usdcAmount < this.config.minBridgeAmount) {
      throw new Error(
        `Amount below minimum: ${this.config.minBridgeAmount.toString()}`,
      );
    }
    if (usdcAmount > this.config.maxBridgeAmount) {
      throw new Error(
        `Amount above maximum: ${this.config.maxBridgeAmount.toString()}`,
      );
    }

    // #849 - Check for existing transfer with same idempotency key
    if (idempotencyKey) {
      const existingTransfer = Array.from(this.bridgeTransfers.values()).find(
        (t) => t.idempotencyKey === idempotencyKey,
      );

      if (existingTransfer) {
        // Verify that the payload matches the original request
        if (
          existingTransfer.user !== stellarUserAddress ||
          existingTransfer.amount !== usdcAmount ||
          existingTransfer.sourceChain !== "stellar" ||
          existingTransfer.destinationChain !== "ethereum"
        ) {
          throw new Error(
            "Idempotency key already used with different parameters. Use a new key.",
          );
        }

        // Return the existing transfer for safe retry
        this.logger.info(
          { transferId: existingTransfer.id, idempotencyKey },
          "Returning existing transfer for idempotent retry",
        );
        return {
          ...existingTransfer,
          status: existingTransfer.status,
        };
      }
    }

    // Calculate bridge fee
    const bridgeFee =
      (usdcAmount * BigInt(Math.floor(this.config.bridgeFeePercentage * 100))) /
      BigInt(10000);
    const netAmount = usdcAmount - bridgeFee;

    const transfer: BridgeTransfer = {
      id: uuidv4(),
      status: "pending",
      direction: "withdraw",
      sourceChain: "stellar",
      destinationChain: "ethereum",
      user: stellarUserAddress,
      amount: usdcAmount,
      bridgeFee,
      netAmount,
      createdAt: Date.now(),
      updatedAt: Date.now(),
      estimatedArrivalTime: Date.now() + 15 * 60 * 1000, // ~15 min
      idempotencyKey, // #849 - Store the idempotency key
      requiredConfirmationDepth: this.getConfirmationDepth("ethereum"), // #851 - Store required depth
    };

    this.bridgeTransfers.set(transfer.id, {
      ...transfer,
      retriesRemaining: 3,
      stage: INITIAL_STAGE,
      attemptCount: 0,
    });

    this.logger.info(
      { transferId: transfer.id, status: transfer.status, idempotencyKey },
      "Bridge transfer initiated",
    );

    return transfer;
  }

  /**
   * Get quote for bridge transfer
   */
  async getBridgeQuote(
    amount: bigint,
    direction: "eth_to_stellar" | "stellar_to_eth",
  ): Promise<BridgeQuote> {
    const bridgeFee =
      (amount * BigInt(Math.floor(this.config.bridgeFeePercentage * 100))) /
      BigInt(10000);
    const netAmount = amount - bridgeFee;

    // Estimate based on direction
    const estimatedTime = direction === "eth_to_stellar" ? 10 * 60 : 15 * 60; // seconds

    return {
      amount,
      bridgeFee,
      netAmount,
      estimatedTime,
      slippagePercentage: 0.1, // 0.1% slippage buffer
    };
  }

  /**
   * Execute transfer via Axelar GMP
   */
  async executeAxelarTransfer(
    transferId: string,
    sourceChainTxHash: string,
  ): Promise<string> {
    const transfer = this.bridgeTransfers.get(transferId);
    if (!transfer) {
      throw new Error(`Transfer not found: ${transferId}`);
    }
    this.assertTransition(transfer, "confirming");

    this.logger.info(
      { transferId, sourceChainTxHash },
      "Executing Axelar transfer",
    );

    try {
      // Build GMP message payload
      const payload = this.encodeGMPPayload(transfer);

      // Send via Axelar API
      const axelarResponse = await axios.post(
        `${this.config.axelarApiUrl}/transfers`,
        {
          sourceChain:
            transfer.sourceChain === "ethereum" ? "ethereum" : "stellar",
          destinationChain:
            transfer.destinationChain === "ethereum" ? "ethereum" : "stellar",
          payload,
          amount: transfer.netAmount.toString(),
          gasLimit: "500000",
        },
      );

      const bridgeTxHash = axelarResponse.data.transactionHash;

      // Update transfer status
      this.setStatus(transfer, "confirming");
      transfer.sourceChainTxHash = sourceChainTxHash;
      transfer.bridgeTxHash = bridgeTxHash;
      transfer.updatedAt = Date.now();

      this.logger.info(
        { transferId, bridgeTxHash },
        "Axelar transfer submitted",
      );

      return bridgeTxHash;
    } catch (error) {
      this.logger.error({ error, transferId }, "Axelar transfer failed");
      this.setStatus(transfer, "failed");
      transfer.errorMessage =
        error instanceof Error ? error.message : "Unknown error";
      transfer.updatedAt = Date.now();
      throw error;
    }
  }

  /**
   * Poll Axelar for transfer status
   */
  async pollTransferStatus(transferId: string): Promise<BridgeStatus> {
    const transfer = this.bridgeTransfers.get(transferId);
    if (!transfer) {
      throw new Error(`Transfer not found: ${transferId}`);
    }

    // Only in-flight transfers have a remote status worth polling.
    if (transfer.status !== "confirming" || !transfer.bridgeTxHash) {
      return transfer.status;
    }

    try {
      const response = await axios.get(
        `${this.config.axelarApiUrl}/transfers/${transfer.bridgeTxHash}`,
      );

      const axelarStatus = response.data.status;
      const currentDepth = response.data.confirmationDepth || 0;
      const requiredDepth = this.getConfirmationDepth(transfer.destinationChain);

      this.logger.debug(
        {
          transferId,
          axelarStatus,
          currentDepth,
          requiredDepth,
          destinationChain: transfer.destinationChain,
        },
        "Checking transfer confirmation depth",
      );

      // #851 - Only mark as confirmed when required depth is reached
      if (axelarStatus === "executed") {
        // Update current confirmation depth for monitoring
        transfer.currentConfirmationDepth = currentDepth;

        if (currentDepth >= requiredDepth) {
          this.setStatus(transfer, "confirmed");
          transfer.destinationTxHash = response.data.destinationTxHash;
          this.logger.info(
            { transferId, currentDepth, requiredDepth },
            "Transfer confirmed with sufficient depth",
          );
        } else {
          this.logger.info(
            { transferId, currentDepth, requiredDepth },
            "Transfer executed but waiting for required confirmation depth",
          );
        }
      } else if (axelarStatus === "failed") {
        this.setStatus(transfer, "failed");
      }

      return transfer.status;
    } catch (error) {
      this.logger.error(
        { error, transferId },
        "Failed to poll transfer status",
      );
      return transfer.status;
    }
  }

  /**
   * Retry failed transfer
   */
  async retryTransfer(transferId: string): Promise<void> {
    const transfer = this.bridgeTransfers.get(transferId);
    if (!transfer) {
      throw new Error(`Transfer not found: ${transferId}`);
    }

    this.assertTransition(transfer, "pending");
    if (transfer.retriesRemaining <= 0) {
      throw new Error(`No retries remaining for transfer ${transferId}`);
    }

    this.logger.info(
      { transferId, retriesRemaining: transfer.retriesRemaining - 1 },
      "Retrying failed transfer",
    );

    transfer.retriesRemaining -= 1;
    transfer.lastRetryTime = Date.now();
    this.setStatus(transfer, "pending");
  }

  /**
   * Cancel pending transfer
   */
  async cancelTransfer(transferId: string): Promise<void> {
    const transfer = this.bridgeTransfers.get(transferId);
    if (!transfer) {
      throw new Error(`Transfer not found: ${transferId}`);
    }

    if (!canTransition(transfer.status, "cancelled")) {
      throw new Error(`Cannot cancel transfer in status: ${transfer.status}`);
    }

    this.setStatus(transfer, "cancelled");

    this.logger.info({ transferId }, "Transfer cancelled");
  }

  /**
   * Get transfer status
   */
  getTransfer(transferId: string): BridgeTransfer | undefined {
    return this.bridgeTransfers.get(transferId);
  }

  /**
   * Verify Axelar relayer signature
   */
  async verifyAxelarSignature(
    message: string,
    signature: string,
    relayerAddress: string,
  ): Promise<boolean> {
    try {
      const recoveredAddress = ethers.verifyMessage(message, signature);
      return recoveredAddress.toLowerCase() === relayerAddress.toLowerCase();
    } catch (error) {
      this.logger.error({ error }, "Failed to verify signature");
      return false;
    }
  }

  private assertTransition(
    transfer: StoredBridgeTransfer,
    to: BridgeStatus,
  ): void {
    if (!canTransition(transfer.status, to)) {
      throw new Error(
        `Invalid transfer status transition: ${transfer.status} -> ${to}`,
      );
    }
  }

  private setStatus(transfer: StoredBridgeTransfer, to: BridgeStatus): void {
    this.assertTransition(transfer, to);
    transfer.status = to;
    transfer.updatedAt = Date.now();
  }

  /**
   * Encode GMP payload for Axelar
   */
  private encodeGMPPayload(transfer: BridgeTransfer): string {
    const payload = {
      version: 1,
      transferId: transfer.id,
      user: transfer.user,
      amount: transfer.netAmount.toString(),
      destinationChain: transfer.destinationChain,
      timestamp: Date.now(),
    };

    return Buffer.from(JSON.stringify(payload)).toString("hex");
  }

  /**
   * Get all pending transfers
   */
  getPendingTransfers(): BridgeTransfer[] {
    return Array.from(this.bridgeTransfers.values()).filter(
      (t) => t.status === "pending" || t.status === "confirming",
    );
  }

  /**
   * Get transfer statistics
   */
  getStatistics(): {
    totalTransfers: number;
    pendingTransfers: number;
    confirmedTransfers: number;
    failedTransfers: number;
    totalVolume: bigint;
  } {
    const transfers = Array.from(this.bridgeTransfers.values());
    const confirmed = transfers.filter((t) => t.status === "confirmed");
    const pending = transfers.filter(
      (t) => t.status === "pending" || t.status === "confirming",
    );
    const failed = transfers.filter((t) => t.status === "failed");

    const totalVolume = confirmed.reduce(
      (acc, t) => acc + t.netAmount,
      BigInt(0),
    );

    return {
      totalTransfers: transfers.length,
      pendingTransfers: pending.length,
      confirmedTransfers: confirmed.length,
      failedTransfers: failed.length,
      totalVolume,
    };
  }
}
