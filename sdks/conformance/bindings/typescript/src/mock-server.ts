import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';

import { splitOn } from './case-loader.js';
import type { ScriptEntry } from './case-spec.js';

export interface RecordedRequest {
  method: string;
  path: string;
  headers: Record<string, string>;
  body: string;
}

interface RuleGroup {
  method: string;
  path: string;
  rules: ScriptEntry[];
  hits: number;
}

/**
 * Scripted HTTP server for one conformance case. Rules are grouped by
 * (method, path). For each incoming request the group's counter increments
 * and the rule with matching `match_count` is selected (or the fallthrough
 * rule with no `match_count`).
 *
 * Every request is recorded in {@link recorded} for post-hoc assertions.
 */
export class MockServer {
  readonly #server: Server;
  readonly #groups = new Map<string, RuleGroup>();
  readonly recorded: RecordedRequest[] = [];
  #baseUrl = '';

  constructor(script: ScriptEntry[]) {
    for (const entry of script) {
      const { method, path } = splitOn(entry.on);
      const key = `${method} ${path}`;
      const existing = this.#groups.get(key);
      if (existing) {
        existing.rules.push(entry);
      } else {
        this.#groups.set(key, { method, path, rules: [entry], hits: 0 });
      }
    }

    this.#server = createServer((req, res) => {
      this.#handleRequest(req, res).catch((err) => {
        // eslint-disable-next-line no-console
        console.error('mock server handler error', err);
        if (!res.headersSent) res.writeHead(500);
        res.end();
      });
    });
  }

  async start(): Promise<string> {
    await new Promise<void>((resolve) => this.#server.listen(0, '127.0.0.1', resolve));
    const addr = this.#server.address() as AddressInfo;
    this.#baseUrl = `http://127.0.0.1:${addr.port}`;
    return this.#baseUrl;
  }

  async stop(): Promise<void> {
    await new Promise<void>((resolve, reject) => {
      this.#server.close((err) => (err ? reject(err) : resolve()));
    });
  }

  get baseUrl(): string {
    return this.#baseUrl;
  }

  async #handleRequest(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const method = (req.method ?? 'GET').toUpperCase();
    const path = req.url ?? '/';
    const body = await readBody(req);
    const headers: Record<string, string> = {};
    for (const [k, v] of Object.entries(req.headers)) {
      if (v == null) continue;
      headers[k.toLowerCase()] = Array.isArray(v) ? v.join(',') : String(v);
    }
    this.recorded.push({ method, path, headers, body });

    const group = this.#groups.get(`${method} ${path}`);
    if (!group) {
      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: `no rule for ${method} ${path}` }));
      return;
    }

    group.hits += 1;
    const rule =
      group.rules.find((r) => r.match_count === group.hits) ??
      group.rules.find((r) => r.match_count === undefined);

    if (!rule) {
      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: `no rule for hit ${group.hits} on ${method} ${path}` }));
      return;
    }

    if (rule.respond.delay_ms && rule.respond.delay_ms > 0) {
      await new Promise((r) => setTimeout(r, rule.respond.delay_ms));
    }

    const resHeaders: Record<string, string> = { ...(rule.respond.headers ?? {}) };
    const respondBody = rule.respond.body;
    if (respondBody !== undefined && respondBody !== null) {
      const payload = typeof respondBody === 'string' ? respondBody : JSON.stringify(respondBody);
      resHeaders['content-type'] ??= 'application/json';
      res.writeHead(rule.respond.status, resHeaders);
      res.end(payload);
    } else {
      res.writeHead(rule.respond.status, resHeaders);
      res.end();
    }
  }
}

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on('data', (chunk: Buffer) => chunks.push(chunk));
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}
