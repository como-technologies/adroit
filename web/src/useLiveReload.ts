// Auto live-reload client for the read-only dashboard.
//
// Opens a single native `EventSource` to the server's `GET /api/events` SSE
// stream (wired in `src/serve/mod.rs`). The server emits an `event: change`
// whenever the watched ADR directory changes on disk (CLI/TUI/$EDITOR edits or
// git operations), coalesced server-side. Each view passes a callback that
// re-fetches its own data on a `change`.
//
// `EventSource` auto-reconnects if the stream drops, so we don't manage retries
// here. The composable tears the connection down on unmount to avoid leaks, and
// exposes a small reactive `connected`/`updatedAt` state for an optional
// "updated" indicator in the UI.

import { onMounted, onUnmounted, ref, type Ref } from 'vue'

export interface LiveReload {
  // True while the SSE connection is open.
  connected: Ref<boolean>
  // Timestamp (ms) of the last `change` event, or null if none yet. Views can
  // watch this to flash a subtle "updated" badge.
  updatedAt: Ref<number | null>
}

/**
 * Subscribe to live-reload change events. `onChange` runs once per coalesced
 * filesystem change; use it to re-fetch the current view's data.
 */
export function useLiveReload(onChange: () => void): LiveReload {
  const connected = ref(false)
  const updatedAt = ref<number | null>(null)
  let source: EventSource | null = null

  onMounted(() => {
    // Guard for non-browser/test environments without EventSource.
    if (typeof EventSource === 'undefined') return

    source = new EventSource('/api/events')
    source.onopen = () => {
      connected.value = true
    }
    source.onerror = () => {
      // The browser will auto-reconnect; just reflect the transient drop.
      connected.value = false
    }
    // The server names the event `change`; listen for that specifically (the
    // keep-alive comments are not delivered as events and are ignored).
    source.addEventListener('change', () => {
      updatedAt.value = Date.now()
      onChange()
    })
  })

  onUnmounted(() => {
    source?.close()
    source = null
  })

  return { connected, updatedAt }
}
