import { describe, expect, it, vi } from 'vitest';

import { consoleLogger } from '../src/logger.js';
import {
  escapeControlChars,
  isSafeExecutionId,
  isSafeJobKey,
  MAX_EXECUTION_ID_LENGTH,
  MAX_JOB_KEY_LENGTH,
  previewForLog,
  rejectAssignmentReason,
  rejectionAckError,
} from '../src/sanitize.js';

/** Literal ESC — spelled via an escape so this file stays plain ASCII. */
const ESC = '\u001b';
const CRLF_KEY = 'billing:invoice\r\n2026-01-01 ERROR forged record';
const ANSI_KEY = `billing:${ESC}[31minvoice${ESC}[0m`;

describe('isSafeJobKey', () => {
  it('accepts every key the Croniqfile lexer can produce unquoted', () => {
    for (const key of [
      'billing:invoice',
      'ops:health:eu-west',
      'ops:db-dump',
      'a:b',
      'ns:name.with.dots',
      'ns:name_with_underscore',
      'ns:path/segment',
      'ns:*',
      'ns:name+variant@host',
      'ns:what?',
    ]) {
      expect(isSafeJobKey(key), key).toBe(true);
    }
  });

  it('accepts keys only a *quoted* DSL key or the HTTP API can produce', () => {
    // `job "billing:monthly invoice" { … }` is legal DSL: parse_job_key accepts
    // a QuotedString and enforces only the colon-part count. An allowlist would
    // strand these valid configurations, so interior spaces and non-ASCII text
    // must pass.
    for (const key of [
      'billing:monthly invoice',
      'ops:health check:eu-west',
      'berichte:monatsabschluss (märz)',
      'ops:1С-выгрузка',
      'ops:日次バッチ',
      'ops:deploy#42',
      'ops:a,b;c',
      'ops:100%-check',
      'ops:emoji-🚀',
    ]) {
      expect(isSafeJobKey(key), key).toBe(true);
    }
  });

  it('rejects CRLF, ESC, NUL, DEL and C1 — and nothing else printable', () => {
    expect(isSafeJobKey(CRLF_KEY)).toBe(false);
    expect(isSafeJobKey(ANSI_KEY)).toBe(false);
    expect(isSafeJobKey('billing:in\u0000voice')).toBe(false);
    expect(isSafeJobKey('billing:in\tvoice')).toBe(false);
    expect(isSafeJobKey('billing:invoice\u007f')).toBe(false);
    expect(isSafeJobKey('billing:invoice\u009b')).toBe(false);
    // A trailing or interior space is legal DSL and harmless — it cannot
    // forge a record, so an allowlist that excluded it would be wrong.
    expect(isSafeJobKey('billing:invoice ')).toBe(true);
    expect(isSafeJobKey('billing: invoice')).toBe(true);
  });

  it('rejects the empty string, over-long keys and non-strings', () => {
    expect(isSafeJobKey('')).toBe(false);
    expect(isSafeJobKey('a'.repeat(MAX_JOB_KEY_LENGTH))).toBe(true);
    expect(isSafeJobKey('a'.repeat(MAX_JOB_KEY_LENGTH + 1))).toBe(false);
    expect(isSafeJobKey(undefined)).toBe(false);
    expect(isSafeJobKey(42)).toBe(false);
  });

  it('bounds length by scalar values, not UTF-16 code units', () => {
    // Each astral character is two UTF-16 units but one scalar value, so a key
    // of MAX astral characters must pass rather than be rejected at half length.
    expect(isSafeJobKey('🚀'.repeat(MAX_JOB_KEY_LENGTH))).toBe(true);
    expect(isSafeJobKey('🚀'.repeat(MAX_JOB_KEY_LENGTH + 1))).toBe(false);
  });
});

describe('isSafeExecutionId', () => {
  it('accepts a v4 UUID and the opaque ids the conformance suite uses', () => {
    expect(isSafeExecutionId('6f8c1a2e-4b7d-4a1f-9c3e-2d5b8a0f1e77')).toBe(true);
    expect(isSafeExecutionId('exec-001')).toBe(true);
  });

  it('rejects CRLF, ANSI escapes and over-long ids', () => {
    expect(isSafeExecutionId('exec-001\r\nforged')).toBe(false);
    expect(isSafeExecutionId(`exec${ESC}[2J001`)).toBe(false);
    expect(isSafeExecutionId('a'.repeat(MAX_EXECUTION_ID_LENGTH + 1))).toBe(false);
    expect(isSafeExecutionId('')).toBe(false);
  });
});

describe('rejectAssignmentReason', () => {
  it('names the offending field, and passes a legitimate pair', () => {
    expect(rejectAssignmentReason('exec-001', 'billing:invoice')).toBeUndefined();
    expect(rejectAssignmentReason('exec-001', 'billing:monthly invoice')).toBeUndefined();
    expect(rejectAssignmentReason('exec-001', CRLF_KEY)).toBe('job_key');
    expect(rejectAssignmentReason('exec\r\n001', 'billing:invoice')).toBe('execution_id');
    // execution_id is checked first: it is what addresses the server, so when
    // both are bad the assignment is unackable and must be dropped.
    expect(rejectAssignmentReason('exec\r\n001', CRLF_KEY)).toBe('execution_id');
  });
});

describe('rejectionAckError', () => {
  it('names the field and carries the value escaped', () => {
    const message = rejectionAckError('job_key', CRLF_KEY);
    expect(message).toContain('job_key');
    expect(message).not.toContain('\r');
    expect(message).not.toContain('\n');
    expect(message).toContain('\\r\\n');
  });
});

describe('escapeControlChars', () => {
  it('escapes CR, LF, TAB, ESC and the C1 range', () => {
    expect(escapeControlChars('a\r\nb')).toBe('a\\r\\nb');
    expect(escapeControlChars('a\tb')).toBe('a\\tb');
    expect(escapeControlChars(`${ESC}[31mred`)).toBe('\\u001b[31mred');
    expect(escapeControlChars('\u009b')).toBe('\\u009b');
  });

  it('leaves printable text — including non-ASCII — untouched', () => {
    expect(escapeControlChars('billing:invoice — läuft')).toBe(
      'billing:invoice — läuft',
    );
  });
});

describe('previewForLog', () => {
  it('escapes and truncates', () => {
    expect(previewForLog(CRLF_KEY)).not.toContain('\n');
    expect(previewForLog('a'.repeat(500)).length).toBeLessThanOrEqual(121);
  });
});

describe('consoleLogger', () => {
  it('escapes control characters in the message before writing to the console', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      consoleLogger('warn').warn(`handler threw: ${ESC}[31mboom\r\nFAKE ERROR`);
      expect(spy).toHaveBeenCalledTimes(1);
      const line = spy.mock.calls[0]![0] as string;
      expect(line).not.toContain(ESC);
      expect(line).not.toContain('\n');
      expect(line).toContain('\\u001b');
      expect(line).toContain('\\r\\n');
    } finally {
      spy.mockRestore();
    }
  });

  it('renders fields as JSON, which already escapes control characters', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      consoleLogger('warn').warn('job handler threw', { job_key: ANSI_KEY });
      const line = spy.mock.calls[0]![0] as string;
      expect(line).not.toContain(ESC);
      expect(line).toContain('\\u001b');
    } finally {
      spy.mockRestore();
    }
  });
});
