import { describe, expect, it } from 'vitest';

import { trimTrailingSlashes } from '../src/url.js';

describe('trimTrailingSlashes', () => {
  it('returns the input unchanged when no trailing slash', () => {
    expect(trimTrailingSlashes('http://localhost:4000')).toBe('http://localhost:4000');
    expect(trimTrailingSlashes('')).toBe('');
    expect(trimTrailingSlashes('x')).toBe('x');
  });

  it('strips a single trailing slash', () => {
    expect(trimTrailingSlashes('http://x:4000/')).toBe('http://x:4000');
  });

  it('strips many trailing slashes', () => {
    expect(trimTrailingSlashes('http://x:4000/////')).toBe('http://x:4000');
  });

  it('only strips at the end (interior slashes preserved)', () => {
    expect(trimTrailingSlashes('http://x/api/')).toBe('http://x/api');
  });

  it('handles all-slashes input', () => {
    expect(trimTrailingSlashes('///')).toBe('');
  });
});
