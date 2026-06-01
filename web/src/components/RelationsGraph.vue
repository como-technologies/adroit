<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import type { Graph, Status } from '@/api'

// Lightweight self-contained SVG graph (circular layout) — no external graph
// library. Nodes are placed on a circle; edges are drawn as lines/arrows;
// clicking a node opens that ADR. Colors are theme-aware via CSS tokens.
const props = defineProps<{ graph: Graph }>()

const router = useRouter()

const W = 820
const H = 620
const R = 210

interface Placed {
  number: number | null
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
  const labelR = R + 22
  return nodes.map((node, i) => {
    const angle = (i / n) * Math.PI * 2 - Math.PI / 2
    const cos = Math.cos(angle)
    return {
      number: node.number,
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

const posByNumber = computed(() => {
  const map = new Map<number, Placed>()
  for (const p of placed.value) {
    if (p.number !== null) map.set(p.number, p)
  }
  return map
})

const edges = computed(() =>
  (props.graph.edges ?? [])
    .map((e) => {
      const from = posByNumber.value.get(e.from)
      const to = posByNumber.value.get(e.to)
      if (!from || !to) return null
      return { from, to, kind: e.kind }
    })
    .filter((e): e is { from: Placed; to: Placed; kind: 'supersedes' | 'related' } => e !== null),
)

// Mid-tone status fills — readable on both light and dark surfaces.
const statusFill: Record<Status, string> = {
  Proposed: '#fbbf24',
  Accepted: '#34d399',
  Rejected: '#f87171',
  Deprecated: '#94a3b8',
  Superseded: '#a78bfa',
}

function truncate(s: string, n = 18): string {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s
}

function open(p: Placed) {
  if (p.number !== null) router.push(`/adr/${p.number}`)
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
      <circle :cx="p.x" :cy="p.y" r="22" :fill="statusFill[p.status]" class="node-circle" />
      <text :x="p.x" :y="p.y" class="node-num">
        {{ p.number !== null ? String(p.number).padStart(4, '0') : '?' }}
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
.edge.supersedes {
  stroke: var(--ad-text-muted);
}
.edge.related {
  stroke: var(--ad-text-muted);
  stroke-dasharray: 4 3;
  opacity: 0.45;
}
.arrow-head {
  fill: var(--ad-text-muted);
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
  fill: #0f172a;
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
