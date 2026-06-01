<script setup lang="ts">
import { computed } from 'vue'
import type { AdrSummary, Status } from '@/api'
import { statusColor as fill } from '@/statusColor'

// Status by cohort: stacked columns, one per created-month, segmented by the
// ADR's *current* status. Derived client-side from listAdrs() because the API's
// created_over_time only exposes per-month totals — each AdrSummary carries both
// `created` and `status`, so we bucket by created-month and stack by status.
// ADRs with a null `created` are grouped under an "undated" column at the end.
const props = defineProps<{ adrs: AdrSummary[] }>()

// Stable status order (bottom→top of each column); fills come from the shared
// status palette (CSS vars).
const ORDER: Status[] = ['Accepted', 'Proposed', 'Superseded', 'Rejected', 'Deprecated']

const UNDATED = 'undated'

interface Cohort {
  key: string // "YYYY-MM" or "undated"
  label: string
  total: number
  counts: Record<Status, number>
}

const cohorts = computed<Cohort[]>(() => {
  const byMonth = new Map<string, Record<Status, number>>()
  const empty = (): Record<Status, number> => ({
    Proposed: 0,
    Accepted: 0,
    Rejected: 0,
    Deprecated: 0,
    Superseded: 0,
  })
  for (const a of props.adrs) {
    const key = a.created ? a.created.slice(0, 7) : UNDATED
    if (!byMonth.has(key)) byMonth.set(key, empty())
    byMonth.get(key)![a.status] += 1
  }
  // Sort dated months ascending; push "undated" last if present.
  const keys = [...byMonth.keys()].filter((k) => k !== UNDATED).sort()
  if (byMonth.has(UNDATED)) keys.push(UNDATED)
  return keys.map((key) => {
    const counts = byMonth.get(key)!
    const total = ORDER.reduce((sum, s) => sum + counts[s], 0)
    return { key, label: cohortLabel(key), total, counts }
  })
})

function cohortLabel(key: string): string {
  if (key === UNDATED) return 'undated'
  const [y, m] = key.split('-')
  const idx = Number(m) - 1
  const names = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
  return names[idx] ? `${names[idx]} '${y.slice(2)}` : key
}

const W = 720
const H = 260
const PAD = { top: 16, right: 16, bottom: 34, left: 34 }
const plotW = W - PAD.left - PAD.right
const plotH = H - PAD.top - PAD.bottom

const maxTotal = computed(() => Math.max(1, ...cohorts.value.map((c) => c.total)))

interface Segment {
  status: Status
  x: number
  y: number
  w: number
  h: number
  count: number
}

const columns = computed(() => {
  const n = cohorts.value.length
  if (n === 0) return []
  const slot = plotW / n
  const barW = Math.min(slot * 0.6, 44)
  const max = maxTotal.value
  return cohorts.value.map((c, i) => {
    const cx = PAD.left + slot * (i + 0.5)
    const x = cx - barW / 2
    let cursorY = PAD.top + plotH
    const segments: Segment[] = []
    for (const status of ORDER) {
      const count = c.counts[status]
      if (count === 0) continue
      const h = (count / max) * plotH
      cursorY -= h
      segments.push({ status, x, y: cursorY, w: barW, h, count })
    }
    return { cohort: c, x: cx, segments }
  })
})

const baseline = PAD.top + plotH
const labelEvery = computed(() => Math.max(1, Math.ceil(cohorts.value.length / 9)))
</script>

<template>
  <div>
    <p
      v-if="columns.length === 0"
      class="py-10 text-center text-sm text-slate-500 dark:text-slate-400"
    >
      No ADRs to chart yet.
    </p>
    <template v-else>
      <svg :viewBox="`0 0 ${W} ${H}`" class="cohort">
        <line :x1="PAD.left" :y1="baseline" :x2="W - PAD.right" :y2="baseline" class="axis" />

        <g v-for="col in columns" :key="col.cohort.key">
          <rect
            v-for="seg in col.segments"
            :key="`${col.cohort.key}-${seg.status}`"
            :x="seg.x"
            :y="seg.y"
            :width="seg.w"
            :height="seg.h"
            :style="{ fill: fill(seg.status) }"
            class="seg"
          >
            <title>{{ col.cohort.label }} · {{ seg.status }}: {{ seg.count }}</title>
          </rect>
          <text
            v-show="columns.indexOf(col) % labelEvery === 0"
            :x="col.x"
            :y="H - 10"
            class="x-label"
          >
            {{ col.cohort.label }}
          </text>
        </g>
      </svg>

      <ul class="mt-3 flex flex-wrap items-center gap-x-5 gap-y-1.5">
        <li
          v-for="status in ORDER"
          :key="status"
          class="flex items-center gap-1.5 text-xs text-slate-600 dark:text-slate-300"
        >
          <span class="h-2.5 w-2.5 rounded-[3px]" :style="{ background: fill(status) }" />
          {{ status }}
        </li>
      </ul>
    </template>
  </div>
</template>

<style scoped>
.cohort {
  width: 100%;
  height: auto;
}
.axis {
  stroke: var(--ad-border);
  stroke-width: 1;
}
.seg {
  stroke: var(--ad-bg-elevated-solid);
  stroke-width: 1;
}
.x-label {
  text-anchor: middle;
  font-size: 0.62rem;
  fill: var(--ad-text-muted);
}
</style>
