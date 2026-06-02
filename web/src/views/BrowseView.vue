<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { ArrowRight, Inbox, Clock, Search, SearchX, X } from 'lucide-vue-next'
import { listAdrs, search, type AdrSummary, type Status } from '@/api'
import { highlight, shortDate } from '@/util'
import { useLiveReload } from '@/useLiveReload'
import StatusPill from '@/components/StatusPill.vue'
import SelectMenu from '@/components/SelectMenu.vue'

const route = useRoute()
const router = useRouter()

const adrs = ref<AdrSummary[]>([])
const status = ref<'' | Status>('')
const sort = ref('number')
const term = ref('')
const loading = ref(false)
const error = ref('')

// True while the box holds a non-empty query — drives the search vs. filter mode.
const searching = computed(() => term.value.trim().length > 0)

const STATUSES: Status[] = ['Proposed', 'Accepted', 'Rejected', 'Superseded', 'Deprecated']
const SORTS = [
  { value: 'number', label: 'Number' },
  { value: 'date', label: 'Newest' },
  { value: 'title', label: 'Title' },
]

// Seed filter / search / sort from the URL query, so deep-links work (the
// dashboard's by-status panel links here with `?status=`) AND the browser Back
// button restores the exact list state you left — we keep the query in sync below.
const q0 = route.query
if (typeof q0.status === 'string' && (STATUSES as string[]).includes(q0.status)) {
  status.value = q0.status as Status
}
if (typeof q0.sort === 'string' && SORTS.some((s) => s.value === q0.sort)) {
  sort.value = q0.sort
}
if (typeof q0.q === 'string') {
  term.value = q0.q
}

// Load either the search results (non-empty query) or the filtered list.
async function load() {
  const q = term.value.trim()
  loading.value = true
  error.value = ''
  try {
    adrs.value = q
      ? await search(q)
      : await listAdrs({ status: status.value || undefined, sort: sort.value })
  } catch (e) {
    error.value = (e as Error).message
  } finally {
    loading.value = false
  }
}

// Debounce typing so we don't spam the API (~200ms).
let debounceTimer: ReturnType<typeof setTimeout> | undefined
function debouncedLoad() {
  clearTimeout(debounceTimer)
  debounceTimer = setTimeout(load, 200)
}

function clearSearch() {
  term.value = ''
  // Immediate: dropping back to the filtered list shouldn't wait on the debounce.
  clearTimeout(debounceTimer)
  load()
}

onMounted(load)
// Status / sort changes refetch the filtered list immediately (ignored while a
// search query is active, since search drives the list then).
watch([status, sort], () => {
  if (!searching.value) load()
})
// Typing in the box refetches (debounced); clearing it returns to the list.
watch(term, debouncedLoad)
// Mirror the active filter / search / sort into the URL (replace → no history
// spam), so clicking into an ADR and pressing Back returns to this exact view.
watch([status, sort, term], () => {
  const query: Record<string, string> = {}
  if (status.value) query.status = status.value
  if (sort.value !== 'number') query.sort = sort.value
  const q = term.value.trim()
  if (q) query.q = q
  router.replace({ query }).catch(() => {})
})
// Re-fetch (current mode) when ADR files change on disk.
useLiveReload(load)

onUnmounted(() => clearTimeout(debounceTimer))

const reviewDueCount = computed(() => adrs.value.filter((a) => a.review_due).length)
</script>

