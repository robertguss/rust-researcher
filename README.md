# Rust Researcher

An API-first research application with a thin CLI, designed for use by Hermes,
Claude Code, Codex, and other agents or harnesses.

The service will own research jobs, sources, evidence, public PDF parsing,
reports, exports, Telegram notifications, and the research quality policy.
Investigations can be delegated to external agents or hosted research APIs; the
service decides what a run must contain before its report is labelled anything
other than a draft.

## Status

Phase 0's thin evidence loop is implemented: authenticated Exa search and
two-rung source acquisition, immutable acquisition/extraction artifacts, report
envelope import with mechanical evidence checks and service-computed labels,
spend reservation, and the thin `research` CLI. Phase 1 foundations include
versioned standing context, idempotent managed run submission, independent run
state dimensions, an attempt-epoch-fenced SQLx lease coordinator, explicit
ambiguous-submission reconciliation, and an Exa Agent worker that durably
collects provider results and reconciles cost. Immutable report revisions now
render as sanitized, responsive private HTML and cached PDF artifacts.
Notifications and deployment remain deferred. D-017 authorizes Exa-first
development while the full backend comparison remains deferred.

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

## Run the Phase 0 service

```sh
export RESEARCH_API_TOKEN='choose-a-long-random-token'
export EXA_API_KEY='from-project-secrets'
cargo run -p research-service

# In another shell:
RESEARCH_API_TOKEN="$RESEARCH_API_TOKEN" cargo run -p research-cli -- \
  search "Rust 1.98 release notes"

# Managed Exa run, followed by its service-labelled report:
RESEARCH_API_TOKEN="$RESEARCH_API_TOKEN" cargo run -p research-cli -- \
  run "Compare two options" --backend exa-agent --depth lookup --effort minimal
RESEARCH_API_TOKEN="$RESEARCH_API_TOKEN" cargo run -p research-cli -- \
  status RUN_ID
RESEARCH_API_TOKEN="$RESEARCH_API_TOKEN" cargo run -p research-cli -- \
  report REPORT_ID
RESEARCH_API_TOKEN="$RESEARCH_API_TOKEN" cargo run -p research-cli -- \
  review REPORT_ID review.json
RESEARCH_API_TOKEN="$RESEARCH_API_TOKEN" cargo run -p research-cli -- \
  download REPORT_ID html report.html
RESEARCH_API_TOKEN="$RESEARCH_API_TOKEN" cargo run -p research-cli -- \
  download REPORT_ID pdf report.pdf
```

`review.json` contains a reviewer identity plus claim IDs mapped to `supported`,
`qualified`, `unsupported`, `contradicted`, or `stale`. Review creates a new
immutable report revision; it never edits the draft in place.

The service defaults to `sqlite://research.db`, `./artifacts`, and
`127.0.0.1:3000`. Override these with `DATABASE_URL`, `ARTIFACT_ROOT`, and
`RESEARCH_LISTEN`; point the CLI elsewhere with `RESEARCH_API_URL`. Provider
calls spend real money. `cargo test` uses deterministic fakes and never calls
live Exa. `GET /v1/reports/{id}/artifacts/html` and
`GET /v1/reports/{id}/artifacts/pdf` serve authenticated artifacts. PDF export
requires `RESEARCH_CHROME_BIN` to name a pinned Chromium executable; generated
PDFs are content-addressed and cached against the immutable report revision.
`/r/{run_id}` is the human report page: without proxy configuration it requires
the bearer token; in deployment, set `RESEARCH_REPORT_PROXY_HOST` to the exact
private proxy host and the page requires that host in `X-Forwarded-Host`.

## Next step

Continue the Exa-first vertical slice under D-017 with notification delivery and
a real-work pilot. The backend comparison remains a quality-validation task
rather than a development gate.
