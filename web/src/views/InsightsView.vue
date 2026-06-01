<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Share2 } from 'lucide-vue-next'
import { getGraph, getStats, listAdrs, type AdrSummary, type Graph, type Stats } from '@/api'
import { useLiveReload } from '@/useLiveReload'
import RelationsGraph from '@/components/RelationsGraph.vue'
import DonutChart from '@/components/DonutChart.vue'
import GrowthChart from '@/components/GrowthChart.vue'
import CohortChart from '@/components/CohortChart.vue'

const graph = ref<Graph | null>(null)
const stats = ref<Stats | null>(null)
const adrs = ref<AdrSummary[]>([])
const loading = ref(false)
const error = ref('')

async function load() {
  loading.value = true
  error.value = ''
  try {
    // Charts need the stats summary, the relations graph, and the full ADR list
    // (the cohort×status breakdown is derived client-side from listAdrs).
    const [g, s, a] = await Promise.all([getGraph(), getStats(), listAdrs({})])
    graph.value = g
    stats.value = s
    adrs.value = a
  } catch (e) {
    error.value = (e as Error).message
  } finally {
    loading.value = false
  }
}

onMounted(load)
// Rebuild every visualization when ADR files change on disk.
useLiveReload(load)
</script>

<template>
  <section class="space-y-6">
    <div class="card-glass hero-gradient relative overflow-hidden px-6 py-5">
      <div class="text-[11px] font-semibold uppercase tracking-wider text-brand-700 dark:text-brand-300">
        Insights
      </div>
      <h1 class="mt-0.5 font-display text-2xl font-bold tracking-tight text-slate-900 dark:text-slate-100">
        Trends &amp; relations
      </h1>
    </div>

    <div v-if="loading" class="space-y-4">
      <div class="grid gap-4 lg:grid-cols-2">
        <div class="h-56 animate-pulse rounded-2xl bg-slate-200/60 dark:bg-slate-800/50" />
        <div class="h-56 animate-pulse rounded-2xl bg-slate-200/60 dark:bg-slate-800/50" />
      </div>
      <div class="h-[480px] animate-pulse rounded-2xl bg-slate-200/60 dark:bg-slate-800/50" />
    </div>

    <div
      v-else-if="error"
      class="rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-800 dark:border-rose-800/50 dark:bg-rose-950/40 dark:text-rose-300"
    >
      {{ error }}
    </div>

    <template v-else>
      <div class="grid gap-4 lg:grid-cols-2">
        <!-- Status breakdown donut -->
        <div class="card-glass p-5">
          <h2 class="font-display text-sm font-semibold text-slate-700 dark:text-slate-200">
            Status breakdown
          </h2>
          <div class="mt-4">
            <DonutChart v-if="stats" :data="stats.by_status" />
          </div>
        </div>

        <!-- Growth over time -->
        <div class="card-glass p-5">
          <h2 class="font-display text-sm font-semibold text-slate-700 dark:text-slate-200">
            Growth over time
          </h2>
          <p class="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
            Cumulative total (line) with per-month additions (bars).
          </p>
          <div class="mt-3">
            <GrowthChart v-if="stats" :data="stats.created_over_time" />
          </div>
        </div>
      </div>

      <!-- Status by cohort -->
      <div class="card-glass p-5">
        <h2 class="font-display text-sm font-semibold text-slate-700 dark:text-slate-200">
          Status by cohort
        </h2>
        <p class="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
          Each column is a creation month, stacked by current status. Undated ADRs are grouped last.
        </p>
        <div class="mt-3">
          <CohortChart :adrs="adrs" />
        </div>
      </div>

      <!-- Relationship wiki-graph (last) -->
      <div class="card-glass p-5">
        <h2 class="font-display text-sm font-semibold text-slate-700 dark:text-slate-200">
          Relationship graph
        </h2>

        <div
          v-if="graph && graph.nodes.length === 0"
          class="flex flex-col items-center gap-2 px-6 py-12 text-center"
        >
          <Share2 :size="28" class="text-slate-400" />
          <p class="text-sm text-slate-500 dark:text-slate-400">No ADRs to graph yet.</p>
        </div>
        <div v-else-if="graph" class="mt-3">
          <RelationsGraph :graph="graph" />
        </div>
      </div>
    </template>
  </section>
</template>
