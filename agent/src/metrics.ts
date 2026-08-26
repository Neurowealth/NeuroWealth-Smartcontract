import { Registry, Histogram, Counter, collectDefaultMetrics } from 'prom-client';
import express from 'express';

export const metricsRegistry = new Registry();

// Add default metrics (CPU, memory, etc.)
collectDefaultMetrics({ register: metricsRegistry });

// Track request duration histogram
export const requestDuration = new Histogram({
  name: 'http_request_duration_seconds',
  help: 'Duration of HTTP requests in seconds',
  labelNames: ['method', 'route', 'status_code'],
  registers: [metricsRegistry],
});

// Track error rate counter
export const errorRate = new Counter({
  name: 'app_error_total',
  help: 'Total number of application errors',
  labelNames: ['type'],
  registers: [metricsRegistry],
});

// Track OpenAI API usage
export const openAiUsage = new Counter({
  name: 'openai_api_usage_total',
  help: 'Total number of OpenAI API calls',
  labelNames: ['model', 'action'],
  registers: [metricsRegistry],
});

// Track Stellar RPC latency
export const stellarRpcLatency = new Histogram({
  name: 'stellar_rpc_latency_seconds',
  help: 'Latency of Stellar RPC calls in seconds',
  labelNames: ['method'],
  registers: [metricsRegistry],
});

export function startMetricsServer(port: number = 9090) {
  const app = express();

  app.get('/metrics', async (req, res) => {
    try {
      res.set('Content-Type', metricsRegistry.contentType);
      res.end(await metricsRegistry.metrics());
    } catch (ex) {
      res.status(500).end(ex);
    }
  });

  app.listen(port, () => {
    console.log(`Metrics server listening on port ${port}`);
  });
}
