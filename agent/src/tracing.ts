/**
 * OpenTelemetry tracing setup for agent service
 * Provides distributed tracing for Stellar RPC, OpenAI API, and database queries
 */

import { trace } from '@opentelemetry/api';
import { BatchSpanProcessor, NodeTracerProvider } from '@opentelemetry/sdk-trace-node';
import { Resource } from '@opentelemetry/resources';
import { SemanticResourceAttributes } from '@opentelemetry/semantic-conventions';
import { JaegerExporter } from '@opentelemetry/exporter-jaeger';
import { HttpInstrumentation } from '@opentelemetry/instrumentation-http';
import { PgInstrumentation } from '@opentelemetry/instrumentation-pg';
import { registerInstrumentations } from '@opentelemetry/instrumentation';
import logger from './logger';

export function initializeTracing(serviceName: string = 'neurowealth-agent') {
  const resource = Resource.default().merge(
    new Resource({
      [SemanticResourceAttributes.SERVICE_NAME]: serviceName,
      [SemanticResourceAttributes.SERVICE_VERSION]: '1.0.0',
    })
  );

  const provider = new NodeTracerProvider({ resource });

  // Configure Jaeger exporter
  const jaegerExporter = new JaegerExporter({
    endpoint: process.env.JAEGER_ENDPOINT || 'http://localhost:4318/v1/traces',
  });

  provider.addSpanProcessor(new BatchSpanProcessor(jaegerExporter));
  provider.register();

  // Register instrumentations
  registerInstrumentations({
    instrumentations: [
      new HttpInstrumentation(),
      new PgInstrumentation(),
    ],
  });

  logger.info({ serviceName }, 'OpenTelemetry tracing initialized');
  return trace.getTracer(serviceName);
}

export const tracer = initializeTracing();
