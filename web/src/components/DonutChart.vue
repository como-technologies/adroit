<script setup lang="ts">
import { computed } from 'vue'
import type { Status, StatusCount } from '@/api'
import { statusColor as fill } from '@/statusColor'

// Theme-aware donut of the status mix, rendered as hand-rolled inline SVG (no
// chart dependency — matches the relations-graph style). Each slice is an arc
// path; the center shows the total. A legend lists each status with its count.
// Slice/legend colors come from the shared status palette (CSS vars).
const props = defineProps<{ data: StatusCount[] }>()

const SIZE = 200
const CENTER = SIZE / 2
const OUTER = 88
const INNER = 54

const total = computed(() => props.data.reduce((sum, s) => sum + s.count, 0))

interface Slice {
  status: Status
  count: number
  path: string
}

// Polar → cartesian on the SIZE×SIZE canvas (0° at 12 o'clock, clockwise).
function point(radius: number, angle: number): [number, number] {
  const rad = ((angle - 90) * Math.PI) / 180
  return [CENTER + radius * Math.cos(rad), CENTER + radius * Math.sin(rad)]
}

function arcPath(startAngle: number, endAngle: number): string {
  const largeArc = endAngle - startAngle > 180 ? 1 : 0
  const [ox1, oy1] = point(OUTER, startAngle)
  const [ox2, oy2] = point(OUTER, endAngle)
  const [ix1, iy1] = point(INNER, endAngle)
  const [ix2, iy2] = point(INNER, startAngle)
  return [
    `M ${ox1} ${oy1}`,
    `A ${OUTER} ${OUTER} 0 ${largeArc} 1 ${ox2} ${oy2}`,
    `L ${ix1} ${iy1}`,
    `A ${INNER} ${INNER} 0 ${largeArc} 0 ${ix2} ${iy2}`,
    'Z',
  ].join(' ')
}

const slices = computed<Slice[]>(() => {
  const t = total.value
  if (t === 0) return []
  const present = props.data.filter((s) => s.count > 0)
  // A single non-zero slice can't be drawn as an arc (start == end after a full
  // turn); render it as a full ring instead via a near-360° sweep.
  let angle = 0
  return present.map((s) => {
    const sweep = (s.count / t) * 360
    const end = angle + Math.min(sweep, 359.999)
    const path = arcPath(angle, end)
    angle += sweep
    return { status: s.status, count: s.count, path }
  })
})

const pct = (count: number) => (total.value ? Math.round((count / total.value) * 100) : 0)
</script>

<template>
  <div class="flex flex-col items-center gap-5 sm:flex-row sm:items-center sm:gap-7">
    <svg :viewBox="`0 0 ${SIZE} ${SIZE}`" class="donut h-44 w-44 shrink-0">
      <g v-if="slices.length">
        <path
          v-for="s in slices"
          :key="s.status"
          :d="s.path"
          :style="{ fill: fill(s.status) }"
          class="slice"
        >
          <title>{{ s.status }}: {{ s.count }} ({{ pct(s.count) }}%)</title>
        </path>
      </g>
      <!-- Empty-state ring -->
      <circle
        v-else
        :cx="CENTER"
        :cy="CENTER"
        :r="(OUTER + INNER) / 2"
        fill="none"
        :stroke-width="OUTER - INNER"
        class="empty-ring"
      />
      <text :x="CENTER" :y="CENTER - 6" class="total-num">{{ total }}</text>
      <text :x="CENTER" :y="CENTER + 14" class="total-label">ADRs</text>
    </svg>

    <ul class="grid w-full grid-cols-1 gap-x-6 gap-y-1.5 sm:flex-1">
      <li
        v-for="s in data"
        :key="s.status"
        class="flex items-center gap-2.5 text-sm"
        :class="s.count === 0 ? 'opacity-50' : ''"
      >
        <span class="h-2.5 w-2.5 shrink-0 rounded-full" :style="{ background: fill(s.status) }" />
        <span class="flex-1 text-slate-700 dark:text-slate-200">{{ s.status }}</span>
        <span class="tabular font-medium text-slate-900 dark:text-slate-100">{{ s.count }}</span>
        <span class="w-10 shrink-0 text-right text-xs tabular text-slate-400 dark:text-slate-500">
          {{ pct(s.count) }}%
        </span>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.donut {
  height: auto;
}
.slice {
  stroke: var(--ad-bg-elevated-solid);
  stroke-width: 2;
}
.empty-ring {
  stroke: var(--ad-border);
}
.total-num {
  text-anchor: middle;
  dominant-baseline: central;
  font-size: 1.6rem;
  font-weight: 700;
  fill: var(--ad-text);
  font-variant-numeric: tabular-nums;
}
.total-label {
  text-anchor: middle;
  dominant-baseline: central;
  font-size: 0.6rem;
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  fill: var(--ad-text-muted);
}
</style>
