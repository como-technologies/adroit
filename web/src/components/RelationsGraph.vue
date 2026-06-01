<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from 'd3-force'
import { select } from 'd3-selection'
import { zoom, type D3ZoomEvent } from 'd3-zoom'
import type { EdgeKind, Graph, Status } from '@/api'
import { statusColor } from '@/statusColor'

// A force-directed "wiki-graph" of ADR relationships. d3 supplies the physics
// (force simulation) and zoom/pan math; we keep rendering our own theme-aware
// SVG (status-colored nodes, per-EdgeKind edges, click-to-open) so it matches
// the rest of the dashboard. Node drag is hand-rolled via pointer events.
const props = defineProps<{ graph: Graph }>()
const router = useRouter()

const W = 820
const H = 600
const NODE_R = 9

interface SimNode extends SimulationNodeDatum {
  ref: string
  address: string | null
  title: string
  status: Status
}
interface SimLink extends SimulationLinkDatum<SimNode> {
  kind: EdgeKind
}

// All edge kinds, in legend order, with their CSS color token + whether the
// relationship is directional (gets an arrowhead).
const KINDS: { kind: EdgeKind; label: string; directed: boolean }[] = [
  { kind: 'supersedes', label: 'Supersedes', directed: true },
  { kind: 'depends_on', label: 'Depends on', directed: true },
  { kind: 'refines', label: 'Refines', directed: true },
  { kind: 'relates_to', label: 'Relates to', directed: false },
  { kind: 'related', label: 'Related', directed: false },
]
function edgeColor(kind: EdgeKind): string {
  return `var(--ad-edge-${kind.replace(/_/g, '-')})`
}
function isDirected(kind: EdgeKind): boolean {
  return KINDS.find((k) => k.kind === kind)?.directed ?? false
}

// Which edge kinds are currently shown (legend toggles).
const visibleKinds = ref<Set<EdgeKind>>(new Set(KINDS.map((k) => k.kind)))
function toggleKind(kind: EdgeKind) {
  const next = new Set(visibleKinds.value)
  if (next.has(kind)) next.delete(kind)
  else next.add(kind)
  visibleKinds.value = next
}

// d3 mutates these plain objects in place; `frame` is bumped each tick so the
// computed `view` re-reads the fresh x/y and Vue re-renders.
let simNodes: SimNode[] = []
let simLinks: SimLink[] = []
let sim: Simulation<SimNode, SimLink> | null = null
const frame = ref(0)
const transform = ref({ x: 0, y: 0, k: 1 })
const svgRef = ref<SVGSVGElement | null>(null)
const gRef = ref<SVGGElement | null>(null)

const transformStr = computed(
  () => `translate(${transform.value.x} ${transform.value.y}) scale(${transform.value.k})`,
)

const view = computed(() => {
  void frame.value // re-read positions whenever the sim ticks
  const nodes = simNodes
  const links = simLinks
    .filter((l) => visibleKinds.value.has(l.kind))
    .map((l) => {
      const s = l.source as SimNode
      const t = l.target as SimNode
      return {
        x1: s.x ?? 0,
        y1: s.y ?? 0,
        x2: t.x ?? 0,
        y2: t.y ?? 0,
        kind: l.kind,
      }
    })
  return { nodes, links }
})

function build() {
  simNodes = props.graph.nodes.map((n) => ({
    ref: n.reference,
    address: n.address,
    title: n.title,
    status: n.status,
  }))
  const byRef = new Map(simNodes.map((n) => [n.ref, n]))
  simLinks = props.graph.edges
    .filter((e) => byRef.has(e.from) && byRef.has(e.to))
    .map((e) => ({ source: byRef.get(e.from)!, target: byRef.get(e.to)!, kind: e.kind }))

  sim?.stop()
  // forceX/forceY (toward the center) replace forceCenter so that *every*
  // component — including isolated nodes and small clusters — is pulled inward,
  // instead of drifting to the edges (forceCenter only shifts the whole
  // centroid, which lets disconnected pieces fly apart).
  sim = forceSimulation<SimNode, SimLink>(simNodes)
    .force(
      'link',
      forceLink<SimNode, SimLink>(simLinks)
        .id((d) => d.ref)
        .distance(70)
        .strength(0.5),
    )
    .force('charge', forceManyBody().strength(-260).distanceMax(360))
    .force('x', forceX(W / 2).strength(0.09))
    .force('y', forceY(H / 2).strength(0.09))
    .force('collide', forceCollide(NODE_R + 16))
    .on('tick', () => {
      frame.value++
    })
}

// --- node drag (pointer events; sets fx/fy and reheats the sim) ------------
let dragging: SimNode | null = null
let didDrag = false

// Convert a pointer position to the inner <g>'s coordinate system using the
// element's screen CTM. This accounts for BOTH the viewBox→screen scaling and
// the current zoom transform, so a dragged node sits exactly under the cursor.
function nodeAt(e: PointerEvent): { x: number; y: number } {
  const g = gRef.value
  const svg = svgRef.value
  if (!g || !svg) return { x: 0, y: 0 }
  const ctm = g.getScreenCTM()
  if (!ctm) return { x: 0, y: 0 }
  const pt = svg.createSVGPoint()
  pt.x = e.clientX
  pt.y = e.clientY
  const p = pt.matrixTransform(ctm.inverse())
  return { x: p.x, y: p.y }
}
function startDrag(node: SimNode, e: PointerEvent) {
  dragging = node
  didDrag = false
  ;(e.target as Element).setPointerCapture?.(e.pointerId)
  sim?.alphaTarget(0.3).restart()
  e.stopPropagation() // don't start a background pan
}
function onPointerMove(e: PointerEvent) {
  if (!dragging) return
  const p = nodeAt(e)
  dragging.fx = p.x
  dragging.fy = p.y
  didDrag = true
}
function endDrag() {
  if (!dragging) return
  dragging.fx = null
  dragging.fy = null
  dragging = null
  sim?.alphaTarget(0)
}
function openNode(node: SimNode) {
  if (didDrag) {
    didDrag = false
    return // it was a drag, not a click
  }
  if (node.address !== null) router.push(`/adr/${node.address}`)
}

