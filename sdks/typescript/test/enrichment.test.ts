import { describe, expect, it } from 'vitest';

import { LogEnrichment } from '../src/enrichment.js';

describe('LogEnrichment', () => {
  it('injects job_key and runner_id into fields', () => {
    const e = new LogEnrichment('billing:invoice', 'runner-abc', []);
    const out = e.enrich({ message: 'hello' });
    expect(out.fields).toEqual({ job_key: 'billing:invoice', runner_id: 'runner-abc' });
    expect(out.message).toBe('hello');
  });

  it('preserves the original event level', () => {
    const e = new LogEnrichment('j', 'r', []);
    expect(e.enrich({ level: 'warn', message: 'hi' }).level).toBe('warn');
  });

  it('serializes tags as a JSON array string', () => {
    const e = new LogEnrichment('j', 'r', ['env=prod', 'lang=ts']);
    const out = e.enrich({ message: 'x' });
    expect(out.fields?.runner_tags).toBe('["env=prod","lang=ts"]');
  });

  it('omits runner_tags when tags is empty', () => {
    const e = new LogEnrichment('j', 'r', []);
    const out = e.enrich({ message: 'x' });
    expect(out.fields).not.toHaveProperty('runner_tags');
  });

  it('caller-provided keys win', () => {
    const e = new LogEnrichment('billing:invoice', 'runner-abc', []);
    const out = e.enrich({
      message: 'x',
      fields: { job_key: 'overridden', custom: 'value' },
    });
    expect(out.fields?.job_key).toBe('overridden');
    expect(out.fields?.runner_id).toBe('runner-abc');
    expect(out.fields?.custom).toBe('value');
  });

  it('does not mutate the source event', () => {
    const e = new LogEnrichment('j', 'r', []);
    const source = { message: 'x', fields: { a: 'b' } };
    e.enrich(source);
    expect(source.fields).toEqual({ a: 'b' });
  });
});
