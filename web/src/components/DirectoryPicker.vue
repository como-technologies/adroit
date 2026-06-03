<script setup lang="ts">
import { nextTick, onUnmounted, ref, watch } from 'vue'
import { ArrowUp, ChevronRight, Folder, FolderOpen, X } from 'lucide-vue-next'
import { browseDir, type BrowseListing } from '@/api'
import { useWorkspace } from '@/composables/useWorkspace'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: []; switched: [dir: string] }>()

const workspace = useWorkspace()

const listing = ref<BrowseListing | null>(null)
const pathInput = ref('')
const loading = ref(false)
const switching = ref(false)
const error = ref('')
const inputRef = ref<HTMLInputElement | null>(null)

async function loadPath(path?: string) {
  loading.value = true
  error.value = ''
  try {
    const l = await browseDir(path)
    listing.value = l
    pathInput.value = l.path
  } catch (e) {
    error.value = (e as Error).message
  } finally {
    loading.value = false
  }
}

function goUp() {
  if (listing.value?.parent) loadPath(listing.value.parent)
}

function openTyped() {
  const p = pathInput.value.trim()
  if (p) loadPath(p)
}

async function chooseCurrent() {
  if (!listing.value || switching.value) return
  switching.value = true
  error.value = ''
  try {
    await workspace.switchTo(listing.value.path)
    emit('switched', listing.value.path)
  } catch (e) {
    error.value = (e as Error).message
  } finally {
    switching.value = false
  }
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      // Open at the active workspace dir (server default when no path given).
      loadPath()
      window.addEventListener('keydown', onKey)
      nextTick(() => inputRef.value?.focus())
    } else {
      window.removeEventListener('keydown', onKey)
    }
  },
)

onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-start justify-center overflow-y-auto bg-slate-900/40 p-4 backdrop-blur-sm sm:items-center"
      @click.self="emit('close')"
    >
      <div
        class="card-glass relative mt-10 flex w-full max-w-lg flex-col overflow-hidden sm:mt-0"
        role="dialog"
        aria-modal="true"
        aria-label="Open ADR directory"
      >
        <!-- Header -->
        <div class="flex items-center justify-between gap-3 border-b border-slate-200/70 px-5 py-3.5 dark:border-slate-800/70">
          <div class="flex items-center gap-2">
            <FolderOpen :size="16" class="text-brand-500" />
            <h2 class="font-display text-sm font-semibold text-slate-900 dark:text-slate-100">
              Open ADR directory
            </h2>
          </div>
          <button
            type="button"
            class="inline-flex h-7 w-7 items-center justify-center rounded-lg text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800 dark:hover:text-slate-200"
            aria-label="Close"
            @click="emit('close')"
          >
            <X :size="15" />
          </button>
        </div>

        <!-- Path bar -->
        <div class="flex items-center gap-2 border-b border-slate-200/70 px-5 py-3 dark:border-slate-800/70">
          <button
            type="button"
            class="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-slate-200 text-slate-500 transition-colors hover:border-brand-300 hover:text-slate-900 disabled:opacity-40 dark:border-slate-800 dark:text-slate-400 dark:hover:text-slate-100"
            :disabled="!listing?.parent"
            title="Up one level"
            @click="goUp"
          >
            <ArrowUp :size="15" />
          </button>
          <form class="relative flex-1" @submit.prevent="openTyped">
            <input
              ref="inputRef"
              v-model="pathInput"
              type="text"
              spellcheck="false"
              placeholder="/path/to/adrs"
              class="w-full rounded-lg border border-slate-300 bg-white/80 px-3 py-1.5 font-mono text-xs text-slate-900 placeholder:text-slate-400 focus:border-brand-400 dark:border-slate-700 dark:bg-slate-900/80 dark:text-slate-100"
            />
          </form>
          <button
            type="button"
            class="shrink-0 rounded-lg border border-slate-200 px-2.5 py-1.5 text-xs font-medium text-slate-600 transition-colors hover:border-brand-300 hover:text-slate-900 dark:border-slate-800 dark:text-slate-300 dark:hover:text-slate-100"
            @click="openTyped"
          >
            Go
          </button>
        </div>

        <!-- Listing -->
        <div class="max-h-[min(50vh,22rem)] overflow-y-auto px-2 py-2">
          <p v-if="loading" class="px-3 py-6 text-center text-sm text-slate-500 dark:text-slate-400">
            Loading…
          </p>
          <p
            v-else-if="error"
            class="mx-1 my-1 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700 dark:border-rose-800/50 dark:bg-rose-950/40 dark:text-rose-300"
          >
            {{ error }}
          </p>
          <template v-else-if="listing">
            <p
              v-if="listing.entries.length === 0"
              class="px-3 py-6 text-center text-sm text-slate-400 dark:text-slate-500"
            >
              No subfolders here.
            </p>
            <button
              v-for="entry in listing.entries"
              :key="entry.path"
              type="button"
              class="group flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left text-sm text-slate-700 transition-colors hover:bg-slate-100 dark:text-slate-200 dark:hover:bg-slate-800/60"
              @click="loadPath(entry.path)"
            >
              <Folder :size="15" class="shrink-0 text-brand-400" />
              <span class="min-w-0 flex-1 truncate">{{ entry.name }}</span>
              <ChevronRight
                :size="14"
                class="shrink-0 text-slate-300 transition-transform group-hover:translate-x-0.5 dark:text-slate-600"
              />
            </button>
          </template>
        </div>

        <!-- Footer -->
        <div class="flex items-center justify-between gap-3 border-t border-slate-200/70 px-5 py-3.5 dark:border-slate-800/70">
          <span class="text-xs text-slate-500 dark:text-slate-400">
            <template v-if="listing">
              <span
                v-if="listing.adr_count > 0"
                class="font-medium text-emerald-700 dark:text-emerald-400"
              >{{ listing.adr_count }} ADR{{ listing.adr_count === 1 ? '' : 's' }} here</span>
              <span v-else>No ADRs in this folder</span>
            </template>
          </span>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="rounded-lg px-3 py-1.5 text-sm font-medium text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
              @click="emit('close')"
            >
              Cancel
            </button>
            <button
              type="button"
              class="btn-spring rounded-lg bg-brand-600 px-3.5 py-1.5 text-sm font-medium text-white hover:bg-brand-700 disabled:opacity-50"
              :disabled="!listing || switching"
              @click="chooseCurrent"
            >
              {{ switching ? 'Opening…' : 'Open this directory' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
