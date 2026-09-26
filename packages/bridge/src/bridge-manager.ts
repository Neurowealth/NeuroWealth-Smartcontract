/**
 * Main bridge manager - orchestrates cross-chain transfers via Axelar
 */

import * as crypto from "crypto";
import pino from "pino";
import { v4 as uuidv4 } from "uuid";
import * as StellarSdk from "@stellar/stellar-sdk";
import { ethers } from "ethers";
import axios from "axios";

/** Maximum age (seconds) accepted for a webhook timestamp before it is rejected as a replay. */
export const WEBHOOK_TIMESTAMP_TOLERANCE_SECONDS = 300;
import {
  BridgeConfig,
  BridgeTransfer,
  BridgeQuote,
  BridgeStatus,
  StoredBridgeTransfer,
  BridgeChain,
  BridgeTransferPatch,
  INITIAL_STAGE,
  TransferStage,
} from "./types";
import { BridgeStore, InMemoryBridgeStore, isTerminalStatus } from "./bridge-store";
import { redactTransfer, sanitizeErrorMessage } from "./redaction";

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

/** Base delay of the reconciliation backoff for unknown external states (#850). */
export const RECONCILE_BACKOFF_BASE_MS = 30_000;
/** Upper bound of the reconciliation backoff (#850). */
export const RECONCILE_BACKOFF_MAX_MS = 15 * 60_000;

/**
 * Exponential, bounded backoff for records whose external state could not be
 * determined. Unknown states stay retryable but never hot-loop.
 */
export function backoffDelayMs(attemptCount: number): number {
  const exponent = Math.max(0, Math.min(attemptCount, 16));
  return Math.min(
    RECONCILE_BACKOFF_BASE_MS * 2 ** exponent,
    RECONCILE_BACKOFF_MAX_MS,
  );
}

/** Result of looking a transfer up on one of the two chains (#850). */
export type ExternalChainState = "found" | "missing" | "unknown";

/** What the bridge knows about a transfer after probing both chains. */
export interface ExternalTransferState {
  sourceChain: ExternalChainState;
  destinationChain: ExternalChainState;
  axelarStatus?: string;
  destinationTxHash?: string;
  confirmationDepth?: number;
}

export class BridgeManager {
  private logger = pino();
  private stellarServer: StellarSdk.SorobanRpc.Server;
  private ethersProvider: ethers.Provider;
  private bridgeTransfers: Map<string, StoredBridgeTransfer> = new Map();

  constructor(
    private config: BridgeConfig,
    private store: BridgeStore = new InMemoryBridgeStore(),
  ) {
    this.stellarServer = new StellarSdk.SorobanRpc.Server(config.stellarRpcUrl);
    this.ethersProvider = new ethers.JsonRpcProvider(config.ethereumRpcUrl);

    // #851 - Validate confirmation depth configuration
    this.validateConfirmationDepths();
  }

