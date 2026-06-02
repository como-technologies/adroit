// Dark-mode composable. Persists the user choice in localStorage, defaults to
// the system preference, and applies a `.dark` class on <html> for Tailwind's
// class-based dark: variant.

import { onMounted, ref, watch } from 'vue'

export type ThemeMode = 'system' | 'light' | 'dark'

const STORAGE_KEY = 'adroit-theme'

const mode = ref<ThemeMode>('system')
const resolved = ref<'light' | 'dark'>('light')

function systemPrefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    !!window.matchMedia &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
  )
}

function applyTheme() {
  const effective: 'light' | 'dark' =
    mode.value === 'system' ? (systemPrefersDark() ? 'dark' : 'light') : mode.value
  resolved.value = effective
  if (typeof document !== 'undefined') {
    document.documentElement.classList.toggle('dark', effective === 'dark')
  }
}

let initialized = false
function init() {
  if (initialized || typeof window === 'undefined') return
  initialized = true
  const stored = window.localStorage.getItem(STORAGE_KEY) as ThemeMode | null
  if (stored === 'light' || stored === 'dark' || stored === 'system') {
    mode.value = stored
  }
  applyTheme()
  // Track system preference changes while in `system` mode.
  const mq = window.matchMedia('(prefers-color-scheme: dark)')
  mq.addEventListener?.('change', () => {
    if (mode.value === 'system') applyTheme()
  })
}

watch(mode, () => {
  applyTheme()
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(STORAGE_KEY, mode.value)
  }
})

export function useTheme() {
  onMounted(init)
  return {
    mode,
    resolved,
    /** Tri-state cycle: system → light → dark → system. */
    cycle() {
      mode.value =
        mode.value === 'system' ? 'light' : mode.value === 'light' ? 'dark' : 'system'
    },
  }
}
