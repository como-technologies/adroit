# Examples

Sample inputs for trying adroit without wiring up the rest of the portfolio.

- **`assessment.json` / `assessment.yaml` / `assessment.toml`** — one generic
  `Domain → Practice → Question` maturity assessment (the same content in each
  format, to show `adroit import` accepts all three). It seeds **4 proposed ADRs**.

  ```sh
  adroit import --from-assessment examples/assessment.json --dry-run   # preview
  adroit import --from-assessment examples/assessment.yaml             # seed
  adroit import --from-assessment examples/assessment.toml --ai        # seed + AI flesh-out
  ```

The full walkthrough — what `import` does, mechanical vs `--ai`, the seam it serves —
lives in the manual (the one doc system), not here:
[**The ADR Workflow → Seed a backlog from an assessment**](../docs/src/usage/workflow.md#seed-a-backlog-from-an-assessment--adroit-import).
