/**
 * What the search box on Runs and on Jobs asks for.
 *
 * Unquoted, the box searches: a case-insensitive substring of the job key
 * (on Jobs, of the description as well).
 * Quoted — `"search:journal-sync"` — it names one job, exactly as
 * `?job_key=` does. A substring of `search:journal-sync` also answers for
 * `search:journal-sync-full`, and typing the whole key is no way out of that;
 * the quotes are, the way they are in most search boxes.
 *
 * Straight quotes and the typographic pairs (“…”, „…“) both count, since a key
 * pasted from a document or a chat often arrives with the latter.
 */
export interface JobKeySearch {
  /** Exact job key, from a quoted search. */
  exact?: string
  /** Substring of the job key, from an unquoted one. */
  contains?: string
}

const QUOTE_PAIRS: ReadonlyArray<readonly [string, string]> = [
  ['"', '"'],
  ['“', '”'],
  ['„', '“'],
]

export function parseJobKeySearch(raw: string): JobKeySearch {
  const text = raw.trim()
  for (const [open, close] of QUOTE_PAIRS) {
    if (text.length >= 2 && text.startsWith(open) && text.endsWith(close)) {
      const exact = text.slice(open.length, -close.length).trim()
      // `""` is a search not yet typed, not a job whose key is empty.
      return exact ? { exact } : {}
    }
  }
  // Halfway through typing `"mail:send"` the box holds `"mail:se`. Searching
  // for that, quote included, matches nothing and blanks the list a keystroke
  // before the closing quote brings it back; the text after the quote is what
  // the person is narrowing towards.
  const opener = QUOTE_PAIRS.find(([open]) => text.startsWith(open))
  const contains = (opener ? text.slice(opener[0].length) : text).trim()
  return contains ? { contains } : {}
}
