// Settings search: every word typed must appear somewhere in a setting's text (its
// name, description, category and extra keywords), in any order.

/** Lowercase words of a query, without punctuation. */
export function queryWords(query: string): string[] {
  return query
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
}

/** Whether all `words` appear in `texts` (as whole words or the start of one). */
export function matchesQuery(words: string[], texts: (string | undefined)[]): boolean {
  if (!words.length) return true
  const haystack = queryWords(texts.filter(Boolean).join(' '))
  return words.every((w) => haystack.some((h) => h.startsWith(w)))
}
