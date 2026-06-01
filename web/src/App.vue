<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink, RouterView, useRoute, useRouter } from 'vue-router'
import { FolderOpen, Moon, MonitorSmartphone, Sun } from 'lucide-vue-next'
import { useLiveReload } from '@/useLiveReload'
import { useTheme } from '@/composables/useTheme'
import { dirBasename, useWorkspace } from '@/composables/useWorkspace'
import DirectoryPicker from '@/components/DirectoryPicker.vue'

const { mode, resolved, cycle } = useTheme()
const themeLabel = computed(() =>
  mode.value === 'system' ? 'System' : mode.value === 'dark' ? 'Dark' : 'Light',
)

const router = useRouter()
const route = useRoute()
const workspace = useWorkspace()
const dirName = computed(() => dirBasename(workspace.dir.value))
const pickerOpen = ref(false)

function onSwitched() {
  pickerOpen.value = false
  // Land on a guaranteed-valid view of the new workspace; open tabs also get an
  // SSE 'change' tick from the server and re-fetch their own data.
  if (route.path !== '/') router.push('/')
}

// App-level live-reload subscription drives a subtle "updated" flash and the
// live / read-only indicator; each view opens its own subscription to re-fetch
// its data on change.
const flash = ref(false)
let flashTimer: ReturnType<typeof setTimeout> | undefined
const live = useLiveReload(() => {})

watch(live.updatedAt, () => {
  flash.value = true
  clearTimeout(flashTimer)
  flashTimer = setTimeout(() => {
    flash.value = false
  }, 1600)
})

const navLinks = [
  { to: '/', label: 'Dashboard' },
  { to: '/browse', label: 'Browse' },
  { to: '/insights', label: 'Insights' },
]

const navClass =
  'text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100 transition-colors'
const navActiveClass = 'text-brand-700 dark:text-brand-300 font-semibold'
</script>

<template>
  <div class="flex min-h-screen flex-col">
    <header
      class="sticky top-0 z-20 border-b border-slate-200/70 bg-white/80 backdrop-blur-md dark:border-slate-800/70 dark:bg-slate-950/80"
    >
      <div class="mx-auto flex max-w-6xl items-center justify-between gap-4 px-6 py-3.5">
        <div class="flex min-w-0 items-center gap-2.5">
          <RouterLink to="/" class="group flex shrink-0 items-center gap-2.5">
            <span
              class="inline-flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-brand-500 to-violet-600 font-display text-lg font-bold text-white shadow-md shadow-brand-500/30 transition-transform group-hover:scale-105"
            >a</span>
            <span
              class="font-display text-lg font-semibold tracking-tight text-slate-900 dark:text-slate-100"
            >adroit</span>
          </RouterLink>

          <!-- Active ADR directory — click to switch workspaces. -->
          <button
            v-if="workspace.dir.value"
            type="button"
            class="hidden min-w-0 items-center gap-1.5 rounded-lg border border-slate-200 px-2.5 py-1 text-xs text-slate-600 transition-colors hover:border-brand-300 hover:text-slate-900 dark:border-slate-800 dark:text-slate-400 dark:hover:text-slate-100 sm:inline-flex"
            :title="`ADR directory: ${workspace.dir.value} — click to change`"
            @click="pickerOpen = true"
          >
            <FolderOpen :size="14" class="shrink-0 text-slate-400" />
            <span class="max-w-[24vw] truncate font-mono">{{ dirName }}</span>
          </button>
        </div>

        <nav class="flex items-center gap-1 sm:gap-2">
          <RouterLink
            v-for="link in navLinks"
            :key="link.to"
            :to="link.to"
            class="rounded-lg px-2.5 py-1.5 text-sm"
            :class="navClass"
            :active-class="navActiveClass"
          >{{ link.label }}</RouterLink>

          <span class="mx-1 hidden h-5 w-px bg-slate-200 dark:bg-slate-800 sm:block" />

          <!-- Live-reload indicator. Pulsing dot + "live" while the SSE stream
               is open; a transient "updated" flash when ADRs change on disk. -->
          <span
            class="hidden items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-medium transition-colors sm:inline-flex"
            :class="
              flash
                ? 'border-emerald-300 bg-emerald-50 text-emerald-700 dark:border-emerald-700/60 dark:bg-emerald-900/30 dark:text-emerald-300'
                : live.connected.value
                  ? 'border-slate-200 text-slate-500 dark:border-slate-800 dark:text-slate-400'
                  : 'border-amber-300 bg-amber-50 text-amber-700 dark:border-amber-700/60 dark:bg-amber-900/20 dark:text-amber-300'
            "
            :title="
              live.connected.value
                ? 'Live-reload connected — updates as ADRs change on disk'
                : 'Live-reload reconnecting…'
            "
          >
            <span
              class="inline-flex h-2 w-2 rounded-full"
              :class="flash || live.connected.value ? 'bg-emerald-500' : 'bg-amber-500'"
            />
            {{ flash ? 'updated' : live.connected.value ? 'live' : 'offline' }}
          </span>

          <!-- Theme toggle: system → light → dark → system. -->
          <button
            type="button"
            class="btn-spring inline-flex h-8 w-8 items-center justify-center rounded-lg border border-slate-200 text-slate-600 hover:bg-slate-100 dark:border-slate-800 dark:text-slate-300 dark:hover:bg-slate-800"
            :aria-label="`Theme: ${themeLabel} (click to cycle)`"
            :title="`Theme: ${themeLabel}`"
            @click="cycle"
          >
            <MonitorSmartphone v-if="mode === 'system'" :size="15" />
            <Sun v-else-if="resolved === 'light'" :size="15" />
            <Moon v-else :size="15" />
          </button>
        </nav>
      </div>
    </header>

    <main class="mx-auto w-full max-w-6xl flex-1 px-6 py-8 sm:py-10">
      <RouterView />
    </main>

    <footer class="mt-auto border-t border-slate-200/70 dark:border-slate-800/70">
      <div
        class="mx-auto flex max-w-6xl items-center justify-between gap-3 px-6 py-4 text-xs text-slate-500 dark:text-slate-400"
      >
        <span>adroit · read-only ADR dashboard</span>
        <span class="tabular">live-reload over SSE</span>
      </div>
    </footer>

    <DirectoryPicker :open="pickerOpen" @close="pickerOpen = false" @switched="onSwitched" />
  </div>
</template>
