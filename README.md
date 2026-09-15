# Rust Researcher

An API-first research application with a thin CLI, designed for use by Hermes,
Claude Code, Codex, and other agents or harnesses.

The service will own research jobs, sources, evidence, public PDF parsing,
reports, exports, Telegram notifications, and the research quality policy.
Investigations can be delegated to external agents or hosted research APIs; the
service decides what a run must contain before its report is labelled anything
other than a draft.

## Status

Design and research only. No application has been implemented, deployed, or
benchmarked yet. The design was reviewed on September 14, 2026 and revised the
same day; see the design review for what changed and why.

## Documents

Start with the architecture proposal. The companion documents carry detail it
points to.

- **[Rust architecture proposal](docs/rust-research-architecture.md)** — the
  current direction: ownership, API/CLI, data model, report contract, job
  lifecycle, worker isolation, budgets, access, delivery order, acceptance
  criteria.
- **[Research quality protocol](docs/research-quality-protocol.md)** — the
  research brief, standing user context, clarification policy, execution
  protocol, material claims, review levels, Draft/Reviewed labels, follow-up
  runs.
- **[Evaluation harness](docs/evaluation-harness.md)** — golden set, rubric,
  hard gates, the backend comparison that chooses the first execution route,
  regression runs.
- **[Run, report, and backend contracts](docs/run-and-report-contracts.md)** —
  run state dimensions, blocked reasons and reconciliation, backend capability
  contract, budget semantics, worker lanes, the `report.json` envelope, added
  API operations, schema evolution.
- **[Source acquisition policy](docs/source-acquisition-policy.md)** —
  acquisition ladder for web pages, freshness classes, source identity and
  origin groups, cache keys, robots and terms, SSRF.
- **[Operations runbook](docs/operations-runbook.md)** — exe.dev layout,
  supervision and upgrades, worker and child-process isolation, backup, restore
  and reconciliation mode, phone workflow, operator view, testing strategy, and
  the privacy, classification, and outbound policy.
- **[Design review, September 14, 2026](docs/design-review-2026-09-14.md)** —
  what the review found, what was kept, what changed, what a second independent
  review added, and the assumptions that remain unvalidated.
- **[Decision records](docs/decisions.md)** — each expensive-to-reverse choice
  with its alternatives, evidence, reversal condition, and status; pending
  decisions with dates.
- **[Research tool landscape](docs/research-tool-landscape.md)** — supporting
  comparisons of open-source projects, search APIs, academic retrieval, and
  hosted research services. Its CLI-first and subscription-first recommendations
  are superseded; see its status note.

## Proposed stack

- Rust with Axum and Tokio for the API and worker, as a maintainer preference.
- SQLite and SQLx for persistent records.
- A thin Rust CLI sharing protocol types with the service.
- Isolated Python/Docling subprocesses for rich PDF parsing, scheduled after the
  first slice.
- Exa retrieval behind an acquisition ladder, with replaceable research backends
  chosen by evaluation.
- A versioned `report.json` envelope around one Markdown body, with HTML and PDF
  exports and a Draft/Reviewed label on every format.
- A read-only report page behind exe.dev's private proxy, linked from Telegram.

## Next step

Phase 0 of the architecture proposal, with the decisions in the decision records
already taken: write the rubric and golden cases, score the incumbent Perplexity
report, build the thin evidence loop, verify Exa's capabilities against the real
account, run the backend comparison between Exa Agent and one official CLI under
an authorised spending ceiling, measure the renderer, Docling, and mixed load on
the target VM, and choose the queue implementation. Then build one vertical
slice to the phone and pilot it on real work before cancelling anything.
