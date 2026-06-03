<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { ArrowRight, BarChart3, Clock, CircleX, TriangleAlert, CircleCheck } from 'lucide-vue-next'
import { getStats, getCheck, type Stats, type Status, type CheckReport } from '@/api'
import { useLiveReload } from '@/useLiveReload'
import StatusPill from '@/components/StatusPill.vue'
import StatTile from '@/components/StatTile.vue'

const stats = ref<Stats | null>(null)
const check = ref<CheckReport | null>(null)
const loading = ref(false)
const error = ref('')

async function load() {
  loading.value = true
  error.value = ''
  try {
    // Checks are best-effort: a failure there must not blank the whole dashboard.
    const [s, c] = await Promise.all([getStats(), getCheck().catch(() => null)])
    stats.value = s
    check.value = c
  } catch (e) {
    error.value = (e as Error).message
  } finally {
    loading.value = false
  }
}

// Failed-check count for the headline "Issues" tile.
const issueCount = computed(() => check.value?.problems.length ?? 0)

// Compact byte size for the duplicate-file hints (e.g. "82 B", "4.1 KB").
function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}

onMounted(load)
// Recompute stats when ADR files change on disk.
useLiveReload(load)

function countOf(status: Status): number {
  return stats.value?.by_status.find((s) => s.status === status)?.count ?? 0
}

const maxStatusCount = computed(() =>
  Math.max(1, ...(stats.value?.by_status.map((s) => s.count) ?? [])),
)
const maxMonthCount = computed(() =>
  Math.max(1, ...(stats.value?.created_over_time.map((m) => m.count) ?? [])),
)

// Solid bar color per status (theme-aware via dark: variants).
const BAR: Record<Status, string> = {
  Proposed: 'bg-amber-400 dark:bg-amber-500',
  Accepted: 'bg-emerald-400 dark:bg-emerald-500',
  Rejected: 'bg-rose-400 dark:bg-rose-500',
  Deprecated: 'bg-slate-400 dark:bg-slate-500',
  Superseded: 'bg-violet-400 dark:bg-violet-500',
}

function monthLabel(month: string): string {
  // month is "YYYY-MM"; render as "Mon YYYY" when parseable.
  const [y, m] = month.split('-')
  const idx = Number(m) - 1
  const names = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
  return names[idx] ? `${names[idx]} ${y}` : month
}
</script>

