import type { Status } from '@/api'

// The status fill colors live as CSS custom properties in `style.css` (the
// single source of truth). This returns the `var(...)` reference for a status,
// applied via an inline `style` binding — CSS context, so `var()` resolves
// (an SVG `fill`/`stroke` *attribute* would not resolve a custom property).
export function statusColor(status: Status): string {
  return `var(--ad-status-${status.toLowerCase()})`
}
