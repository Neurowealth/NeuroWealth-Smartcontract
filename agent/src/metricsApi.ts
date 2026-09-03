/**
 * HTTP API for agent performance metrics (Issue #652).
 *
 * Exposes the [`MetricsEngine`] snapshot to the dashboard UI:
 *
 *   - `GET /api/metrics/snapshot` — full aggregation payload
 *   - `GET /api/metrics/alerts`   — currently firing threshold alerts
 *   - `GET /health`               — liveness/heartbeat endpoint (also used by
 *     the disaster-recovery failover monitor, issue #655)
 *
 * The server binds to `METRICS_PORT` (default 9100) when started via
 * [`startMetricsServer`]; tests can pass port 0 for an ephemeral port.
 */

import express from 'express';
import type { Request, Response } from 'express';
import type { Server } from 'node:http';
import { MetricsEngine, MetricsSnapshot, MetricAlert } from './metrics';
import logger from './logger';

export function createMetricsApp(engine: MetricsEngine): express.Express {
  const app = express();
  app.disable('x-powered-by');

  // Liveness endpoint: agent health + last heartbeat info.
  app.get('/health', (_req: Request, res: Response) => {
    const snapshot = engine.getSnapshot();
    res.status(snapshot.agentHealthy ? 200 : 503).json({
      status: snapshot.agentHealthy ? 'healthy' : 'degraded',
      agentHealthy: snapshot.agentHealthy,
      lastHeartbeatAt: snapshot.lastHeartbeatAt,
      uptimePct: snapshot.uptimePct,
    });
  });

  // Full metrics snapshot for the dashboard.
  app.get('/api/metrics/snapshot', (_req: Request, res: Response) => {
    const snapshot: MetricsSnapshot = engine.getSnapshot();
    res.json(snapshot);
  });

  // Threshold alerts only (pollable by alerters/pagers).
  app.get('/api/metrics/alerts', (_req: Request, res: Response) => {
    const alerts: MetricAlert[] = engine.checkAlerts();
    res.json({ generatedAt: Date.now(), alerts });
  });

  return app;
}

/** Starts the metrics HTTP server; resolves once listening. */
export function startMetricsServer(engine: MetricsEngine, port = Number(process.env.METRICS_PORT) || 9100): Promise<Server> {
  return new Promise((resolve, reject) => {
    const app = createMetricsApp(engine);
    const server = app.listen(port, () => {
      const address = server.address();
      const actualPort = typeof address === 'object' && address ? address.port : port;
      logger.info({ port: actualPort }, 'Metrics server listening');
      resolve(server);
    });
    server.on('error', reject);
  });
}