function compactId(refStr: string): string {
  const s = refStr.replace(/^ADR-/, '')
  return s.length > 8 ? s.slice(0, 8) : s
}
function truncate(s: string, n = 22): string {
  return s.length > n ? `${s.slice(0, n - 1)}…` : s
}

onMounted(() => {
  build()
  // Zoom + pan on the whole svg; the transform drives the inner <g>.
  if (svgRef.value) {
    const z = zoom<SVGSVGElement, unknown>()
      .scaleExtent([0.2, 4])
      .on('zoom', (ev: D3ZoomEvent<SVGSVGElement, unknown>) => {
        transform.value = { x: ev.transform.x, y: ev.transform.y, k: ev.transform.k }
      })
    select(svgRef.value).call(z)
  }
})
onBeforeUnmount(() => sim?.stop())
watch(
  () => props.graph,
  () => build(),
)
</script>

<template>
  <div>
    <svg
      ref="svgRef"
      :viewBox="`0 0 ${W} ${H}`"
      class="graph"
      @pointermove="onPointerMove"
      @pointerup="endDrag"
      @pointerleave="endDrag"
    >
      <defs>
        <marker
          v-for="k in KINDS.filter((x) => x.directed)"
          :id="`arrow-${k.kind}`"
          :key="k.kind"
          viewBox="0 0 10 10"
          refX="18"
          refY="5"
          markerWidth="6"
          markerHeight="6"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" :style="{ fill: edgeColor(k.kind) }" />
        </marker>
      </defs>

      <g ref="gRef" :transform="transformStr">
        <line
          v-for="(e, i) in view.links"
          :key="`e${i}`"
          :x1="e.x1"
          :y1="e.y1"
          :x2="e.x2"
          :y2="e.y2"
          class="edge"
          :class="{ dashed: e.kind === 'related' || e.kind === 'relates_to' }"
          :style="{ stroke: edgeColor(e.kind) }"
          :marker-end="isDirected(e.kind) ? `url(#arrow-${e.kind})` : undefined"
        />

        <g
          v-for="n in view.nodes"
          :key="n.ref"
          class="node"
          :class="{ clickable: n.address !== null }"
          @pointerdown="startDrag(n, $event)"
          @click="openNode(n)"
        >
          <circle
            :cx="n.x ?? 0"
            :cy="n.y ?? 0"
            :r="NODE_R"
            class="node-circle"
            :style="{ fill: statusColor(n.status) }"
          />
          <text :x="n.x ?? 0" :y="(n.y ?? 0) - NODE_R - 5" class="node-label">
            {{ compactId(n.ref) }}
          </text>
          <title>{{ n.ref }} — {{ truncate(n.title, 60) }} ({{ n.status }})</title>
        </g>
      </g>
    </svg>

    <!-- Legend doubles as edge-kind filters -->
    <div class="legend">
      <button
        v-for="k in KINDS"
        :key="k.kind"
        type="button"
        class="legend-item"
        :class="{ off: !visibleKinds.has(k.kind) }"
        @click="toggleKind(k.kind)"
      >
        <span class="swatch" :style="{ background: edgeColor(k.kind) }" />
        {{ k.label }}
      </button>
      <span class="hint">drag to arrange · scroll to zoom · click a node to open</span>
    </div>
  </div>
</template>

<style scoped>
.graph {
  width: 100%;
  height: auto;
  touch-action: none;
  cursor: grab;
}
.graph:active {
  cursor: grabbing;
}
.edge {
  stroke-width: 1.6;
  opacity: 0.75;
}
.edge.dashed {
  stroke-dasharray: 4 3;
  opacity: 0.55;
}
.node {
  cursor: default;
}
.node.clickable {
  cursor: pointer;
}
.node-circle {
  stroke: var(--ad-bg-elevated-solid);
  stroke-width: 2;
  transition: stroke 0.15s ease;
}
.node.clickable:hover .node-circle {
  stroke: var(--color-brand-500);
  stroke-width: 3;
}
.node-label {
  font-size: 9px;
  font-family: ui-monospace, monospace;
  fill: var(--ad-text-muted);
  text-anchor: middle;
  pointer-events: none;
  user-select: none;
}
.legend {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem 0.75rem;
  align-items: center;
  margin-top: 0.5rem;
}
.legend-item {
  display: inline-flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 0.75rem;
  color: var(--ad-text-muted);
  background: none;
  border: none;
  cursor: pointer;
  padding: 0;
}
.legend-item.off {
  opacity: 0.35;
  text-decoration: line-through;
}
.swatch {
  width: 0.85rem;
  height: 0.2rem;
  border-radius: 1px;
  display: inline-block;
}
.hint {
  margin-left: auto;
  font-size: 0.7rem;
  color: var(--ad-text-muted);
  opacity: 0.7;
}
</style>