<template>
  <section class="space-y-6">
    <!-- Header strip -->
    <div
      class="card-glass hero-gradient relative overflow-hidden px-6 py-5 sm:flex sm:items-center sm:justify-between sm:gap-4"
    >
      <div>
        <div class="text-[11px] font-semibold uppercase tracking-wider text-brand-700 dark:text-brand-300">
          Browse
        </div>
        <h1 class="mt-0.5 font-display text-2xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
          Architecture Decision Records
        </h1>
        <p class="mt-1 text-sm text-slate-600 dark:text-slate-300">
          <template v-if="searching">
            {{ adrs.length }} result{{ adrs.length === 1 ? '' : 's' }} for
            “<span class="font-medium text-slate-800 dark:text-slate-100">{{ term.trim() }}</span>”
          </template>
          <template v-else>
            {{ adrs.length }} record{{ adrs.length === 1 ? '' : 's' }}{{ status ? ` · ${status}` : '' }}
            <span v-if="reviewDueCount" class="text-amber-700 dark:text-amber-300">
              · {{ reviewDueCount }} due for review
            </span>
          </template>
        </p>
      </div>
      <label class="mt-4 block text-xs sm:mt-0">
        <span class="mb-1 block font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
          Sort
        </span>
        <SelectMenu v-model="sort" :options="SORTS" :disabled="searching" aria-label="Sort order" />
      </label>
    </div>

    <!-- Search box — empty returns to the filtered list. -->
    <div class="relative">
      <Search
        :size="18"
        class="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-slate-400"
      />
      <input
        v-model="term"
        type="search"
        placeholder="Search titles and bodies…"
        class="w-full rounded-xl border border-slate-300 bg-white/80 py-2.5 pl-11 pr-11 text-sm text-slate-900 shadow-sm placeholder:text-slate-400 focus:border-brand-400 dark:border-slate-700 dark:bg-slate-900/80 dark:text-slate-100"
      />
      <button
        v-if="searching"
        type="button"
        class="absolute right-2 top-1/2 inline-flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-lg text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800 dark:hover:text-slate-200"
        aria-label="Clear search"
        title="Clear search"
        @click="clearSearch"
      >
        <X :size="15" />
      </button>
    </div>

    <!-- Status filter — segmented pills (hidden while searching). -->
    <div v-if="!searching" class="flex flex-wrap gap-1.5">
      <button
        type="button"
        class="rounded-full px-3 py-1.5 text-xs font-medium transition-colors"
        :class="
          status === ''
            ? 'bg-brand-600 text-white shadow-sm shadow-brand-500/30'
            : 'border border-slate-200 text-slate-600 hover:border-brand-300 hover:text-slate-900 dark:border-slate-800 dark:text-slate-400 dark:hover:text-slate-100'
        "
        @click="status = ''"
      >
        All
      </button>
      <button
        v-for="s in STATUSES"
        :key="s"
        type="button"
        class="rounded-full px-3 py-1.5 text-xs font-medium transition-colors"
        :class="
          status === s
            ? 'bg-brand-600 text-white shadow-sm shadow-brand-500/30'
            : 'border border-slate-200 text-slate-600 hover:border-brand-300 hover:text-slate-900 dark:border-slate-800 dark:text-slate-400 dark:hover:text-slate-100'
        "
        @click="status = s"
      >
        {{ s }}
      </button>
    </div>

    <!-- States -->
    <div v-if="loading" class="space-y-2">
      <div
        v-for="i in 5"
        :key="i"
        class="h-[58px] animate-pulse rounded-xl bg-slate-200/60 dark:bg-slate-800/50"
      />
    </div>

    <div
      v-else-if="error"
      class="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:border-rose-800/50 dark:bg-rose-950/40 dark:text-rose-300"
    >
      {{ error }}
    </div>

    <div
      v-else-if="adrs.length === 0"
      class="card-glass flex flex-col items-center gap-2 px-6 py-12 text-center"
    >
      <SearchX v-if="searching" :size="28" class="text-slate-400" />
      <Inbox v-else :size="28" class="text-slate-400" />
      <p class="text-sm text-slate-500 dark:text-slate-400">
        <template v-if="searching">
          No ADRs matched “<span class="font-medium text-slate-700 dark:text-slate-200">{{ term.trim() }}</span>”.
        </template>
        <template v-else>No ADRs match this filter.</template>
      </p>
    </div>

    <!-- ADR list -->
    <div v-else class="card-glass divide-y divide-slate-200/70 overflow-hidden dark:divide-slate-800/70">
      <component
        :is="RouterLink"
        v-for="a in adrs"
        :key="a.address"
        :to="`/adr/${a.address}`"
        class="group flex items-center gap-4 px-4 py-3.5 transition-colors hover:bg-slate-50 sm:px-5 dark:hover:bg-slate-800/40"
      >
        <span
          class="w-14 shrink-0 truncate font-mono text-xs tabular text-slate-400 dark:text-slate-500"
          :title="a.reference"
        >{{ a.reference }}</span>

        <div class="min-w-0 flex-1">
          <div
            v-if="searching"
            class="truncate font-medium text-slate-900 dark:text-slate-100"
            v-html="highlight(a.title, term.trim())"
          />
          <div v-else class="truncate font-medium text-slate-900 dark:text-slate-100">
            {{ a.title }}
          </div>
          <div class="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-slate-500 dark:text-slate-400">
            <span class="tabular">{{ shortDate(a.created) }}</span>
            <span
              v-if="a.review_due"
              class="inline-flex items-center gap-1 rounded-full bg-amber-100 px-1.5 py-0.5 font-medium text-amber-800 dark:bg-amber-900/40 dark:text-amber-300"
            >
              <Clock :size="11" /> review due
            </span>
            <span v-if="a.supersedes.length" class="text-slate-400 dark:text-slate-500">
              supersedes {{ a.supersedes.join(', ') }}
            </span>
          </div>
        </div>

        <StatusPill :status="a.status" size="sm" />
        <ArrowRight
          :size="15"
          class="hidden shrink-0 text-slate-300 transition-all group-hover:translate-x-0.5 group-hover:text-brand-500 sm:block dark:text-slate-600"
        />
      </component>
    </div>
  </section>
</template>
