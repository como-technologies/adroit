<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import {
  ArrowLeft,
  GitBranch,
  Link2,
  Clock,
  CalendarDays,
  History,
  GitPullRequest,
  CircleDot,
  ExternalLink,
  Check,
} from 'lucide-vue-next'
import { getAdr, getAdrForge, type AdrDetail, type ForgeData, type RelatedLink } from '@/api'
import { shortDate } from '@/util'
import { useLiveReload } from '@/useLiveReload'
import StatusPill from '@/components/StatusPill.vue'

const props = defineProps<{ id: string }>()

const adr = ref<AdrDetail | null>(null)
const loading = ref(false)
const error = ref('')
const forge = ref<ForgeData | null>(null)

async function load() {
  loading.value = true
  error.value = ''
  try {
    adr.value = await getAdr(props.id)
  } catch (e) {
    adr.value = null
    error.value = (e as Error).message
  } finally {
    loading.value = false
  }
}

// Read-only forge enrichment (issue/PR links + PR state) — separate, best-effort,
// and non-fatal: a missing/disabled forge just hides the panel, never blocks the
// ADR. Fetched on mount / id change only (not on every live-reload tick) to avoid
// hammering the remote forge API.
async function loadForge() {
  try {
    forge.value = await getAdrForge(props.id)
  } catch {
    forge.value = null
  }
}

onMounted(() => {
  load()
  loadForge()
})
watch(() => props.id, () => {
  load()
  loadForge()
})
// Re-fetch this ADR when files change on disk.
useLiveReload(load)

// Show the panel only when there's actual forge state to show.
const hasForge = computed(() => !!(forge.value && (forge.value.issue_url || forge.value.pr_url)))

// Map a CI status string ("success"/"pending"/"failure"/"none") to a label +
// pill classes; `undefined`/"none" render nothing.
const CI_STYLES: Record<string, { label: string; cls: string }> = {
  success: {
    label: 'CI passing',
    cls: 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300',
  },
  pending: {
    label: 'CI running',
    cls: 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300',
  },
  failure: {
    label: 'CI failing',
    cls: 'bg-rose-100 text-rose-800 dark:bg-rose-900/40 dark:text-rose-300',
  },
}
const ciStyle = computed(() => (forge.value?.pr_ci ? CI_STYLES[forge.value.pr_ci] : undefined))

// `kind` only marks an edge as a supersession; the direction relative to *this*
// ADR comes from its own superseded_by / supersedes fields. So an edge to the
// ADR that replaced this one reads "Superseded by", not "Supersedes".
function linkLabel(link: RelatedLink): string {
  if (link.kind === 'supersedes') {
    return adr.value?.superseded_by === link.reference ? 'Superseded by' : 'Supersedes'
  }
  return 'Related'
}

const relatedSorted = computed(() =>
  [...(adr.value?.related ?? [])].sort((a, b) => a.reference.localeCompare(b.reference)),
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
            v-if="adr.last_modified"
            class="inline-flex items-center gap-1.5 tabular"
            title="Last commit touching this ADR"
          >
            <History :size="13" class="text-slate-400" /> updated {{ shortDate(adr.last_modified) }}
          </span>
          <span
            v-if="adr.review_due"
            class="inline-flex items-center gap-1.5 rounded-full bg-amber-100 px-2 py-0.5 font-medium text-amber-800 dark:bg-amber-900/40 dark:text-amber-300"
          >
            <Clock :size="12" /> review due
          </span>
        </div>
      </header>

      <!-- Lifecycle timeline (git-derived: proposed → accepted/rejected/…) -->
      <section v-if="adr.history.length" class="card-glass px-6 py-5">
        <h2
          class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400"
        >
          <History :size="13" /> Timeline
        </h2>
        <ol class="mt-3 space-y-3">
          <li
            v-for="(e, i) in adr.history"
            :key="`${e.commit}-${i}`"
            class="flex items-center gap-3"
          >
            <span
              class="w-20 shrink-0 font-mono text-xs tabular text-slate-400 dark:text-slate-500"
            >{{ shortDate(e.date) }}</span>
            <StatusPill :status="e.status" size="sm" />
            <span
              class="min-w-0 flex-1 truncate text-sm text-slate-600 dark:text-slate-300"
              :title="e.subject"
            >{{ e.subject }}</span>
            <span class="hidden shrink-0 font-mono text-xs text-slate-400 dark:text-slate-500 sm:inline">{{ e.commit }}</span>
          </li>
        </ol>
      </section>

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
          :key="`${link.kind}-${link.address}`"
          :to="`/adr/${link.address}`"
          class="inline-flex items-center gap-1.5 rounded-full border border-slate-200 bg-white/70 px-3 py-1 text-xs font-medium text-slate-700 transition-colors hover:border-brand-300 hover:text-brand-700 dark:border-slate-800 dark:bg-slate-900/60 dark:text-slate-300 dark:hover:text-brand-300"
        >
          <GitBranch v-if="link.kind === 'supersedes'" :size="12" class="text-violet-500" />
          <Link2 v-else :size="12" class="text-slate-400" />
          <span>{{ linkLabel(link) }}</span>
          <span class="font-mono">{{ link.reference }}</span>
        </RouterLink>
      </nav>

      <!-- Forge state (read-only enrichment: linked issue + PR review state) -->
      <section v-if="hasForge" class="card-glass px-6 py-5">
        <h2
          class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400"
        >
          <GitPullRequest :size="13" /> Forge
        </h2>
        <div class="mt-3 flex flex-wrap items-center gap-2">
          <a
            v-if="forge?.issue_url"
            :href="forge.issue_url"
            target="_blank"
            rel="noopener noreferrer"
            class="inline-flex items-center gap-1.5 rounded-full border border-slate-200 bg-white/70 px-3 py-1 text-xs font-medium text-slate-700 transition-colors hover:border-brand-300 hover:text-brand-700 dark:border-slate-800 dark:bg-slate-900/60 dark:text-slate-300 dark:hover:text-brand-300"
          >
            <CircleDot :size="12" class="text-emerald-500" /> Issue
            <ExternalLink :size="11" class="text-slate-400" />
          </a>
          <a
            v-if="forge?.pr_url"
            :href="forge.pr_url"
            target="_blank"
            rel="noopener noreferrer"
            class="inline-flex items-center gap-1.5 rounded-full border border-slate-200 bg-white/70 px-3 py-1 text-xs font-medium text-slate-700 transition-colors hover:border-brand-300 hover:text-brand-700 dark:border-slate-800 dark:bg-slate-900/60 dark:text-slate-300 dark:hover:text-brand-300"
          >
            <GitPullRequest :size="12" class="text-violet-500" /> Pull request
            <ExternalLink :size="11" class="text-slate-400" />
          </a>
          <span
            v-if="forge?.pr_merged"
            class="inline-flex items-center gap-1.5 rounded-full bg-violet-100 px-2.5 py-1 text-xs font-medium text-violet-800 dark:bg-violet-900/40 dark:text-violet-300"
          >
            <GitBranch :size="12" /> merged
          </span>
          <span
            v-if="ciStyle"
            class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium"
            :class="ciStyle.cls"
          >
            {{ ciStyle.label }}
          </span>
          <span
            v-if="forge?.pr_approvals != null && forge.pr_approvals > 0"
            class="inline-flex items-center gap-1.5 rounded-full bg-emerald-100 px-2.5 py-1 text-xs font-medium text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300"
          >
            <Check :size="12" /> {{ forge.pr_approvals }} approval{{ forge.pr_approvals === 1 ? '' : 's' }}
          </span>
        </div>
      </section>

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
