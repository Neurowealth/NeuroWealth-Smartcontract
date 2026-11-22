import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

export interface RpcRequestRecord {
  scenario: string;
  method: string;
  params: Record<string, unknown>;
}

export interface LocalRpcServer {
  url: string;
  requests: RpcRequestRecord[];
  /**
   * Simulates the agent submitting an on-chain rebalance for a scenario
   * (#934). The agent calls Soroban directly, never through the browser
   * wallet, so tests drive this the same way instead of going through the
   * page.
   */
  rebalance(scenario: string, params: Record<string, unknown>): Promise<void>;
  close(): Promise<void>;
}

interface ContractState {
  strategy: string;
  exchangeRate: number;
  totalAssets: number;
  totalShares: number;
}

const DEFAULT_CONTRACT_STATE: ContractState = {
  strategy: 'balanced',
  exchangeRate: 12_500_000,
  totalAssets: 1_250_000_000,
  totalShares: 1_000_000_000,
};

export async function startLocalRpcServer(): Promise<LocalRpcServer> {
  const requests: RpcRequestRecord[] = [];
  // Per-scenario mutable contract state, so a scenario can simulate the
  // agent submitting an on-chain rebalance between a deposit and a
  // withdrawal (#934) without affecting other scenarios/tests.
  const contractState = new Map<string, ContractState>();
  const stateFor = (scenario: string): ContractState => {
    let state = contractState.get(scenario);
    if (!state) {
      state = { ...DEFAULT_CONTRACT_STATE };
      contractState.set(scenario, state);
    }
    return state;
  };

  const server: Server = createServer((request, response) => {
    response.setHeader('Access-Control-Allow-Origin', '*');
    response.setHeader('Access-Control-Allow-Headers', 'content-type');
    response.setHeader('Access-Control-Allow-Methods', 'POST, OPTIONS');
    if (request.method === 'OPTIONS') {
      response.writeHead(204).end();
      return;
    }

    let body = '';
    request.setEncoding('utf8');
    request.on('data', (chunk: string) => { body += chunk; });
    request.on('end', () => {
      void (async () => {
        const payload = JSON.parse(body) as RpcRequestRecord;
        requests.push(payload);
        const { method, params, scenario } = payload;
        let result: unknown;

        if (method === 'connectWallet') result = 'GABCDE123456789WXYZ';
        else if (method === 'getNetworkDetails') {
          result = { networkPassphrase: scenario === 'wrong-network' ? 'Wrong Network' : 'Test SDF Network ; September 2015' };
        } else if (method === 'signTransaction' && scenario === 'rejected') {
          response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ error: 'User rejected signature' }));
          return;
        } else if (method === 'simulateContractCall') {
          const state = stateFor(scenario);
          const values: Record<string, unknown> = {
            get_balance: 1_000_000_000,
            get_user_strategy: state.strategy,
            get_exchange_rate: state.exchangeRate,
            get_total_assets: state.totalAssets,
            get_total_shares: state.totalShares,
          };
          result = values[String(params.method)];
        } else if (method === 'rebalance') {
          // Simulates the agent submitting an on-chain rebalance directly
          // against the sandbox RPC (the agent never goes through the
          // browser wallet), updating the state subsequent contract reads
          // observe for this scenario.
          const state = stateFor(scenario);
          if (typeof params.strategy === 'string') state.strategy = params.strategy;
          if (typeof params.totalAssets === 'number') state.totalAssets = params.totalAssets;
          if (typeof params.totalShares === 'number') state.totalShares = params.totalShares;
          result = { status: 'SUCCESS' };
        } else if (method === 'simulateTransaction' && scenario === 'simulation-failed') {
          response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ error: 'Transaction simulation failed' }));
          return;
        } else if (method === 'simulateTransaction') result = { xdr: 'local-test-xdr' };
        else if (method === 'signTransaction') result = `signed:${String(params.xdr)}`;
        else if (method === 'sendTransaction') result = { status: 'PENDING', hash: 'local-test-tx-hash-0001' };
        else if (method === 'getTransaction') {
          if (scenario !== 'pending') await new Promise((resolve) => setTimeout(resolve, 500));
          result = { status: scenario === 'pending' ? 'NOT_FOUND' : 'SUCCESS' };
        } else result = {};

        response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ result }));
      })().catch((error: unknown) => {
        response.writeHead(500, { 'content-type': 'application/json' }).end(JSON.stringify({ error: String(error) }));
      });
    });
  });

  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address() as AddressInfo;
  const url = `http://127.0.0.1:${address.port}/rpc`;
  return {
    url,
    requests,
    rebalance: async (scenario, params) => {
      const response = await fetch(url, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ method: 'rebalance', params, scenario }),
      });
      const data = await response.json() as { error?: string };
      if (data.error) throw new Error(data.error);
    },
    close: () => new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve())),
  };
}