<template>
  <section class="space-y-6">
    <div class="card-glass hero-gradient relative overflow-hidden px-6 py-5 sm:flex sm:items-center sm:justify-between sm:gap-4">
      <div>
        <div class="text-[11px] font-semibold uppercase tracking-wider text-brand-700 dark:text-brand-300">
          Dashboard
        </div>
        <h1 class="mt-0.5 font-display text-2xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
          Decision log at a glance
        </h1>
      </div>
      <RouterLink
        to="/insights"
        class="mt-3 inline-flex items-center gap-1.5 rounded-lg border border-slate-200 bg-white/70 px-3 py-1.5 text-xs font-medium text-slate-700 transition-colors hover:border-brand-300 hover:text-brand-700 dark:border-slate-800 dark:bg-slate-900/60 dark:text-slate-300 dark:hover:text-brand-300 sm:mt-0"
      >
        <BarChart3 :size="14" /> Explore insights
      </RouterLink>
    </div>

    <div v-if="loading" class="grid grid-cols-2 gap-3 md:grid-cols-4">
      <div
        v-for="i in 4"
        :key="i"
        class="h-24 animate-pulse rounded-2xl bg-slate-200/60 dark:bg-slate-800/50"
      />
    </div>

    <div
      v-else-if="error"
      class="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:border-rose-800/50 dark:bg-rose-950/40 dark:text-rose-300"
    >
      {{ error }}
    </div>

    <template v-else-if="stats">
      <!-- Headline tiles -->
      <div class="grid grid-cols-2 gap-3 md:grid-cols-4">
        <StatTile label="Total ADRs" :value="stats.total" tone="brand" />
        <StatTile label="Accepted" :value="countOf('Accepted')" tone="emerald" />
        <StatTile label="Proposed" :value="countOf('Proposed')" tone="amber" />
        <StatTile label="Issues" :value="issueCount" tone="rose" />
      </div>

      <div class="grid gap-4 lg:grid-cols-2">
        <!-- By status — each row jumps into Browse filtered by that status. -->
        <div class="card-glass p-5">
          <div class="flex items-center justify-between gap-3">
            <h2 class="font-display text-sm font-semibold text-slate-700 dark:text-slate-200">
              By status
            </h2>
            <RouterLink
              to="/browse"
              class="inline-flex items-center gap-1 text-xs font-medium text-slate-500 transition-colors hover:text-brand-700 dark:text-slate-400 dark:hover:text-brand-300"
            >
              Browse all <ArrowRight :size="13" />
            </RouterLink>
          </div>
          <div class="mt-4 space-y-2.5">
            <RouterLink
              v-for="s in stats.by_status"
              :key="s.status"
              :to="{ path: '/browse', query: { status: s.status } }"
              class="group flex items-center gap-3 rounded-lg px-1 py-0.5 transition-colors hover:bg-slate-100/60 dark:hover:bg-slate-800/40"
            >
              <div class="w-24 shrink-0">
                <StatusPill :status="s.status" size="sm" />
              </div>
              <div class="h-2.5 flex-1 overflow-hidden rounded-full bg-slate-200/70 dark:bg-slate-800/70">
                <div
                  class="h-full rounded-full transition-[width] duration-700 ease-out"
                  :class="BAR[s.status]"
                  :style="{ width: `${(s.count / maxStatusCount) * 100}%` }"
                />
              </div>
              <span class="w-6 shrink-0 text-right text-sm tabular font-medium text-slate-700 dark:text-slate-300">
                {{ s.count }}
              </span>
            </RouterLink>
          </div>
        </div>

        <!-- Created over time -->
        <div class="card-glass p-5">
          <div class="flex items-center justify-between gap-3">
            <h2 class="font-display text-sm font-semibold text-slate-700 dark:text-slate-200">
              Created over time
            </h2>
            <RouterLink
              to="/insights"
              class="inline-flex items-center gap-1 text-xs font-medium text-slate-500 transition-colors hover:text-brand-700 dark:text-slate-400 dark:hover:text-brand-300"
            >
              Insights <ArrowRight :size="13" />
            </RouterLink>
          </div>
          <p v-if="stats.created_over_time.length === 0" class="mt-4 text-sm text-slate-500 dark:text-slate-400">
            No dated ADRs yet.
          </p>
          <div v-else class="mt-4 space-y-2">
            <div v-for="m in stats.created_over_time" :key="m.month" class="flex items-center gap-3">
              <span class="w-20 shrink-0 text-xs tabular text-slate-500 dark:text-slate-400">
                {{ monthLabel(m.month) }}
              </span>
              <div class="h-2.5 flex-1 overflow-hidden rounded-full bg-slate-200/70 dark:bg-slate-800/70">
                <div
                  class="h-full rounded-full bg-gradient-to-r from-brand-500 to-violet-500 transition-[width] duration-700 ease-out"
                  :style="{ width: `${(m.count / maxMonthCount) * 100}%` }"
                />
              </div>
              <span class="w-6 shrink-0 text-right text-sm tabular font-medium text-slate-700 dark:text-slate-300">
                {{ m.count }}
              </span>
            </div>
          </div>
        </div>

        <!-- Proposed awaiting decision -->
        <div class="card-glass p-5">
          <h2 class="font-display text-sm font-semibold text-slate-700 dark:text-slate-200">
            Proposed · awaiting decision
          </h2>
          <p v-if="stats.proposed_age.length === 0" class="mt-4 text-sm text-slate-500 dark:text-slate-400">
            Nothing sitting in proposed. 🎉
          </p>
          <ul v-else class="mt-3 divide-y divide-slate-200/70 dark:divide-slate-800/70">
            <li
              v-for="p in stats.proposed_age"
              :key="p.title"
              class="flex items-center gap-3 py-2"
            >
              <span class="w-16 shrink-0 truncate font-mono text-xs tabular text-slate-400 dark:text-slate-500" :title="p.reference">
                {{ p.reference }}
              </span>
              <span class="min-w-0 flex-1 truncate text-sm text-slate-700 dark:text-slate-200">
                <RouterLink
                  :to="`/adr/${p.address}`"
                  class="hover:text-brand-700 dark:hover:text-brand-300"
                >{{ p.title }}</RouterLink>
              </span>
              <span
                class="flex shrink-0 items-center gap-1 text-xs tabular font-medium"
                :class="
                  p.review_due
                    ? 'text-rose-600 dark:text-rose-400'
                    : (p.age_days ?? 0) > 30
                      ? 'text-amber-700 dark:text-amber-400'
                      : 'text-slate-500 dark:text-slate-400'
                "
                :title="p.review_due ? 'Review due' : undefined"
              >
                <Clock v-if="p.review_due" :size="12" class="shrink-0" />
                {{ p.age_days !== null ? `${p.age_days}d` : '—' }}
              </span>
            </li>
          </ul>
        </div>

        <!-- Checks · repo health (mirrors `adroit check`) -->
        <div class="card-glass flex flex-col p-5">
          <div class="flex items-center justify-between gap-3">
            <h2 class="font-display text-sm font-semibold text-slate-700 dark:text-slate-200">
              Checks
            </h2>
            <span
              v-if="check && check.problems.length"
              class="inline-flex items-center rounded-full bg-rose-100 px-2 py-0.5 text-xs font-semibold text-rose-700 dark:bg-rose-950/50 dark:text-rose-300"
            >
              {{ check.problems.length }} {{ check.problems.length === 1 ? 'issue' : 'issues' }}
            </span>
          </div>

          <p v-if="!check" class="mt-4 text-sm text-slate-500 dark:text-slate-400">
            Checks unavailable.
          </p>
          <div
            v-else-if="check.problems.length === 0"
            class="mt-4 flex min-h-[13rem] flex-1 flex-col items-center justify-center gap-3 text-center"
          >
            <div
              class="flex h-14 w-14 items-center justify-center rounded-full bg-emerald-100 ring-1 ring-emerald-200/70 dark:bg-emerald-950/40 dark:ring-emerald-900/60"
            >
              <CircleCheck :size="26" class="text-emerald-600 dark:text-emerald-400" />
            </div>
            <div>
              <p class="font-display text-sm font-semibold text-emerald-700 dark:text-emerald-400">
                All checks passing
              </p>
              <p class="mt-1 text-xs text-slate-400 dark:text-slate-500">
                {{ check.checked }} ADRs validated · no issues found
              </p>
            </div>
          </div>
          <ul v-else class="mt-3 divide-y divide-slate-200/70 dark:divide-slate-800/70">
            <li
              v-for="(pr, i) in check.problems"
              :key="i"
              class="flex items-start gap-2.5 py-2"
            >
              <component
                :is="pr.severity === 'error' ? CircleX : TriangleAlert"
                :size="15"
                class="mt-1 shrink-0"
                :class="pr.severity === 'error' ? 'text-rose-500' : 'text-amber-500'"
              />
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
                  <span class="shrink-0 font-mono text-xs tabular text-slate-400 dark:text-slate-500">
                    {{ pr.label }}
                  </span>
                  <span class="min-w-0 break-words text-sm text-slate-700 dark:text-slate-200">
                    {{ pr.summary }}
                  </span>
                </div>
                <ul v-if="pr.paths.length" class="mt-1 space-y-0.5">
                  <li
                    v-for="f in pr.paths"
                    :key="f.path"
                    class="flex items-baseline justify-between gap-3"
                  >
                    <span
                      class="truncate font-mono text-xs text-slate-400 dark:text-slate-500"
                      :title="f.path"
                    >
                      {{ f.path }}
                    </span>
                    <span class="shrink-0 font-mono text-[11px] tabular text-slate-400 dark:text-slate-500">
                      {{ f.lines }} {{ f.lines === 1 ? 'line' : 'lines' }} · {{ fmtBytes(f.bytes) }}
                    </span>
                  </li>
                </ul>
              </div>
            </li>
          </ul>
        </div>
      </div>
    </template>
  </section>
</template>
