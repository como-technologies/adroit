# The Adopt Read Slice (Dogfood)

The portfolio's **Adopt**-stage agent engine (Conduit) consumes adroit's
decisions through a narrow, fully machine-readable read slice — discover the
tool via `manifest`, enumerate accepted decisions, read one decision, read its
stored implementation plan. This page records the **dogfood rehearsal** that
proves that exact slice end to end on a live local model: not a feature, a
verification — run the slice the way the downstream engine will, measure it,
and fix any friction at root cause.

The slice has a write side (how the corpus gets into the consumable state) and
the read side (what the Adopt engine actually issues):

```sh
# Write side — assessment → accepted, planned decision (AI on local ollama)
adroit import --from-assessment export.yaml --ai -o json   # seed Proposed ADRs, AI-fleshed
adroit lint 1 -o json                                      # authoring gate (mechanical)
adroit set-status 1 accepted
adroit plan 1 --save                                       # persist the plan (ADR-0008)

# Read side — the conduit-shaped reads (all -o json, all provider-free)
adroit manifest -o json                                    # tool discovery + schemas
adroit list --status accepted -o json                      # the decision backlog
adroit show 1 -o json                                      # one decision (carries `plan`)
adroit plan 1 -o json                                      # the stored plan, deterministic
```

Re-run it any time with **`just adopt-slice`** — a temp-corpus rehearsal recipe,
gated on a local ollama actually listening (it skips cleanly otherwise, and is
deliberately *not* part of `just ci`: it makes live model calls).

## Rehearsal evidence

Run 2026-06-12 — debug build, `ADROIT_AI_PROVIDER=ollama`,
`ADROIT_AI_MODEL=llama3.2`, fresh temp corpus, the vendored
[golden assessment fixture](./testing.md#the-test-layers)
(`tests/fixtures/golden-assessment.yaml`, one practice) as input:

| Step | Result | Time |
|---|---|---|
| `import --from-assessment … --ai -o json` | seeded `ADR-0001` (`Proposed`, domain `Delivery`); pure JSON summary on stdout, provider chatter on stderr | 28.2s |
| `lint 1 -o json` | `[]` — the AI-fleshed body passes the mechanical authoring gate | 0.03s |
| `set-status 1 accepted` | file moved `proposed/` → `accepted/` | 0.02s |
| `plan 1 --save` | plan persisted as the `<!-- adroit:plan -->`-marked `## Implementation` section | 26.3s |
| `manifest -o json` | `tool: "adroit"`, `manifest_schema: 1`, 38 commands | 0.01s |
| `list --status accepted -o json` | exactly one row, `status: "Accepted"` | 0.01s |
| `show 1 -o json` | full detail; `plan` field carries the stored plan (1,935 chars) | 0.01s |
| `plan 1 -o json` (twice) | **byte-identical** (`sha256 9807e7f1…` both runs), `stored: true` — a pure corpus read, no provider call | 0.01s each |

The shape of the slice held: model time dominates the two provider calls
(~27–30s each on a small local model), every read is a local millisecond-scale
operation, and the stored-plan read is deterministic across invocations — the
property the Adopt engine's snapshot/ingest relies on.

## What the rehearsal caught (fixed at root cause)

The first run broke the slice at `plan --save` — exactly the class of friction
a rehearsal exists to find:

1. **`import --ai` could block `plan --save` forever.** The import flesh-out
   instruction asked the model for "a short implementation outline"; llama3.2
   reasonably wrote it under a bare `## Implementation` heading, which then
   read as a *hand-written* section — the one thing `plan --save` refuses to
   manage (ADR-0008). adroit set the trap itself. Fixed twice over: the
   instruction now directs the outline to `## Implementation notes`, and every
   AI draft is mechanically **sanitized** before the splice — an unmanaged
   `## Implementation` heading with real content is retitled, so no model
   output can squat on the plan seam (prompt compliance is hope; the sanitizer
   is the guarantee). Pinned by the
   `plan_save_succeeds_after_an_ai_import_drafted_an_implementation_outline`
   CLI regression and the `ai_drafts_never_block_plan_save` property
   (arbitrary model output never reads as a hand-written plan section).
2. **The re-emitted H1.** Despite the prompt forbidding it, llama3.2 re-emits
   the `# ADR-NNNN: Title` heading at the top of its draft (already noted in
   the M4 ingest report); the splice preserves the mechanical heading, so the
   body carried a duplicate H1. The sanitizer now drops a *leading* re-emitted
   H1 (a later `# ` heading is the model's own prose and stays).
3. **`lint` false positive on `###`-recorded options.** The model recorded its
   two options as `### Option 1:` / `### Option 2:` sub-headings (MADR's long
   form does the same); `lint` only counted list items and flagged "fewer than
   two options" on a body that manifestly weighs two. The rule now counts list
   items *and* `###` sub-headings under `## Considered Options`.

After the fixes, the second run was green end to end — `lint` clean, the model
itself putting its outline under `## Implementation notes`.

## Small-model quirks observed

- llama3.2 re-emits **section headings the prompt forbids** — beyond the H1, it
  echoed a `## Stakeholders` section, including the template's italic prompt
  line verbatim, then filled it with invented placeholders (`[Your Name]`,
  `[Manager's Name]`). Initially recorded as harmless; the iteration-1
  full-loop run then showed the echo at scale (duplicate `## Status` /
  `## Stakeholders` blocks below the marker in ADR-0001/0005, chat residue
  trailing ADR-0002), so the sanitizer now drops skeleton-echo sections and
  echoed markers wherever they appear and strips trailing conversational
  closers — pinned by the run-1-shaped regressions in `src/ai/mod.rs` and the
  `ai_drafts_never_duplicate_the_mechanical_preamble` property. The
  iteration-2 full-loop run then surfaced the same filler *outside* an echoed
  skeleton: a **novel whole-line bracket placeholder** ("[Insert
  implementation plan or other details as needed]") closed a drafted body the
  template never contained it in, sailing past the known-scaffold rules — so
  the sanitizer now drops `[Insert …]` / `[Your Name]`-shaped placeholder
  lines (conservatively: links, checkboxes, citations, and fenced code never
  match) and `lint` warns on them, pinned by the run-2-shaped regression and
  the `ai_drafts_never_carry_bracket_placeholder_lines` property.
- It invents **structure inside sections**: a `### Continuous Integration`
  sub-heading inside the Context section, tab-indented `+` nested bullets.
  Valid GFM, cosmetic only.
- The generated **plan opens with its own heading**
  (`### Implementation Plan: …`) — fine: the stored section is
  end-marker-bracketed precisely so free-form plan markdown (own headings
  included) round-trips verbatim.
- Provider-call latency is the whole cost: ~28s to flesh one seed and ~26s to
  generate one plan on llama3.2, against ~10ms for every read in the slice.

## Relation to the test suites

The live rehearsal is the manual, evidence-producing end of seams that are all
pinned offline (see [Testing & fuzzing](./testing.md)): the golden-fixture
ingest contract, the `ADROIT_AI_FAKE`-driven import/draft/plan CLI tests (now
including the slice regression above), the `ai_drafts_never_block_plan_save`
property, and the env-gated `ADROIT_LIVE_OLLAMA=1` live import test. What only
the rehearsal proves is the *composition* on a real model: real prompts, real
small-model output shapes, real timings.
