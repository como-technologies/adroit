<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { ArrowLeft, GitBranch, Link2, Clock, CalendarDays } from 'lucide-vue-next'
import { getAdr, type AdrDetail, type RelatedLink } from '@/api'
import { shortDate } from '@/util'
import { useLiveReload } from '@/useLiveReload'
import StatusPill from '@/components/StatusPill.vue'

const props = defineProps<{ number: string }>()

const adr = ref<AdrDetail | null>(null)
const loading = ref(false)
const error = ref('')

async function load() {
  loading.value = true
  error.value = ''
  try {
    adr.value = await getAdr(Number(props.number))
  } catch (e) {
    adr.value = null
    error.value = (e as Error).message
  } finally {
    loading.value = false
  }
}

onMounted(load)
watch(() => props.number, load)
// Re-fetch this ADR when files change on disk.
useLiveReload(load)

function adrLabel(n: number): string {
  return `ADR-${String(n).padStart(4, '0')}`
}

// `kind` only marks an edge as a supersession; the direction relative to *this*
// ADR comes from its own superseded_by / supersedes fields. So an edge to the
// ADR that replaced this one reads "Superseded by", not "Supersedes".
function linkLabel(link: RelatedLink): string {
  if (link.kind === 'supersedes') {
    return adr.value?.superseded_by === link.number ? 'Superseded by' : 'Supersedes'
  }
  return 'Related'
}

const relatedSorted = computed(() =>
  [...(adr.value?.related ?? [])].sort((a, b) => a.number - b.number),
)
</script>

<template>
  <section class="space-y-6">
    <RouterLink
      to="/browse"
      class="inline-flex items-center gap-1.5 text-sm text-slate-500 transition-colors hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
    >
      <ArrowLeft :size="15" /> Back to list
    </RouterLink>

    <div v-if="loading" class="space-y-4">
      <div class="h-28 animate-pulse rounded-2xl bg-slate-200/60 dark:bg-slate-800/50" />
      <div class="h-72 animate-pulse rounded-2xl bg-slate-200/60 dark:bg-slate-800/50" />
    </div>

    <div
      v-else-if="error"
      class="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:border-rose-800/50 dark:bg-rose-950/40 dark:text-rose-300"
    >
      {{ error }}
    </div>

    <article v-else-if="adr" class="space-y-6">
      <!-- Header -->
      <header class="card-glass hero-gradient relative overflow-hidden px-6 py-5">
        <div class="font-mono text-xs font-semibold uppercase tracking-wider text-brand-700 dark:text-brand-300">
          {{ adr.number_display }}
        </div>
        <h1 class="mt-1 font-display text-2xl font-bold tracking-tight text-slate-900 dark:text-slate-100 sm:text-3xl">
          {{ adr.title }}
        </h1>
        <div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-slate-600 dark:text-slate-300">
          <StatusPill :status="adr.status" />
          <span v-if="adr.created" class="inline-flex items-center gap-1.5 tabular">
            <CalendarDays :size="13" class="text-slate-400" /> {{ shortDate(adr.created) }}
          </span>
          <span
            v-if="adr.review_due"
            class="inline-flex items-center gap-1.5 rounded-full bg-amber-100 px-2 py-0.5 font-medium text-amber-800 dark:bg-amber-900/40 dark:text-amber-300"
          >
            <Clock :size="12" /> review due
          </span>
        </div>
      </header>

      <!-- Cross-links -->
      <nav
        v-if="relatedSorted.length"
        class="flex flex-wrap items-center gap-2"
        aria-label="Related ADRs"
      >
        <span class="text-xs font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
          Links
        </span>
        <RouterLink
          v-for="link in relatedSorted"
          :key="`${link.kind}-${link.number}`"
          :to="`/adr/${link.number}`"
          class="inline-flex items-center gap-1.5 rounded-full border border-slate-200 bg-white/70 px-3 py-1 text-xs font-medium text-slate-700 transition-colors hover:border-brand-300 hover:text-brand-700 dark:border-slate-800 dark:bg-slate-900/60 dark:text-slate-300 dark:hover:text-brand-300"
        >
          <GitBranch v-if="link.kind === 'supersedes'" :size="12" class="text-violet-500" />
          <Link2 v-else :size="12" class="text-slate-400" />
          <span>{{ linkLabel(link) }}</span>
          <span class="font-mono">{{ adrLabel(link.number) }}</span>
        </RouterLink>
      </nav>

      <!-- Body (server-rendered markdown) -->
      <div class="card-glass px-6 py-6 sm:px-8 sm:py-7">
        <div
          v-if="adr.body_html"
          class="prose prose-slate max-w-none dark:prose-invert prose-headings:font-display prose-headings:tracking-tight prose-a:text-brand-600 dark:prose-a:text-brand-300 prose-code:before:content-none prose-code:after:content-none prose-img:rounded-lg"
          v-html="adr.body_html"
        />
        <p v-else class="text-sm text-slate-500 dark:text-slate-400">This ADR has no body.</p>
      </div>
    </article>
  </section>
</template>