  /**
   * The durable store backing this manager. A restarted process must construct
   * its manager with the same store to see in-flight transfers (#848).
   */
  getStore(): BridgeStore {
    return this.store;
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
   * #848 - Restores every durable record into the in-memory mirror.
   *
   * Must be awaited before serving traffic: after a restart the process knows
   * about in-flight transfers only because they were written to the store on
   * every transition.
   */
  async loadDurableState(): Promise<StoredBridgeTransfer[]> {
    const records = await this.store.getAll();
    for (const record of records) {
      this.bridgeTransfers.set(record.id, { ...record });
    }

    const nonTerminal = records.filter((record) => !isTerminalStatus(record.status));
    this.logger.info(
      {
        restored: records.length,
        nonTerminal: nonTerminal.length,
      },
      "Durable bridge state loaded",
    );
    return nonTerminal;
  }

  /**
   * #848/#850 - Durable records that still need work after a restart.
   */
  async getIncompleteTransfers(): Promise<StoredBridgeTransfer[]> {
    return this.store.getNonTerminal();
  }

  /**
   * #850 - Makes a durable record visible to the in-memory operations (poll,
   * resume) so reconciliation can drive it without a full reload.
   */
  attachDurableRecord(record: StoredBridgeTransfer): void {
    this.bridgeTransfers.set(record.id, { ...record });
  }

  /**
   * #851 - Confirmation depth a destination chain requires, exposed for the
   * reconciliation pass.
   */
  getRequiredConfirmationDepth(chain: BridgeChain): number {
    return this.getConfirmationDepth(chain);
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
    const existingTransfer = await this.findByIdempotencyKey(idempotencyKey);
    if (existingTransfer) {
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
        { transfer: redactTransfer(existingTransfer), idempotencyKey },
        "Returning existing transfer for idempotent retry",
      );
      return {
        ...existingTransfer,
        status: existingTransfer.status,
      };
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

    await this.persistNew(transfer);

    this.logger.info(
      { transfer: redactTransfer(this.mirror(transfer.id)!), idempotencyKey },
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
    const existingTransfer = await this.findByIdempotencyKey(idempotencyKey);
    if (existingTransfer) {
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
        { transfer: redactTransfer(existingTransfer), idempotencyKey },
        "Returning existing transfer for idempotent retry",
      );
      return {
        ...existingTransfer,
        status: existingTransfer.status,
      };
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

    await this.persistNew(transfer);

    this.logger.info(
      { transfer: redactTransfer(this.mirror(transfer.id)!), idempotencyKey },
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

    // #848 - A restart must never create a second source-chain submission.
    // Once a bridge message is in flight the durable record is authoritative:
    // the reconciliation pass polls it instead of re-submitting.
    if (transfer.stage !== INITIAL_STAGE || transfer.bridgeTxHash) {
      throw new Error(
        `Transfer ${transferId} was already submitted (stage: ${transfer.stage})`,
      );
    }

    this.logger.info(
      { transfer: redactTransfer(transfer), sourceChainTxHash },
      "Executing Axelar transfer",
    );

    try {
      // Build GMP message payload
      const payload = this.encodeGMPPayload(transfer);

      // Send via Axelar API. The transfer id doubles as the external
      // idempotency key, so a retried submission collapses onto the message
      // that is already in flight (#848).
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
          idempotencyKey: transfer.id,
        },
      );

      const bridgeTxHash = axelarResponse.data.transactionHash;

      // Update transfer status
      this.setStatus(transfer, "confirming");
      transfer.stage = "submitted"; // #848
      transfer.sourceChainTxHash = sourceChainTxHash;
      transfer.bridgeTxHash = bridgeTxHash;
      transfer.attemptCount += 1;
      transfer.lastError = undefined;
      transfer.nextAttemptAt = undefined;
      transfer.updatedAt = Date.now();
      await this.persist(transfer);

      this.logger.info(
        { transfer: redactTransfer(transfer), bridgeTxHash },
        "Axelar transfer submitted",
      );

      return bridgeTxHash;
    } catch (error) {
      // #848 - Persist the failure (sanitised) so a restart knows what happened
      // and can back off instead of hammering the bridge.
      transfer.attemptCount += 1;
      transfer.errorMessage = sanitizeErrorMessage(error);
      transfer.lastError = transfer.errorMessage;
      transfer.nextAttemptAt = Date.now() + backoffDelayMs(transfer.attemptCount);
      this.setStatus(transfer, "failed");
      await this.persist(transfer);

      this.logger.error(
        { transfer: redactTransfer(transfer), errorClass: this.classifyError(error) },
        "Axelar transfer failed",
      );
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
          transfer.stage = "confirmed"; // #848
          transfer.destinationTxHash = response.data.destinationTxHash;
          transfer.lastError = undefined;
          transfer.nextAttemptAt = undefined;
          await this.persist(transfer);
          this.logger.info(
            { transferId, currentDepth, requiredDepth },
            "Transfer confirmed with sufficient depth",
          );
        } else {
          await this.persist(transfer);
          this.logger.info(
            { transferId, currentDepth, requiredDepth },
            "Transfer executed but waiting for required confirmation depth",
          );
        }
      } else if (axelarStatus === "failed") {
        transfer.attemptCount += 1;
        transfer.lastError = "Axelar reported the transfer as failed";
        transfer.nextAttemptAt = Date.now() + backoffDelayMs(transfer.attemptCount);
        this.setStatus(transfer, "failed");
        await this.persist(transfer);
      }

      return transfer.status;
    } catch (error) {
      // An unreachable bridge is an unknown external state, not a failure:
      // keep the record retryable with bounded backoff (#850).
      transfer.attemptCount += 1;
      transfer.lastError = sanitizeErrorMessage(error);
      transfer.nextAttemptAt = Date.now() + backoffDelayMs(transfer.attemptCount);
      await this.persist(transfer);

      this.logger.error(
        { transfer: redactTransfer(transfer), errorClass: this.classifyError(error) },
        "Failed to poll transfer status",
      );
      return transfer.status;
    }
  }

  /**
   * #850 - Queries both chains before a decision is made, so reconciliation
   * never resubmits a transfer the destination already executed.
   */
  async probeExternalState(
    transfer: StoredBridgeTransfer,
  ): Promise<ExternalTransferState> {
    const sourceChain = await this.probeSourceChain(transfer);
    const destination = await this.probeDestinationChain(transfer);
    return { sourceChain, ...destination };
  }

  private async probeSourceChain(
    transfer: StoredBridgeTransfer,
  ): Promise<ExternalChainState> {
    if (!transfer.sourceChainTxHash) {
      // Nothing has been submitted on the source chain yet.
      return "unknown";
    }

    try {
      if (transfer.sourceChain === "ethereum") {
        const receipt = await this.ethersProvider.getTransactionReceipt(
          transfer.sourceChainTxHash,
        );
        return receipt ? "found" : "missing";
      }

      const response = await this.stellarServer.getTransaction(
        transfer.sourceChainTxHash,
      );
      return response && response.status !== "NOT_FOUND" ? "found" : "missing";
    } catch (error) {
      this.logger.debug(
        { transferId: transfer.id, errorClass: this.classifyError(error) },
        "Source-chain probe inconclusive",
      );
      return "unknown";
    }
  }

  private async probeDestinationChain(
    transfer: StoredBridgeTransfer,
  ): Promise<Omit<ExternalTransferState, "sourceChain">> {
    if (!transfer.bridgeTxHash) {
      return { destinationChain: "unknown" };
    }

    try {
      const response = await axios.get(
        `${this.config.axelarApiUrl}/transfers/${transfer.bridgeTxHash}`,
      );
      return {
        destinationChain: "found",
        axelarStatus: response.data.status,
        destinationTxHash: response.data.destinationTxHash,
        confirmationDepth: response.data.confirmationDepth || 0,
      };
    } catch (error) {
      this.logger.debug(
        { transferId: transfer.id, errorClass: this.classifyError(error) },
        "Destination-chain probe inconclusive",
      );
      return { destinationChain: "unknown" };
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
    transfer.attemptCount += 1;
    transfer.lastError = undefined;
    transfer.nextAttemptAt = Date.now() + backoffDelayMs(transfer.attemptCount);
    this.setStatus(transfer, "pending");
    await this.persist(transfer);
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
    transfer.stage = "completed"; // #848 - nothing left to resume
    await this.persist(transfer);

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

  /**
   * #854 - Verify the HMAC-SHA256 signature on an inbound bridge webhook.
   *
   * The provider signs `${timestamp}.${rawBody}` with the shared secret.
   * Verification uses constant-time comparison to prevent timing attacks.
   * Timestamps outside the tolerance window are rejected to prevent replays.
   *
   * @param rawBody   Raw (unparsed) request body buffer
   * @param timestamp Unix seconds string from the provider header (e.g. "X-Bridge-Timestamp")
   * @param signature Hex-encoded HMAC-SHA256 from the provider header (e.g. "X-Bridge-Signature")
   * @param secret    Shared signing secret for this provider
   * @param nowSeconds Current time in seconds (injectable for testing; defaults to Date.now()/1000)
   * @returns true if the signature is valid and the timestamp is fresh
   */
  verifyWebhookSignature(
    rawBody: Buffer,
    timestamp: string,
    signature: string,
    secret: string,
    nowSeconds: number = Date.now() / 1000,
  ): boolean {
    const ts = Number(timestamp);
    if (!Number.isFinite(ts) || timestamp.trim() === "") {
      this.logger.warn({ timestamp }, "Webhook rejected: malformed timestamp");
      return false;
    }

    const age = nowSeconds - ts;
    if (age < 0 || age > WEBHOOK_TIMESTAMP_TOLERANCE_SECONDS) {
      this.logger.warn(
        { age: Math.round(age), tolerance: WEBHOOK_TIMESTAMP_TOLERANCE_SECONDS },
        "Webhook rejected: stale or future timestamp",
      );
      return false;
    }

    let expected: Buffer;
    try {
      expected = Buffer.from(
        crypto
          .createHmac("sha256", secret)
          .update(`${timestamp}.`)
          .update(rawBody)
          .digest("hex"),
        "utf8",
      );
    } catch (error) {
      this.logger.error({ error }, "Webhook HMAC computation failed");
      return false;
    }

    let actual: Buffer;
    try {
      actual = Buffer.from(signature, "utf8");
    } catch {
      this.logger.warn("Webhook rejected: signature encoding error");
      return false;
    }

    if (expected.length !== actual.length) {
      this.logger.warn("Webhook rejected: invalid signature");
      return false;
    }

    const valid = crypto.timingSafeEqual(expected, actual);
    if (!valid) {
      this.logger.warn("Webhook rejected: signature mismatch");
    }
    return valid;
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
   * #848 - Persists a brand new transfer and mirrors it in memory.
   */
  private async persistNew(transfer: BridgeTransfer): Promise<void> {
    const record: StoredBridgeTransfer = {
      ...transfer,
      retriesRemaining: 3,
      stage: INITIAL_STAGE,
      attemptCount: 0,
    };
    this.bridgeTransfers.set(record.id, record);
    await this.store.save(record);
  }

  /**
   * #848 - Write-through persistence. The durable copy is the source of truth
   * after a restart, so every mutation lands in the store before the caller
   * continues.
   */
  private async persist(transfer: StoredBridgeTransfer): Promise<void> {
    this.bridgeTransfers.set(transfer.id, transfer);
    await this.store.update(transfer.id, this.toPatch(transfer));
  }

  /**
   * Builds the patch written to the store: the full durable workflow state
   * minus the identity field, with free-form text already sanitised.
   */
  private toPatch(transfer: StoredBridgeTransfer): BridgeTransferPatch {
    const { id: _id, user: _user, amount: _amount, bridgeFee: _bridgeFee, netAmount: _net, ...rest } =
      transfer;
    return { ...rest, lastError: transfer.lastError };
  }

  private mirror(transferId: string): StoredBridgeTransfer | undefined {
    return this.bridgeTransfers.get(transferId);
  }

  private async findByIdempotencyKey(
    idempotencyKey: string | undefined,
  ): Promise<StoredBridgeTransfer | undefined> {
    if (!idempotencyKey) {
      return undefined;
    }

    // Durable lookup first: after a restart the mirror is empty, and a retry
    // must still return the original transfer instead of creating a second one.
    const all = await this.store.getAll();
    return all.find((t) => t.idempotencyKey === idempotencyKey);
  }

  /**
   * #850 - Coarse error classes used by the reconciliation summary.
   */
  classifyError(error: unknown): string {
    const message =
      error instanceof Error ? error.message : String(error ?? "unknown");
    const lower = message.toLowerCase();

    if (lower.includes("timeout") || lower.includes("timed out")) return "timeout";
    if (
      [
        "econnreset",
        "econnrefused",
        "etimedout",
        "enotfound",
        "network",
        "socket",
        "reset by peer",
        "getaddrinfo",
        "unreachable",
        "failed to fetch",
      ].some((needle) => lower.includes(needle))
    ) {
      return "network";
    }
    if (lower.includes("429") || lower.includes("rate limit"))
      return "rate_limited";
    if (lower.includes("not found") || lower.includes("404"))
      return "not_found";
    return "unknown";
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
