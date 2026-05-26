// Small URL utilities. Kept regex-free on purpose: CodeQL's "polynomial
// regex" heuristic flags even linear patterns like `/\/+$/` when the
// input is library-controlled, and the loop equivalent is both clearer
// and immune to the warning.

/**
 * Strips trailing forward slashes from a string. Linear in the length of
 * the suffix that's stripped.
 *
 *   trimTrailingSlashes("http://x")    === "http://x"
 *   trimTrailingSlashes("http://x/")   === "http://x"
 *   trimTrailingSlashes("http://x///") === "http://x"
 */
export function trimTrailingSlashes(value: string): string {
  let end = value.length;
  while (end > 0 && value.charCodeAt(end - 1) === 47 /* '/' */) {
    end--;
  }
  return end === value.length ? value : value.slice(0, end);
}
