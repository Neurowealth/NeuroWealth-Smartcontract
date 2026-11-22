export type ExecutionMode = 'live' | 'dry-run';

export function resolveExecutionMode(env: NodeJS.ProcessEnv = process.env): ExecutionMode {
  if (env.DRY_RUN !== undefined || env.LIVE_MODE !== undefined) {
    throw new Error('Use AGENT_EXECUTION_MODE only; legacy DRY_RUN/LIVE_MODE flags are ambiguous');
  }

  if (env.AGENT_EXECUTION_MODE === 'live' || env.AGENT_EXECUTION_MODE === 'dry-run') {
    return env.AGENT_EXECUTION_MODE;
  }

  throw new Error('AGENT_EXECUTION_MODE must be explicitly set to "live" or "dry-run"');
}