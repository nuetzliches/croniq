import { describe, expect, it } from 'vitest';

import { HttpError } from '../src/client.js';
import { CroniqTriggerClient, QueueOverflowError, createTriggerClient } from '../src/trigger.js';

interface CapturedCall {
  url: string;
  init: RequestInit;
}

interface StubResponse {
  status: number;
  body?: string;
  headers?: Record<string, string>;
}

function stubFetch(responder: (call: CapturedCall) => StubResponse): {
  fetchImpl: typeof fetch;
  calls: CapturedCall[];
} {
  const calls: CapturedCall[] = [];
  const fetchImpl = (async (input: Parameters<typeof fetch>[0], init?: RequestInit): Promise<Response> => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const call: CapturedCall = { url, init: init ?? {} };
    calls.push(call);
    const r = responder(call);
    return new Response(r.body ?? '', { status: r.status, headers: r.headers });
  }) as typeof fetch;
  return { fetchImpl, calls };
}

function bodyOf(call: CapturedCall): Record<string, unknown> {
  return JSON.parse(call.init.body as string) as Record<string, unknown>;
}

const OK = (body: string): StubResponse => ({ status: 200, body });

describe('CroniqTriggerClient.trigger', () => {
  it('posts a snake_case body to POST /v1/trigger with every field', async () => {
    const { fetchImpl, calls } = stubFetch(() => OK('{"execution_id":"exec-1","queued":3}'));
    const client = createTriggerClient({
      serverUrl: 'http://example.test:4000',
      apiKey: 'croniq_trigger_key',
      fetchImpl,
    });

    const result = await client.trigger('billing:invoice-generate', {
      metadata: { invoice_id: 'inv_42' },
      require: ['billing'],
      prefer: ['eu-central'],
      timeout: '10m',
      idempotencyKey: 'evt-123',
    });

    expect(calls).toHaveLength(1);
    expect(calls[0]!.url).toBe('http://example.test:4000/v1/trigger');
    expect(calls[0]!.init.method).toBe('POST');

    const body = bodyOf(calls[0]!);
    expect(body.job_key).toBe('billing:invoice-generate');
    expect(body.metadata).toEqual({ invoice_id: 'inv_42' });
    expect(body.require).toEqual(['billing']);
    expect(body.prefer).toEqual(['eu-central']);
    expect(body.timeout).toBe('10m');
    expect(body.idempotency_key).toBe('evt-123');

    expect(result).toEqual({ executionId: 'exec-1', queued: 3, deduplicated: false });
  });

  it('omits unset optional fields from the wire body', async () => {
    const { fetchImpl, calls } = stubFetch(() => OK('{"execution_id":"exec-1","queued":1}'));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000', fetchImpl });

    await client.trigger('etl:data-sync');

    const body = bodyOf(calls[0]!);
    expect(body).toEqual({ job_key: 'etl:data-sync' });
    for (const key of ['metadata', 'require', 'prefer', 'timeout', 'idempotency_key']) {
      expect(key in body).toBe(false);
    }
  });

  it('forwards nested/typed metadata verbatim as a JSON object', async () => {
    const { fetchImpl, calls } = stubFetch(() => OK('{"execution_id":"exec-3","queued":1}'));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000', fetchImpl });

    await client.trigger('email:send', {
      metadata: { user_id: 'u-42', attempt: 2, flags: { urgent: true } },
    });

    expect(bodyOf(calls[0]!).metadata).toEqual({ user_id: 'u-42', attempt: 2, flags: { urgent: true } });
  });

  it('defaults a missing deduplicated flag to false (older server)', async () => {
    const { fetchImpl } = stubFetch(() => OK('{"execution_id":"exec-1","queued":0}'));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000', fetchImpl });

    const result = await client.trigger('etl:data-sync');

    expect(result.deduplicated).toBe(false);
  });

  it('surfaces deduplicated: true', async () => {
    const { fetchImpl } = stubFetch(() => OK('{"execution_id":"exec-1","queued":0,"deduplicated":true}'));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000', fetchImpl });

    const result = await client.trigger('etl:data-sync', { idempotencyKey: 'evt-1' });

    expect(result.deduplicated).toBe(true);
    expect(result.executionId).toBe('exec-1');
  });

  it('throws HttpError on a non-2xx response', async () => {
    const { fetchImpl } = stubFetch(() => ({ status: 404, body: '{"error":"unknown job"}' }));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000', fetchImpl });

    await expect(client.trigger('nope:missing')).rejects.toBeInstanceOf(HttpError);
  });

  it('throws QueueOverflowError on 429 and parses Retry-After', async () => {
    const { fetchImpl } = stubFetch(() => ({
      status: 429,
      body: '{"execution_id":"","queued":0,"deduplicated":false}',
      headers: { 'retry-after': '5' },
    }));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000', fetchImpl });

    const err = await client.trigger('billing:invoice').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(QueueOverflowError);
    expect(err).toBeInstanceOf(HttpError);
    expect((err as QueueOverflowError).status).toBe(429);
    expect((err as QueueOverflowError).retryAfterMs).toBe(5000);
  });

  it('leaves retryAfterMs undefined when the 429 carries no Retry-After', async () => {
    const { fetchImpl } = stubFetch(() => ({ status: 429, body: '{}' }));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000', fetchImpl });

    const err = await client.trigger('billing:invoice').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(QueueOverflowError);
    expect((err as QueueOverflowError).retryAfterMs).toBeUndefined();
  });

  it('rejects a blank job key without hitting the network', async () => {
    const { fetchImpl, calls } = stubFetch(() => OK('{"execution_id":"x","queued":0}'));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000', fetchImpl });

    await expect(client.trigger('   ')).rejects.toBeInstanceOf(TypeError);
    expect(calls).toHaveLength(0);
  });

  it('sends Authorization: ApiKey when an api key is set (precedence over bearer)', async () => {
    const { fetchImpl, calls } = stubFetch(() => OK('{"execution_id":"exec-1","queued":0}'));
    const client = createTriggerClient({
      serverUrl: 'http://example.test:4000',
      apiKey: 'key-abc',
      bearerToken: 'tok-xyz',
      fetchImpl,
    });

    await client.trigger('billing:invoice');

    const headers = new Headers(calls[0]!.init.headers);
    expect(headers.get('authorization')).toBe('ApiKey key-abc');
  });

  it('falls back to Bearer when only a bearer token is set', async () => {
    const { fetchImpl, calls } = stubFetch(() => OK('{"execution_id":"exec-1","queued":0}'));
    const client = createTriggerClient({
      serverUrl: 'http://example.test:4000',
      bearerToken: 'tok-xyz',
      fetchImpl,
    });

    await client.trigger('billing:invoice');

    const headers = new Headers(calls[0]!.init.headers);
    expect(headers.get('authorization')).toBe('Bearer tok-xyz');
  });

  it('trims trailing slashes from serverUrl', async () => {
    const { fetchImpl, calls } = stubFetch(() => OK('{"execution_id":"exec-1","queued":0}'));
    const client = createTriggerClient({ serverUrl: 'http://example.test:4000///', fetchImpl });

    await client.trigger('billing:invoice');

    expect(calls[0]!.url).toBe('http://example.test:4000/v1/trigger');
  });

  it('rejects a missing or invalid serverUrl at construction', () => {
    expect(() => new CroniqTriggerClient({ serverUrl: '' })).toThrow(TypeError);
    expect(() => new CroniqTriggerClient({ serverUrl: 'not a url' })).toThrow(TypeError);
  });
});
