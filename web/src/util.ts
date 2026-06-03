// Escapes HTML for safe insertion, then wraps case-insensitive matches of
// `term` in <mark>. Used to highlight search hits.
export function highlight(text: string, term: string): string {
  const escaped = escapeHtml(text)
  if (!term) return escaped
  const safeTerm = escapeRegExp(escapeHtml(term))
  return escaped.replace(new RegExp(safeTerm, 'gi'), (m) => `<mark>${m}</mark>`)
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function escapeRegExp(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/** Format an ISO date/datetime string to `YYYY-MM-DD`, or an em dash if absent. */
export function shortDate(iso: string | null | undefined): string {
  return iso ? iso.slice(0, 10) : '—'
}
