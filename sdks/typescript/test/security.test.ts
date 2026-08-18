// Base-URL transport-security checks (issue #440).
//
// `https://` is always accepted; `http://` only for a loopback host (the
// `http://localhost:4000` quickstart path) or behind an explicit
// `allowInsecureHttp: true`, which additionally emits one loud warning.
// Enforced at construction time for both the runner options and the producer.

import { describe, expect, it, vi } from 'vitest';

import { noopLogger } from '../src/logger.js';
import type { Logger } from '../src/logger.js';
import { resolveOptions } from '../src/options.js';
import { isLoopbackHostname } from '../src/security.js';
import { createTriggerClient } from '../src/trigger.js';

const ACCEPTED = [
  'https://croniq.example.com',
  'https://croniq.example.com:4000',
  'http://localhost:4000',
  'http://LOCALHOST:4000',
  'http://127.0.0.1:4000',
  'http://127.10.20.30:4000',
  'http://[::1]:4000',
];

const REJECTED = [
  'http://croniq.example.com',
  'http://croniq.example.com:4000',
  'http://10.0.0.5:4000',
  'http://[2001:db8::1]:4000',
];

function recordingLogger(): { logger: Logger; warnings: string[] } {
  const warnings: string[] = [];
  return {
    warnings,
    logger: { ...noopLogger, warn: (message: string) => void warnings.push(message) },
  };
}

describe('serverUrl transport security', () => {
  it.each(ACCEPTED)('accepts %s in runner options', (serverUrl) => {
    expect(resolveOptions({ serverUrl }, noopLogger).serverUrl).toBe(serverUrl);
  });

  it.each(ACCEPTED)('accepts %s in the trigger client', (serverUrl) => {
    expect(() => createTriggerClient({ serverUrl, logger: noopLogger })).not.toThrow();
  });

  it.each(REJECTED)('rejects %s in runner options', (serverUrl) => {
    expect(() => resolveOptions({ serverUrl }, noopLogger)).toThrow(TypeError);
    // Actionable: names the URL and the opt-in flag.
    expect(() => resolveOptions({ serverUrl }, noopLogger)).toThrow(/allowInsecureHttp/);
    expect(() => resolveOptions({ serverUrl }, noopLogger)).toThrow(serverUrl);
  });

  it.each(REJECTED)('rejects %s in the trigger client', (serverUrl) => {
    expect(() => createTriggerClient({ serverUrl })).toThrow(/allowInsecureHttp/);
  });

  it('keeps the documented quickstart default working', () => {
    const { warnings, logger } = recordingLogger();
    expect(resolveOptions({ serverUrl: 'http://localhost:4000' }, logger).serverUrl).toBe(
      'http://localhost:4000',
    );
    expect(warnings).toEqual([]);
  });

  it('rejects an unsupported scheme', () => {
    expect(() => resolveOptions({ serverUrl: 'ftp://croniq.example.com' }, noopLogger)).toThrow(
      /unsupported scheme/,
    );
  });

  it('accepts a cleartext URL with the opt-in and warns exactly once', () => {
    const { warnings, logger } = recordingLogger();
    const resolved = resolveOptions(
      { serverUrl: 'http://croniq.example.com:4000', allowInsecureHttp: true, logger },
      noopLogger,
    );

    expect(resolved.serverUrl).toBe('http://croniq.example.com:4000');
    expect(resolved.allowInsecureHttp).toBe(true);
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain('SECURITY');
    expect(warnings[0]).toContain('cleartext');
    expect(warnings[0]).toContain('http://croniq.example.com:4000');
  });

  it('accepts a cleartext trigger URL with the opt-in and warns exactly once', () => {
    const { warnings, logger } = recordingLogger();
    createTriggerClient({
      serverUrl: 'http://croniq.example.com:4000',
      allowInsecureHttp: true,
      logger,
    });

    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain('SECURITY');
  });

  it('warns through the console when the trigger client has no logger', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      createTriggerClient({ serverUrl: 'http://croniq.example.com:4000', allowInsecureHttp: true });
      expect(spy).toHaveBeenCalledTimes(1);
      expect(String(spy.mock.calls[0]?.[0])).toContain('SECURITY');
    } finally {
      spy.mockRestore();
    }
  });
});

describe('isLoopbackHostname', () => {
  it.each([
    ['localhost', true],
    ['LocalHost', true],
    ['127.0.0.1', true],
    ['127.255.255.254', true],
    ['[::1]', true],
    ['::1', true],
    ['croniq.example.com', false],
    ['10.0.0.5', false],
    ['[2001:db8::1]', false],
    ['127.0.0', false],
    ['127.0.0.999', false],
    ['', false],
  ] as const)('classifies %s as %s', (host, expected) => {
    expect(isLoopbackHostname(host)).toBe(expected);
  });
});
