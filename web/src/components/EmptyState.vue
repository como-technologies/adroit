<script setup lang="ts">
import type { Component } from 'vue'

// A centered empty / blank / error splash — the shared shape behind the
// dashboard's "All checks passing" panel. Pass a lucide-vue-next icon, a title,
// an optional subtitle, and a tone that colors the ringed icon badge.
withDefaults(
  defineProps<{
    icon: Component
    title: string
    subtitle?: string
    /** neutral = empty, success = all-clear, warn = couldn't-load / problem. */
    tone?: 'neutral' | 'success' | 'warn'
  }>(),
  { tone: 'neutral' },
)

// Badge background + ring + icon color (icons inherit `currentColor`).
const BADGE: Record<string, string> = {
  neutral:
    'bg-slate-100 text-slate-400 ring-slate-200/70 dark:bg-slate-800/50 dark:text-slate-400 dark:ring-slate-700/60',
  success:
    'bg-emerald-100 text-emerald-600 ring-emerald-200/70 dark:bg-emerald-950/40 dark:text-emerald-400 dark:ring-emerald-900/60',
  warn: 'bg-amber-100 text-amber-600 ring-amber-200/70 dark:bg-amber-950/40 dark:text-amber-400 dark:ring-amber-900/60',
}
const TITLE: Record<string, string> = {
  neutral: 'text-slate-600 dark:text-slate-300',
  success: 'text-emerald-700 dark:text-emerald-400',
  warn: 'text-amber-700 dark:text-amber-400',
}
</script>

<template>
  <div
    class="flex min-h-[12rem] flex-1 flex-col items-center justify-center gap-3 px-4 py-8 text-center"
  >
    <div
      class="flex h-14 w-14 items-center justify-center rounded-full ring-1"
      :class="BADGE[tone]"
    >
      <component :is="icon" :size="26" />
    </div>
    <div>
      <p class="font-display text-sm font-semibold" :class="TITLE[tone]">{{ title }}</p>
      <p v-if="subtitle" class="mt-1 text-xs text-slate-400 dark:text-slate-500">{{ subtitle }}</p>
    </div>
  </div>
</template>
