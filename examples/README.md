# Examples

Sample inputs for trying adroit's features without wiring up the real portfolio tools.

## `assessment.json` / `assessment.yaml`

A small, **generic** maturity assessment shaped like an
[`assessments`](https://github.com/como-technologies) export — a
`Domain → Practice → Question` model where each leaf carries *context*, *value*, and
*risk*. The three files are equivalent; they show that `import` accepts JSON, YAML,
or TOML.

Use them to try the **ingest seam** — seeding a proposed-ADR backlog from an
assessment ([issue #18](https://github.com/como-technologies/adroit/issues/18)):

```sh
# Preview what would be seeded (writes nothing):
adroit --dir /tmp/demo-adrs import --from-assessment examples/assessment.json --dry-run

# Seed for real — one proposed ADR per practice:
adroit --dir /tmp/demo-adrs import --from-assessment examples/assessment.yaml
adroit --dir /tmp/demo-adrs list
adroit --dir /tmp/demo-adrs show 1        # see a seeded ADR (context/drivers filled, rest prompts)
```

This assessment has **2 domains and 4 practices**, so it seeds **4 proposed ADRs**.
Each one's *context* becomes the problem statement, its *value* / *risk* / *effort*
become decision drivers, and its questions are recorded as assessment signals —
mechanically, with no AI or network. The body is marked
`<!-- adroit:seeded-from-assessment -->` and carries a provenance note back to the
source practice; you then refine it (`adroit draft <id>`, `edit`) before review.

`import` is re-runnable: a second run skips practices whose title already has an ADR,
so importing an *updated* assessment only adds what's new.
