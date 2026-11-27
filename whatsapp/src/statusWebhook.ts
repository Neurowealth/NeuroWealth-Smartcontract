import { Request, Response } from "express";
import pino from "pino";
import {
  applyDeliveryStatus,
  retryFailedDelivery,
} from "./messageDeliveryService";
import { boundProviderCallback } from "./inputBounds";
import { smsClient } from "./smsService";

const logger = pino({ name: "whatsapp-status-webhook" });

/**
 * Twilio calls this URL asynchronously as an outbound WhatsApp message moves
 * through queued -> sent -> delivered (or failed / undelivered). We always
 * acknowledge with 200 so Twilio does not retry-storm us - even a callback
 * for an identifier we can't reconcile is logged and dropped, not failed.
 */
export async function handleDeliveryStatusCallback(
  req: Request,
  res: Response,
): Promise<void> {
  const body = (req.body ?? {}) as Record<string, unknown>;
  const messageSid =
    [body.MessageSid, body.SmsSid].find(
      (v) => v !== undefined && v !== null && v !== "",
    ) ?? "";
  const status =
    typeof body.MessageStatus === "string"
      ? body.MessageStatus
      : (body.MessageStatus ?? "");
  const bounded = boundProviderCallback({
    messageSid,
    status,
    to: body.To,
    errorCode: body.ErrorCode,
    errorMessage: body.ErrorMessage,
  });

  if (!bounded.ok) {
    logger.warn(
      { field: bounded.field, reason: bounded.reason },
      "Dropping provider callback outside input bounds",
    );
    res.status(200).send();
    return;
  }

  try {
    const result = await applyDeliveryStatus({
      messageSid: bounded.value.messageSid,
      status: bounded.value.status.toLowerCase(),
      to: bounded.value.to,
      errorCode: bounded.value.errorCode,
      errorMessage: bounded.value.errorMessage,
    });

    // Trigger retry/escalation on terminal failure
    if (
      result.applied &&
      (bounded.value.status.toLowerCase() === "failed" ||
        bounded.value.status.toLowerCase() === "undelivered")
    ) {
      await retryFailedDelivery(
        bounded.value.messageSid,
        async (to) => {
          // Resend SMS without the OTP code (code is device-only per #942)
          // The user's device has the original code, so this is just a reattempt to deliver
          await smsClient.send(
            to,
            "Your NeuroWealth verification code was not delivered. Please check your SMS or request a new code in the WhatsApp chat.",
          );
        },
        (sid) => {
          logger.error(
            { messageSid: sid },
            "OTP delivery failure escalated after exhausting retries",
          );
        },
      );
    }
  } catch (error) {
    logger.error(
      { error, messageSid: bounded.value.messageSid },
      "Failed to persist delivery status callback",
    );
  }

  res.status(200).send();
}
