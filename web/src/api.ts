// Small typed client over the read-only `/api/*` JSON endpoints exposed by
// `src/serve.rs`. Mirrors the serde view types in `src/view.rs` exactly.

// Status serializes as PascalCase (serde default on the Rust enum).
export type Status = 'Proposed' | 'Accepted' | 'Rejected' | 'Deprecated' | 'Superseded'

// EdgeKind serializes snake_case (see view.rs).
export type EdgeKind = 'supersedes' | 'related'

export interface AdrSummary {
  number: number | null
  number_display: string
  title: string
  status: Status
  created: string | null
  supersedes: number[]
  superseded_by: number | null
  review_due: boolean
}

export interface RelatedLink {
  number: number
  kind: EdgeKind
}

// One git-derived lifecycle milestone (proposed → accepted/rejected/…).
export interface TimelineEvent {
  date: string
  status: Status
  label: string
  commit: string
  subject: string
}

// AdrDetail flattens the summary fields at the top level (serde flatten).
export interface AdrDetail extends AdrSummary {
  body: string
  body_html: string | null
  related: RelatedLink[]
  history: TimelineEvent[]
  last_modified: string | null
}

export interface StatusCount {
  status: Status
  count: number
}

export interface ProposedAge {
  number: number | null
  title: string
  age_days: number | null
}

export interface CreatedBucket {
  month: string
  count: number
}

export interface Stats {
  total: number
  by_status: StatusCount[]
  proposed_age: ProposedAge[]
  review_due: AdrSummary[]
  created_over_time: CreatedBucket[]
}

export interface GraphNode {
  number: number | null
  title: string
  status: Status
}

export interface GraphEdge {
  from: number
  to: number
  kind: EdgeKind
}

export interface Graph {
  nodes: GraphNode[]
  edges: GraphEdge[]
}

async function toError(resp: Response): Promise<Error> {
  let detail = ''
  try {
    const body = await resp.json()
    detail = body?.error ? `: ${body.error}` : ''
  } catch {
    /* ignore */
  }
  return new Error(`${resp.status} ${resp.statusText}${detail}`)
}

async function getJson<T>(url: string): Promise<T> {
  const resp = await fetch(url)
  if (!resp.ok) throw await toError(resp)
  return resp.json() as Promise<T>
}

async function postJson<T>(url: string, body: unknown): Promise<T> {
  const resp = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!resp.ok) throw await toError(resp)
  return resp.json() as Promise<T>
}

export function listAdrs(opts: { status?: string; sort?: string } = {}): Promise<AdrSummary[]> {
  const params = new URLSearchParams()
  if (opts.status) params.set('status', opts.status)
  if (opts.sort) params.set('sort', opts.sort)
  const qs = params.toString()
  return getJson<AdrSummary[]>(`/api/adrs${qs ? `?${qs}` : ''}`)
}

export function getAdr(number: number): Promise<AdrDetail> {
  return getJson<AdrDetail>(`/api/adrs/${number}`)
}

export function search(q: string): Promise<AdrSummary[]> {
  return getJson<AdrSummary[]>(`/api/search?q=${encodeURIComponent(q)}`)
}

export function getStats(): Promise<Stats> {
  return getJson<Stats>('/api/stats')
}

export function getGraph(): Promise<Graph> {
  return getJson<Graph>('/api/graph')
}

// ---- workspace / directory switching ----

export interface Workspace {
  dir: string
}

export interface BrowseEntry {
  name: string
  path: string
}

export interface BrowseListing {
  path: string
  parent: string | null
  entries: BrowseEntry[]
  adr_count: number
}

export interface SwitchResult {
  dir: string
  adr_count: number
}

/** The dashboard's currently active ADR directory. */
export function getWorkspace(): Promise<Workspace> {
  return getJson<Workspace>('/api/workspace')
}

/** List subdirectories of `path` (default: the active dir) for the picker. */
export function browseDir(path?: string): Promise<BrowseListing> {
  const qs = path ? `?path=${encodeURIComponent(path)}` : ''
  return getJson<BrowseListing>(`/api/browse${qs}`)
}

/** Switch the active ADR directory to `path`. */
export function switchWorkspace(path: string): Promise<SwitchResult> {
  return postJson<SwitchResult>('/api/workspace', { path })
}
