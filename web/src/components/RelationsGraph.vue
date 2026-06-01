<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import type { Graph, Status } from '@/api'
import { statusColor } from '@/statusColor'

// Lightweight self-contained SVG graph (circular layout) — no external graph
// library. Nodes are placed on a circle; edges are drawn as lines/arrows;
// clicking a node opens that ADR. Colors are theme-aware via CSS tokens.
const props = defineProps<{ graph: Graph }>()

const router = useRouter()

const W = 820
const H = 620
const R = 210

interface Placed {
  reference: string
  address: string | null
  title: string
  status: Status
  x: number
  y: number
  labelX: number
  labelY: number
  anchor: 'start' | 'middle' | 'end'
}

const placed = computed<Placed[]>(() => {
  const nodes = props.graph.nodes ?? []
  const cx = W / 2
  const cy = H / 2
  const n = Math.max(1, nodes.length)
  // Node radius is 22; offset labels well clear of the circle so they don't
  // sit on the bubble (R + radius left zero gap and they overlapped).
  const labelR = R + 40
  return nodes.map((node, i) => {
    const angle = (i / n) * Math.PI * 2 - Math.PI / 2
    const cos = Math.cos(angle)
    return {
      reference: node.reference,
      address: node.address,
      title: node.title,
      status: node.status,
      x: cx + R * cos,
      y: cy + R * Math.sin(angle),
      labelX: cx + labelR * cos,
      labelY: cy + labelR * Math.sin(angle),
      anchor: Math.abs(cos) < 0.3 ? 'middle' : cos > 0 ? 'start' : 'end',
    }
  })
})

const posByRef = computed(() => {
  const map = new Map<string, Placed>()
  for (const p of placed.value) map.set(p.reference, p)
  return map
})

const edges = computed(() =>
  (props.graph.edges ?? [])
    .map((e) => {
      const from = posByRef.value.get(e.from)
      const to = posByRef.value.get(e.to)
      if (!from || !to) return null
      return { from, to, kind: e.kind }
    })
    .filter((e): e is { from: Placed; to: Placed; kind: 'supersedes' | 'related' } => e !== null),
)

function truncate(s: string, n = 18): string {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s
}

// Compact id for the node circle: drop the `ADR-` prefix, clip to 8 chars
// (handles `ADR-0006` → `0006`, a date slug, or a short uuid alike).
function nodeId(p: Placed): string {
  const s = p.reference.replace(/^ADR-/, '')
  return s.length > 8 ? s.slice(0, 8) : s
}

function open(p: Placed) {
  if (p.address !== null) router.push(`/adr/${p.address}`)
}
</script>

<template>
  <svg :viewBox="`0 0 ${W} ${H}`" class="graph">
    <defs>
      <marker
        id="arrow"
        viewBox="0 0 10 10"
        refX="9"
        refY="5"
        markerWidth="7"
        markerHeight="7"
        orient="auto-start-reverse"
      >
        <path d="M 0 0 L 10 5 L 0 10 z" class="arrow-head" />
      </marker>
    </defs>

    <line
      v-for="(e, i) in edges"
      :key="`e${i}`"
      :x1="e.from.x"
      :y1="e.from.y"
      :x2="e.to.x"
      :y2="e.to.y"
      :class="['edge', e.kind]"
      :marker-end="e.kind === 'supersedes' ? 'url(#arrow)' : undefined"
    />

    <g v-for="(p, i) in placed" :key="`n${i}`" class="node" @click="open(p)">
      <circle :cx="p.x" :cy="p.y" r="22" :style="{ fill: statusColor(p.status) }" class="node-circle" />
      <text :x="p.x" :y="p.y" class="node-num">
        {{ nodeId(p) }}
      </text>
      <text :x="p.labelX" :y="p.labelY" :text-anchor="p.anchor" class="node-label">
        {{ truncate(p.title) }}
      </text>
      <title>{{ p.title }} ({{ p.status }})</title>
    </g>
  </svg>
</template>

<style scoped>
.graph {
  width: 100%;
  height: auto;
}

/* Edges */
.edge {
  stroke-width: 1.75;
}
/* Supersedes: solid green (a decision was replaced by a newer one). */
.edge.supersedes {
  stroke: var(--ad-edge-supersedes);
}
/* Related: dashed, muted grey. */
.edge.related {
  stroke: var(--ad-text-muted);
  stroke-dasharray: 4 3;
  opacity: 0.45;
}
.arrow-head {
  fill: var(--ad-edge-supersedes);
}

/* Nodes */
.node {
  cursor: pointer;
}
.node-circle {
  stroke: var(--ad-bg-elevated-solid);
  stroke-width: 2.5;
  transition: stroke 150ms ease;
}
.node:hover .node-circle {
  stroke: var(--color-brand-500);
  stroke-width: 3.5;
}
.node-num {
  text-anchor: middle;
  dominant-baseline: central;
  font-size: 0.62rem;
  font-weight: 700;
  fill: var(--ad-status-fg);
  pointer-events: none;
}
.node-label {
  dominant-baseline: central;
  font-size: 0.7rem;
  font-weight: 500;
  fill: var(--ad-text);
  pointer-events: none;
}
</style>
