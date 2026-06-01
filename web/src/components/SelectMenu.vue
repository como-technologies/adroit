<script setup lang="ts">
// A themed dropdown that replaces the native <select>, whose option popup the
// browser renders with OS chrome (unstyleable). This is a button trigger + a
// custom listbox popover, so it matches the dashboard's dark/glass theme: a
// brand-highlighted active option, rounded elevated surface, and a brand focus
// ring. Keyboard-accessible (Arrow/Home/End/Enter/Esc) with click-outside close.
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { Check, ChevronDown } from 'lucide-vue-next'

interface SelectOption {
  value: string
  label: string
}

const props = withDefaults(
  defineProps<{
    modelValue: string
    options: SelectOption[]
    disabled?: boolean
    ariaLabel?: string
  }>(),
  { disabled: false, ariaLabel: undefined },
)
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()

const open = ref(false)
const root = ref<HTMLElement | null>(null)
const triggerRef = ref<HTMLButtonElement | null>(null)
const listRef = ref<HTMLElement | null>(null)
// Index the keyboard/pointer highlight currently sits on (while open).
const activeIndex = ref(0)
// The popover is teleported to <body> and positioned with `fixed` against the
// trigger, so no ancestor's `overflow-hidden` / stacking context can clip it.
const menuStyle = ref<Record<string, string>>({})

function updatePosition() {
  const el = triggerRef.value
  if (!el) return
  const r = el.getBoundingClientRect()
  menuStyle.value = {
    position: 'fixed',
    top: `${r.bottom + 6}px`,
    left: `${r.left}px`,
    width: `${r.width}px`,
  }
}

const selectedIndex = computed(() => {
  const i = props.options.findIndex((o) => o.value === props.modelValue)
  return i >= 0 ? i : 0
})
const selectedLabel = computed(() => props.options[selectedIndex.value]?.label ?? '')

function openMenu() {
  if (props.disabled || !props.options.length) return
  open.value = true
  activeIndex.value = selectedIndex.value
  updatePosition()
  // Capture-phase so a click anywhere outside closes before other handlers run;
  // capture scroll so the popover tracks the trigger inside nested scrollers.
  window.addEventListener('click', onDocClick, true)
  window.addEventListener('scroll', updatePosition, true)
  window.addEventListener('resize', updatePosition)
  nextTick(scrollActiveIntoView)
}
function closeMenu() {
  open.value = false
  window.removeEventListener('click', onDocClick, true)
  window.removeEventListener('scroll', updatePosition, true)
  window.removeEventListener('resize', updatePosition)
}
function toggle() {
  if (open.value) closeMenu()
  else openMenu()
}
function choose(i: number) {
  const opt = props.options[i]
  if (opt) emit('update:modelValue', opt.value)
  closeMenu()
}

function onDocClick(e: MouseEvent) {
  const t = e.target as Node
  // The trigger lives in `root`; the popover is teleported out, so check both.
  if (root.value?.contains(t) || listRef.value?.contains(t)) return
  closeMenu()
}

function scrollActiveIntoView() {
  const el = listRef.value?.children[activeIndex.value] as HTMLElement | undefined
  el?.scrollIntoView({ block: 'nearest' })
}

function move(delta: number) {
  const n = props.options.length
  if (!n) return
  activeIndex.value = (activeIndex.value + delta + n) % n
  scrollActiveIntoView()
}

function onKeydown(e: KeyboardEvent) {
  if (props.disabled) return
  if (!open.value) {
    if (['Enter', ' ', 'ArrowDown', 'ArrowUp'].includes(e.key)) {
      e.preventDefault()
      openMenu()
    }
    return
  }
  switch (e.key) {
    case 'Escape':
      e.preventDefault()
      closeMenu()
      break
    case 'ArrowDown':
      e.preventDefault()
      move(1)
      break
    case 'ArrowUp':
      e.preventDefault()
      move(-1)
      break
    case 'Home':
      e.preventDefault()
      activeIndex.value = 0
      scrollActiveIntoView()
      break
    case 'End':
      e.preventDefault()
      activeIndex.value = props.options.length - 1
      scrollActiveIntoView()
      break
    case 'Enter':
    case ' ':
      e.preventDefault()
      choose(activeIndex.value)
      break
  }
}

// If the control is disabled while open (e.g. Browse enters search mode), close.
watch(
  () => props.disabled,
  (d) => {
    if (d) closeMenu()
  },
)

onUnmounted(() => {
  window.removeEventListener('click', onDocClick, true)
  window.removeEventListener('scroll', updatePosition, true)
  window.removeEventListener('resize', updatePosition)
})
</script>

<template>
  <div ref="root" class="relative inline-block min-w-[8rem]">
    <button
      ref="triggerRef"
      type="button"
      role="combobox"
      :aria-expanded="open"
      aria-haspopup="listbox"
      :aria-label="ariaLabel"
      :disabled="disabled"
      class="inline-flex w-full items-center justify-between gap-2 rounded-lg border bg-white/80 px-3 py-1.5 text-sm text-slate-800 shadow-sm transition-colors disabled:cursor-not-allowed disabled:opacity-50 dark:bg-slate-900/80 dark:text-slate-100"
      :class="
        open
          ? 'border-brand-400 ring-2 ring-brand-500/40 dark:border-brand-400'
          : 'border-slate-300 hover:border-brand-300 dark:border-slate-700 dark:hover:border-brand-500'
      "
      @click="toggle"
      @keydown="onKeydown"
    >
      <span class="truncate">{{ selectedLabel }}</span>
      <ChevronDown
        :size="15"
        class="shrink-0 text-slate-400 transition-transform"
        :class="open ? 'rotate-180' : ''"
      />
    </button>

    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-100 ease-out"
        enter-from-class="-translate-y-1 opacity-0"
        enter-to-class="translate-y-0 opacity-100"
        leave-active-class="transition duration-75 ease-in"
        leave-from-class="translate-y-0 opacity-100"
        leave-to-class="-translate-y-1 opacity-0"
      >
        <ul
          v-if="open"
          ref="listRef"
          role="listbox"
          :style="menuStyle"
          class="z-50 max-h-60 overflow-auto rounded-lg border border-slate-200 bg-white py-1 shadow-lg shadow-slate-900/10 dark:border-slate-700 dark:bg-slate-800 dark:shadow-black/40"
        >
        <li
          v-for="(opt, i) in options"
          :key="opt.value"
          role="option"
          :aria-selected="i === selectedIndex"
          class="flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm transition-colors"
          :class="
            i === activeIndex ? 'bg-brand-600 text-white' : 'text-slate-700 dark:text-slate-200'
          "
          @mouseenter="activeIndex = i"
          @click="choose(i)"
        >
          <span class="flex-1 truncate">{{ opt.label }}</span>
          <Check
            v-if="i === selectedIndex"
            :size="14"
            class="shrink-0"
            :class="i === activeIndex ? 'text-white' : 'text-brand-500'"
          />
        </li>
        </ul>
      </Transition>
    </Teleport>
  </div>
</template>
