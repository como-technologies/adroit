<script setup lang="ts">
import { computed } from 'vue'
import { useCountUp } from '@/composables/useCountUp'

const props = withDefaults(
  defineProps<{
    label: string
    value: number
    /** Accent color for the value. */
    tone?: 'brand' | 'emerald' | 'amber' | 'rose' | 'slate'
  }>(),
  { tone: 'slate' },
)

const display = useCountUp(() => props.value)
const rounded = computed(() => Math.round(display.value))

const TONE_TEXT: Record<NonNullable<typeof props.tone>, string> = {
  brand: 'text-brand-600 dark:text-brand-300',
  emerald: 'text-emerald-600 dark:text-emerald-400',
  amber: 'text-amber-600 dark:text-amber-400',
  rose: 'text-rose-600 dark:text-rose-400',
  slate: 'text-slate-900 dark:text-slate-100',
}
</script>

<template>
  <div class="card-glass p-5">
    <div class="text-[10px] font-semibold uppercase tracking-wider text-slate-500 dark:text-slate-400">
      {{ label }}
    </div>
    <div class="mt-1 font-display text-3xl font-bold tabular" :class="TONE_TEXT[tone]">
      {{ rounded }}
    </div>
  </div>
</template>
