// Subset matcher with one wildcard token (`"*"`). Used to assert request
// bodies without forcing tests to enumerate every SDK-emitted field.
//
//  * Literal scalars (string, number, bool) must match exactly.
//  * `"*"` matches any non-empty value of any JSON kind.
//  * Nested objects match recursively; extra keys on the actual side are
//    ignored.
//  * Arrays match length-and-order, element-by-element.
//  * `null` asserts the key is present and explicitly null.

export function matchBody(expected: unknown, actual: unknown, path = '$'): string | null {
  if (expected === null) {
    return actual === null ? null : `${path}: expected null but got ${describe(actual)}`;
  }
  if (typeof expected === 'string' && expected === '*') {
    if (actual === null || actual === undefined) {
      return `${path}: expected non-empty wildcard match but got ${describe(actual)}`;
    }
    if (typeof actual === 'string' && actual.length === 0) {
      return `${path}: expected non-empty string but got empty`;
    }
    return null;
  }

  if (Array.isArray(expected)) {
    if (!Array.isArray(actual)) return `${path}: expected array but got ${describe(actual)}`;
    if (actual.length !== expected.length) {
      return `${path}: expected ${expected.length} item(s) but got ${actual.length}`;
    }
    for (let i = 0; i < expected.length; i++) {
      const err = matchBody(expected[i], actual[i], `${path}[${i}]`);
      if (err) return err;
    }
    return null;
  }

  if (typeof expected === 'object') {
    if (actual === null || typeof actual !== 'object' || Array.isArray(actual)) {
      return `${path}: expected object but got ${describe(actual)}`;
    }
    const exp = expected as Record<string, unknown>;
    const act = actual as Record<string, unknown>;
    for (const [key, value] of Object.entries(exp)) {
      if (!(key in act)) return `${path}.${key}: missing key`;
      const err = matchBody(value, act[key], `${path}.${key}`);
      if (err) return err;
    }
    return null;
  }

  if (typeof expected === 'number') {
    if (typeof actual !== 'number') return `${path}: expected number but got ${describe(actual)}`;
    if (Math.abs(actual - expected) > 1e-9) {
      return `${path}: expected ${expected} but got ${actual}`;
    }
    return null;
  }

  if (typeof expected === 'boolean') {
    if (typeof actual !== 'boolean') return `${path}: expected boolean but got ${describe(actual)}`;
    return actual === expected ? null : `${path}: expected ${expected} but got ${actual}`;
  }

  if (typeof expected === 'string') {
    if (typeof actual !== 'string') return `${path}: expected string but got ${describe(actual)}`;
    return actual === expected ? null : `${path}: expected '${expected}' but got '${actual}'`;
  }

  return `${path}: unexpected expected type ${typeof expected}`;
}

function describe(value: unknown): string {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  return typeof value;
}
