// Tracks the dashboard's active ADR directory and exposes a switch action.
// Module-level refs so every consumer (header chip, directory picker) shares
// one piece of state.

import { onMounted, ref } from 'vue'
import { getWorkspace, switchWorkspace } from '@/api'

const dir = ref<string>('')
let started = false

async function refresh() {
  try {
    const w = await getWorkspace()
    dir.value = w.dir
  } catch {
    // Leave dir empty; the header chip simply won't render.
  }
}

export function useWorkspace() {
  onMounted(() => {
    if (started) return
    started = true
    refresh()
  })
  return {
    dir,
    refresh,
    /** Switch the active directory; updates the shared `dir` on success. */
    async switchTo(path: string): Promise<void> {
      const r = await switchWorkspace(path)
      dir.value = r.dir
    },
  }
}

/** Last path segment of a directory, for compact display. */
export function dirBasename(path: string): string {
  const trimmed = path.replace(/[/\\]+$/, '')
  const parts = trimmed.split(/[/\\]/)
  return parts[parts.length - 1] || trimmed
}
