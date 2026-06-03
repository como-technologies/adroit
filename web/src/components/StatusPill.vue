<script setup lang="ts">
import { computed } from 'vue'
import type { Status } from '@/api'

const props = withDefaults(
  defineProps<{
    status: Status
    /** Render a smaller pill (used inside dense table rows). */
    size?: 'sm' | 'md'
  }>(),
  { size: 'md' },
)

// Theme-aware Tailwind classes per status. Pastel chip in light mode, dimmed
// tint in dark mode — matching the shared house palette.
const TONES: Record<Status, { chip: string; dot: string }> = {
  Proposed: {
    chip: 'bg-amber-100 text-amber-800 ring-amber-200 dark:bg-amber-900/40 dark:text-amber-300 dark:ring-amber-800/60',
    dot: 'bg-amber-500',
  },
  Accepted: {
    chip: 'bg-emerald-100 text-emerald-800 ring-emerald-200 dark:bg-emerald-900/40 dark:text-emerald-300 dark:ring-emerald-800/60',
    dot: 'bg-emerald-500',
  },
  Rejected: {
    chip: 'bg-rose-100 text-rose-800 ring-rose-200 dark:bg-rose-900/40 dark:text-rose-300 dark:ring-rose-800/60',
    dot: 'bg-rose-500',
  },
  Deprecated: {
    chip: 'bg-slate-200 text-slate-700 ring-slate-300 dark:bg-slate-700/50 dark:text-slate-300 dark:ring-slate-600/60',
    dot: 'bg-slate-400',
  },
  Superseded: {
    chip: 'bg-violet-100 text-violet-800 ring-violet-200 dark:bg-violet-900/40 dark:text-violet-300 dark:ring-violet-800/60',
    dot: 'bg-violet-500',
  },
}

const tone = computed(() => TONES[props.status])
const sizing = computed(() =>
  props.size === 'sm' ? 'px-2 py-0.5 text-[10px]' : 'px-2.5 py-1 text-[11px]',
)
</script>

<template>
  <span
    class="inline-flex items-center gap-1.5 rounded-full font-semibold uppercase tracking-wide ring-1 ring-inset"
    :class="[tone.chip, sizing]"
  >
    <span class="h-1.5 w-1.5 rounded-full" :class="tone.dot" />
    {{ status }}
  </span>
</template>
