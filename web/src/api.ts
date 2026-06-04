// Small typed client over the read-only `/api/*` JSON endpoints exposed by
// `src/serve.rs`. Mirrors the serde view types in `src/view.rs` exactly.

// Status serializes as PascalCase (serde default on the Rust enum).
export type Status = 'Proposed' | 'Accepted' | 'Rejected' | 'Deprecated' | 'Superseded'

// EdgeKind serializes snake_case (see view.rs).
export type EdgeKind = 'supersedes' | 'depends_on' | 'refines' | 'relates_to' | 'related'

export interface AdrSummary {
  number: number | null
  number_display: string
  // Display id ("ADR-0006" / a date slug / "ADR-<short-uuid>").
  reference: string
  // Routing/addressing token (the value `/api/adrs/:id` accepts).
  address: string
  title: string
  status: Status
  created: string | null
  // Display references of superseded / superseding ADRs.
  supersedes: string[]
  superseded_by: string | null
  review_due: boolean
}

// Live forge state for an ADR (issue/PR links + PR review state), served by the
// opt-in read-only `/api/adrs/:id/forge` endpoint. Every field is optional —
// the endpoint returns `null` (→ no panel) when no provider is configured.
export interface ForgeData {
  issue_url?: string
  pr_url?: string
  pr_approvals?: number
  pr_ci?: string
  pr_merged?: boolean
}

export interface RelatedLink {
  reference: string
  address: string
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
  reference: string
  address: string
  title: string
  age_days: number | null
  // True when this Proposed ADR is also flagged review-due (past deadline or stale).
  review_due: boolean
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
  // Display id, also the key edges reference.
  reference: string
  // Routing token, or null for an unassigned ADR.
  address: string | null
  title: string
  status: Status
}

export interface GraphEdge {
  // Endpoints reference nodes by their `reference`.
  from: string
  to: string
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

export function getAdr(id: string): Promise<AdrDetail> {
  return getJson<AdrDetail>(`/api/adrs/${encodeURIComponent(id)}`)
}

// Live forge state for one ADR, or `null` when no provider is configured (or
// the build lacks the `forge` feature, or the ADR has no linked issue/PR).
export function getAdrForge(id: string): Promise<ForgeData | null> {
  return getJson<ForgeData | null>(`/api/adrs/${encodeURIComponent(id)}/forge`)
}

// Aggregate forge counts for the dashboard tiles, or `null` when there's no
// active forge (unconfigured / no token / no `forge` feature).
export interface ForgeSummary {
  proposed_without_pr: number
  approved_unmerged: number
}

export function getForgeSummary(): Promise<ForgeSummary | null> {
  return getJson<ForgeSummary | null>('/api/forge/summary')
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

// ---- repo health / checks (mirrors `adroit check`) ----

export type Severity = 'error' | 'warning'

export type ProblemKind =
  | 'duplicate_id'
  | 'status_dir_mismatch'
  | 'unparseable'
  | 'broken_supersession'
  | 'broken_link'
  | 'stale_link'

export interface ProblemFile {
  path: string
  lines: number
  bytes: number
}

export interface Problem {
  severity: Severity
  kind: ProblemKind
  // Headline id: an ADR reference ("ADR-0009") for a duplicate, else the file path.
  label: string
  // Short description (no leading label, no path list).
  summary: string
  // Affected files with sizes (duplicates list every colliding file; else empty).
  paths: ProblemFile[]
  // Full one-line message (matches the `adroit check` CLI output).
  message: string
}

export interface CheckReport {
  // Number of ADR files inspected.
  checked: number
  // Problems found, sorted by severity (errors first) then message; empty when clean.
  problems: Problem[]
}

/** The repo-validation report (the same checks as `adroit check`). */
export function getCheck(): Promise<CheckReport> {
  return getJson<CheckReport>('/api/check')
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
