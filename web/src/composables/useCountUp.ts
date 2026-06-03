// Tween a numeric value from its previous value to a target with cubic-ease-out.
// Used to bring the stats tiles to life. Respects prefers-reduced-motion: when
// the user opts out of motion, the value snaps to the target with no animation.

import { onBeforeUnmount, ref, watch } from 'vue'

function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3)
}

function prefersReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    !!window.matchMedia &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  )
}

/**
 * Returns a ref that animates toward whatever `targetGetter` returns, re-running
 * on every change. Pass an arrow (`() => someRef.value`) so the target is
 * reactive.
 */
export function useCountUp(targetGetter: () => number, durationMs = 700) {
  const display = ref<number>(targetGetter())
  let frame = 0

  function animateTo(next: number) {
    cancelAnimationFrame(frame)
    if (prefersReducedMotion() || typeof requestAnimationFrame === 'undefined') {
      display.value = next
      return
    }
    const start = display.value
    const startedAt = performance.now()
    const tick = (now: number) => {
      const elapsed = now - startedAt
      const t = Math.min(1, elapsed / durationMs)
      display.value = start + (next - start) * easeOutCubic(t)
      if (t < 1) frame = requestAnimationFrame(tick)
      else display.value = next
    }
    frame = requestAnimationFrame(tick)
  }

  watch(targetGetter, (v) => animateTo(v), { immediate: true })
  onBeforeUnmount(() => cancelAnimationFrame(frame))

  return display
}
