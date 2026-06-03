<script setup lang="ts">
import { computed } from 'vue'
import { CalendarClock } from 'lucide-vue-next'
import type { CreatedBucket } from '@/api'
import EmptyState from '@/components/EmptyState.vue'

// Growth over time: cumulative ADR count as a filled area/line, overlaid with
// per-month creation bars. Hand-rolled inline SVG (no chart dependency), styled
// with the brand gradient to match the dashboard's "created over time" panel.
const props = defineProps<{ data: CreatedBucket[] }>()

const W = 720
const H = 240
const PAD = { top: 16, right: 16, bottom: 34, left: 34 }
const plotW = W - PAD.left - PAD.right
const plotH = H - PAD.top - PAD.bottom

interface Pt {
  month: string
  count: number
  cumulative: number
  x: number
  yArea: number
  barX: number
  barW: number
  barY: number
  barH: number
}

const cumulativeMax = computed(() => {
  let running = 0
  for (const m of props.data) running += m.count
  return Math.max(1, running)
})

const points = computed<Pt[]>(() => {
  const n = props.data.length
  if (n === 0) return []
  const max = cumulativeMax.value
  // Bars get a slot per month; the line sits on the slot centers.
  const slot = plotW / n
  const barW = Math.min(slot * 0.5, 40)
  let running = 0
  return props.data.map((m, i) => {
    running += m.count
    const cx = PAD.left + slot * (i + 0.5)
    const yArea = PAD.top + plotH - (running / max) * plotH
    const barH = (m.count / max) * plotH
    return {
      month: m.month,
      count: m.count,
      cumulative: running,
      x: cx,
      yArea,
      barX: cx - barW / 2,
      barW,
      barY: PAD.top + plotH - barH,
      barH,
    }
  })
})

// Line path along cumulative points; the area path closes down to the baseline.
const linePath = computed(() => {
  const pts = points.value
  if (pts.length === 0) return ''
  return pts.map((p, i) => `${i === 0 ? 'M' : 'L'} ${p.x} ${p.yArea}`).join(' ')
})
const areaPath = computed(() => {
  const pts = points.value
  if (pts.length === 0) return ''
  const baseline = PAD.top + plotH
  const first = pts[0]
  const last = pts[pts.length - 1]
  return [
    `M ${first.x} ${baseline}`,
    ...pts.map((p) => `L ${p.x} ${p.yArea}`),
    `L ${last.x} ${baseline}`,
    'Z',
  ].join(' ')
})

const baseline = PAD.top + plotH

function monthLabel(month: string): string {
  const [y, m] = month.split('-')
  const idx = Number(m) - 1
  const names = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
  return names[idx] ? `${names[idx]} '${y.slice(2)}` : month
}

// Show at most ~8 x-axis labels so they never overlap.
const labelEvery = computed(() => Math.max(1, Math.ceil(points.value.length / 8)))
</script>

<template>
  <div>
    <EmptyState
      v-if="points.length === 0"
      :icon="CalendarClock"
      title="No dated ADRs yet"
      subtitle="Dates appear once ADRs are committed to git."
    />
    <svg v-else :viewBox="`0 0 ${W} ${H}`" class="growth">
      <defs>
        <linearGradient id="growth-area" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stop-color="var(--color-brand-500)" stop-opacity="0.32" />
          <stop offset="100%" stop-color="var(--color-brand-500)" stop-opacity="0.02" />
        </linearGradient>
      </defs>

      <!-- Baseline -->
      <line :x1="PAD.left" :y1="baseline" :x2="W - PAD.right" :y2="baseline" class="axis" />

      <!-- Per-month creation bars -->
      <rect
        v-for="p in points"
        :key="`bar-${p.month}`"
        :x="p.barX"
        :y="p.barY"
        :width="p.barW"
        :height="p.barH"
        rx="2"
        class="bar"
      >
        <title>{{ monthLabel(p.month) }}: +{{ p.count }} ({{ p.cumulative }} total)</title>
      </rect>

      <!-- Cumulative area + line -->
      <path :d="areaPath" fill="url(#growth-area)" />
      <path :d="linePath" class="line" />

      <!-- Cumulative dots -->
      <circle
        v-for="p in points"
        :key="`dot-${p.month}`"
        :cx="p.x"
        :cy="p.yArea"
        r="3"
        class="dot"
      >
        <title>{{ monthLabel(p.month) }}: {{ p.cumulative }} total</title>
      </circle>

      <!-- X labels -->
      <text
        v-for="(p, i) in points"
        v-show="i % labelEvery === 0"
        :key="`lbl-${p.month}`"
        :x="p.x"
        :y="H - 10"
        class="x-label"
      >
        {{ monthLabel(p.month) }}
      </text>
    </svg>
  </div>
</template>

<style scoped>
.growth {
  width: 100%;
  height: auto;
}
.axis {
  stroke: var(--ad-border);
  stroke-width: 1;
}
.bar {
  fill: var(--color-brand-400);
  opacity: 0.45;
}
.line {
  fill: none;
  stroke: var(--color-brand-500);
  stroke-width: 2.5;
  stroke-linejoin: round;
  stroke-linecap: round;
}
.dot {
  fill: var(--color-brand-600);
  stroke: var(--ad-bg-elevated-solid);
  stroke-width: 1.5;
}
.x-label {
  text-anchor: middle;
  font-size: 0.62rem;
  fill: var(--ad-text-muted);
}
</style>
