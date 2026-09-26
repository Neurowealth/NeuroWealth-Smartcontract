import express from 'express';
import bodyParser from 'body-parser';
import dotenv from 'dotenv';
import { handleWhatsAppWebhook } from './webhook';
import { handleDeliveryStatusCallback } from './statusWebhook';
import { verifyTwilioSignature } from './twilioSignature';
import { assertConfig } from './cryptoUtils';

dotenv.config();
assertConfig();

const app = express();
const PORT = process.env.PORT || 3000;

app.use(bodyParser.urlencoded({ extended: false }));
app.use(bodyParser.json());

// Health check endpoint
app.get('/health', (req, res) => {
  res.json({ status: 'ok', service: 'NeuroWealth WhatsApp Bot Handler' });
});

// WhatsApp Twilio Webhook route
// Only requests signed by Twilio (X-Twilio-Signature) reach the handler.
app.post('/api/whatsapp/webhook', verifyTwilioSignature(), handleWhatsAppWebhook);

// Twilio delivery-status callback for outbound WhatsApp messages
// (queued/sent/delivered/failed/undelivered). Signature validation is
// mandatory here too, before any status is ever persisted.
app.post(
  '/api/whatsapp/status',
  verifyTwilioSignature({
    authToken: process.env.TWILIO_AUTH_TOKEN,
    webhookUrl: process.env.TWILIO_STATUS_CALLBACK_URL,
    skipValidation: process.env.TWILIO_SKIP_SIGNATURE_VALIDATION === 'true',
    nodeEnv: process.env.NODE_ENV,
  }),
  handleDeliveryStatusCallback
);

app.listen(PORT, () => {
  console.log(`🚀 NeuroWealth WhatsApp Bot Handler running on port ${PORT}`);
  console.log(`Webhook URL: http://localhost:${PORT}/api/whatsapp/webhook`);
});
