# Rust research service: architecture proposal

Prepared for Robert Guss · September 14, 2026 · Design for discussion, not an
implemented system

## 1. Decision summary

Build a **single-user, API-first research application in Rust**, with a thin
Rust CLI. Hermes, Claude Code, Codex, and other harnesses are peers: none owns
the application or its data.

The service owns **jobs, sources, public PDF acquisition/parsing, evidence,
reports, exports, and notifications**. An external agent or hosted research API
owns investigation strategy and synthesis. The service enforces limits and
validates artifacts; it does not initially implement another general-purpose
reasoning loop.

Recommended first deployment:

| Component                         | Choice                                                                  |
| --------------------------------- | ----------------------------------------------------------------------- |
| HTTP API                          | Axum, Tokio, Serde, tracing                                             |
| Provider HTTP clients             | reqwest; only integrations actually needed                              |
| Persistence                       | SQLite on local persistent storage, accessed through SQLx               |
| Files                             | Private content-addressed source storage and versioned report artifacts |
| CLI                               | Rust/clap; shared protocol types, no duplicated business logic          |
| Durable execution                 | One worker process with bounded task queues; database-backed claims     |
| Source PDF parsing                | Pinned Python/Docling executable in a subprocess                        |
| PDF report export                 | Pinned Markdown → HTML → PDF renderer in a separate subprocess          |
| General retrieval                 | Exa search and contents                                                 |
| Academic retrieval                | arXiv adapter in v1; OpenAlex next                                      |
| First unattended research backend | Exa Agent, conditional on a small contract/quality test                 |
| Additional execution route        | One official subscription-authenticated agent CLI, then the second      |
| Notifications                     | Telegram completion, failure, and action-needed messages only           |

Exa Agent is a proposed first backend because the account already exists and the
documented API offers asynchronous research and fixed effort levels. It is
**separately billed** and is not assumed equivalent to Perplexity. If its output
contract or quality is unsuitable, use Parallel or You.com instead. Do not
implement all three speculatively.

This supersedes the earlier standalone-CLI and Hermes-centered suggestions. The
prior landscape report remains useful background, not the current architecture
decision.

## 2. Architecture and ownership

```diagram
┌──────────────────────────────────────────────────┐
│ Hermes · Claude Code · Codex · other clients       │
└────────────────────────┬─────────────────────────┘
                         │ CLI or HTTP
                         ▼
┌──────────────────────────────────────────────────┐
│ Rust API                                          │
│ authentication · validation · retrieval · job API │
└───────────────┬──────────────────┬───────────────┘
                │                  │
                ▼                  ▼
┌────────────────────────┐  ┌──────────────────────┐
│ SQLite                 │  │ Private file storage │
│ jobs · sources ·       │  │ PDFs · extracts ·    │
│ evidence · reports     │  │ report versions      │
└────────────┬───────────┘  └───────────▲──────────┘
             │ durable claims         │ artifacts
             ▼                        │
┌─────────────────────────────────────┴────────────┐
│ Rust worker                                       │
│ research execution · parsing · export · notify   │
└───────────┬──────────────────┬────────────────────┘
            │                  │
            ▼                  ▼
┌─────────────────────┐  ┌─────────────────────────┐
│ Hosted research API │  │ Isolated subprocesses   │
│ or official CLI     │  │ Docling · PDF renderer  │
└─────────────────────┘  └─────────────────────────┘
```

This is one application, not a microservice platform. Build one server
executable with `serve` and `worker` modes, run as two supervised processes.
This allows API restarts without automatically terminating research workers and
gives each process separate resource limits. The agent CLI and Python parser
remain separately managed children.

The service is the system of record. Agent transcripts, provider dashboards, and
Telegram messages are not authoritative job state.

### Three workflows

1. **Tool use:** an active agent calls search/read/paper endpoints, obtains
   source IDs, and continues reasoning in its existing session. No second agent
   is launched.
2. **Client-led research:** an agent creates a run, attaches retrieved evidence,
   and submits a report. The service validates and exports it. A lost client can
   reconnect to the stored run, but the service cannot automatically resume that
   client's reasoning.
3. **Service-managed research:** submit a prompt and explicit backend; the
   worker delegates investigation, persists results, validates, exports, and
   notifies. The client may disconnect immediately.

All three share source and report formats. A quick lookup does not need a
research run or a background job.

## 3. Proposed API and CLI

Names are illustrative, not existing commands or a committed product name. HTTP
is the source of truth; the CLI is a small client.

| Operation                   | HTTP                                      | CLI example                                                    |
| --------------------------- | ----------------------------------------- | -------------------------------------------------------------- |
| Web search                  | `POST /v1/search`                         | `research search "topic" --json`                               |
| Academic discovery          | `POST /v1/papers/search`                  | `research papers search "topic" --provider arxiv --json`       |
| Read public URL             | `POST /v1/sources`                        | `research read URL --json`                                     |
| Inspect source/extraction   | `GET /v1/sources/{id}`                    | `research source SOURCE_ID --json`                             |
| Start/create a research run | `POST /v1/runs`                           | `research run "question" --backend exa-agent --depth standard` |
| Inspect/follow run          | `GET /v1/runs/{id}` and `/events`         | `research status RUN_ID --json`; `research watch RUN_ID`       |
| Request cancellation        | `POST /v1/runs/{id}/cancel`               | `research cancel RUN_ID`                                       |
| Continue recoverable work   | `POST /v1/runs/{id}/resume`               | `research resume RUN_ID`                                       |
| Submit client-led report    | `POST /v1/runs/{id}/report`               | `research report submit RUN_ID --file report.json`             |
| Download report format      | `GET /v1/reports/{id}/artifacts/{format}` | `research report download REPORT_ID --format pdf`              |

`POST /sources` may return a ready cached source, or `202 Accepted` with a job
ID for downloading/parsing. It should not keep an HTTP connection open while OCR
runs. The CLI can offer `--wait`, but submission without it returns promptly.

Persist monotonically ordered event IDs for reconnectable progress. Start with
cursor-based HTTP polling; SSE can be added without changing event semantics. A
progress stream closing does not mean a job completed—read authoritative status.

Use JSON on stdout for `--json`, diagnostics on stderr, explicit exit codes, and
`Idempotency-Key` on submissions. The same key and body returns the same run;
the same key with a different body is a conflict. Errors have stable codes,
retryability, and a request ID, not raw provider payloads or credentials.

`--depth` expresses intent; `--max-duration` and `--max-cost` express limits.
Backend selection must be explicit or come from a visible configured
default—never silently choose separately billed reasoning.

## 4. Data model

Keep the first schema small and relational. Large extracts and documents are
files, not database blobs.

| Record            | Essential contents                                                                                                 |
| ----------------- | ------------------------------------------------------------------------------------------------------------------ |
| `runs`            | Prompt, mode (`client`/`managed`), backend, depth, limits, status, assumptions, timestamps                         |
| `jobs`            | Run/source reference, task kind, state, attempt, lease owner/expiry, retry time, external task ID, error           |
| `sources`         | Canonical URL, title, DOI/arXiv metadata where available                                                           |
| `source_versions` | Source ID, actual retrieval time, final URL, content hash/path, access level, extraction version/options, warnings |
| `run_sources`     | Run, source version, discovery provenance/query, inclusion or exclusion reason                                     |
| `evidence`        | Run, source version, element/page/section locator, quoted text, verification state                                 |
| `reports`         | Run, immutable revision, structured content path, validation results, artifact manifest                            |
| `events`          | Run/job ID, sequence, type, timestamp, bounded structured payload                                                  |

Task kinds initially cover research, fetch/parse, export, and notification. They
use the same durable executor rather than four independent queue systems.

Do not overwrite a source when a page changes. Create a new version. Preserve
the difference between **when we fetched a provider response** and any
**crawl/publication date supplied by the provider**. A fresh API call does not
prove fresh webpage content.

Source access level is explicit: `metadata_only`, `snippet`, `partial_text`, or
`full_text`. A cited URL imported from a hosted report remains metadata-only
until acquired. Fetching it afterward is our own verification step, not proof
that the provider read that exact version.

## 5. Evidence and report contract

Use a versioned `report.json` as the canonical report representation. It
contains a summary, ordered sections/blocks, comparison tables, recommendations,
limitations, and references to evidence/source IDs. Markdown and PDF are derived
views of that same revision.

Keep schema flexibility where research needs it: optional comparison tables,
paper metadata, and recommendations. Do not force every task into a
vendor-comparison template. Unknowns are null/explicitly unavailable, not
invented defaults.

Preserve provider-native output unchanged alongside the normalized report. A
hosted API may return Markdown and citations without detailed evidence. Do not
fill the gap by fabricating quotations, page locations, or a complete search
history.

Validation has separate layers:

- **Structural:** schema is valid, IDs resolve, files exist, hashes agree.
- **Mechanical evidence:** quotation occurs in the recorded extract; PDF locator
  refers to the expected page/element. Record any text normalization used in
  matching.
- **Semantic review:** does the evidence actually support the claim, with its
  qualifications and context? An agent or human performs this; a substring check
  cannot establish it.

Publish statuses honestly: a report can be structurally valid but not
semantically reviewed. Tables must be checked as well as prose. Two models
agreeing is not independent source corroboration.

## 6. Job lifecycle and recovery

```diagram
┌────────┐    ┌─────────┐    ┌───────────┐
│ queued │───▶│ running │───▶│ succeeded │
└───▲────┘    └────┬────┘    └───────────┘
    │              ├───────▶ failed
 retry             ├───────▶ blocked
    │              ├───────▶ cancelled
┌───┴────────┐     └───────▶ unknown
│ retry_wait │◀── retryable failure
└────────────┘
```

`blocked` means action such as reauthentication or a spending decision is
needed. `unknown` means an external action may have happened but cannot yet be
reconciled. Neither is silently treated as failure eligible for blind
resubmission.

Rules:

1. Claim a job atomically with an attempt token and lease. Heartbeat while
   working; require the same token to commit completion so a stale worker cannot
   publish after another attempt takes ownership.
2. Persist stages and external task IDs. Poll an accepted hosted task rather
   than submitting a new one after restart.
3. There is an unavoidable submission ambiguity if the provider accepts a task
   just before our process dies. Use provider idempotency/reconciliation where
   supported; otherwise mark unknown and require resolution. Do not promise
   exactly-once external execution.
4. Retry safe retrieval/parsing with backoff. Write temporary artifacts and
   publish only after successful validation. Filesystem rename and SQLite commit
   are not one transaction: reference completed immutable files in the DB, and
   clean orphan files later.
5. Cancellation stops local work and requests provider cancellation where
   supported. It does not imply provider charges are reversed or remote
   computation stopped.
6. Expired leases do not themselves prove a child process stopped.
   Reconcile/terminate tracked processes before reassigning expensive work.
7. `resume` reuses compatible evidence/checkpoints and creates another attempt;
   it does not promise exact replay of an agent's internal state. Use a stored
   session ID only where the official CLI supports it.

SQLite starts in WAL mode with short transactions, a busy timeout, and no
network calls while holding write locks. Keep the DB on a local persistent
filesystem. Back it up consistently with the artifact store and test restoring
it. PostgreSQL becomes appropriate if multiple VMs or write contention justify
it; migration is real work, not a free SQLx switch.

### Queue implementation decision

Evaluate pinned **Apalis + SQLite** against these recovery requirements before
building a custom queue. Prefer it if its task lifecycle fits without
maintaining a second competing state machine. A bespoke SQLx lease executor is
the fallback, not an assumed shortcut: claims, crash windows, fencing,
cancellation and retries need explicit tests either way. No Redis in v1.

## 7. PDF ingestion and worker isolation

Use Docling for the first rich parser, not a cascade of three untested parsers.
Prefer accessible XML/HTML full text when it preserves the information needed,
but keep academic metadata separate from full-text acquisition.

Parser contract:

- Input: service-assigned file path, source hash, output directory, bounded
  options.
- Output files: versioned document JSON, Markdown, optional table/image
  artifacts, parser/version/options metadata, page-level warnings.
- Stdout: small completion manifest only. Stderr: bounded diagnostics.

The JSON preserves page numbers, coordinate conventions, element IDs and
bounding boxes when provided. Define locators against a particular extraction
version; re-parsing must not silently change the meaning of old citations.
Parser reading-order confidence is not scientific confidence.

Run one parser at a time initially. Prefetch pinned model assets during
deployment. Enforce download/page/time limits and OS-level memory/CPU limits;
spawning a subprocess alone does not impose a memory limit. Disable parser
network access where practical after model provisioning.

Use argument arrays, never shell interpolation. Drain or redirect both output
streams, terminate the process group on cancellation, and reap children. Tokio
does not kill a child merely because its future is dropped. Preserve enough
diagnostics to distinguish unsupported input, OOM, timeout, and extraction
failure.

Short-lived Docling processes trade model cold-start latency for released idle
memory. Only introduce a persistent Python service if measurements justify its
always-resident memory. Do not add FastAPI just to run a parser.

Report export is separate from source parsing. Select a pinned renderer after
testing wide tables, page breaks, references and fonts. A Chromium-based
renderer is a practical baseline but also needs its own memory limit and
sanitized HTML with external networking disabled.

## 8. Research execution and budgets

### Hosted backend

Implement submit, inspect/poll, retrieve results, and cancellation if actually
supported. Store raw output and honest provenance. Backend capability
differences remain visible: not every backend supports hard deadlines, cost
ceilings, cancellation, or structured reports.

### Official agent CLI worker

Later add one of Claude Code/Codex, authenticated through its own supported
flow. Give it a dedicated job workspace and a run-scoped service credential. It
calls the research CLI for retrieval and submits the final report to the API.

The worker can read approved sources and write its report, but cannot administer
providers, inspect unrelated jobs, expose credentials, or recursively create
managed research jobs. Shared Exa and Telegram secrets stay in the service, not
the child environment. The model CLI's own credential is necessarily a separate
security consideration; use its sandbox and isolate it from parsing/rendering.

Fail visibly on authentication or quota exhaustion. Do not switch accounts or
billing modes to evade limits. Hosted API spending remains explicit. Verify
subscription behavior with the selected CLI version before promising it as a
supported execution mode.

### Initial depth settings

- Lookup: no managed investigation unless requested.
- Standard: target 5–10 minutes.
- Deep: target around 20 minutes.
- Extended: explicit longer budget.

These are targets, not guarantees. Reserve time for report publication and
return marked partial results when appropriate. Local deadlines can stop our
worker, but remote spending can only be bounded as strongly as the provider
allows. Record estimated and provider-reported costs separately; unreported cost
is unknown, not zero.

## 9. Access, notifications, and observability

Single owner, private service. Start with bearer authentication over a private
connection/SSH tunnel or HTTPS reverse proxy. Do not expose raw worker
endpoints. Keep configured provider credentials out of logs, DB exports, CLI
output and report artifacts.

Protect URL acquisition against SSRF: validate schemes and destinations, reject
private/link-local/metadata addresses, recheck redirects and actual connections,
and set response-size/time limits. Apply an egress policy, not just a string
check. Treat all fetched content as untrusted data, including instructions
embedded in webpages/PDFs.

Telegram is notification-only. Publish a report before enqueuing its
notification. Delivery failure must not rerun research. Retry definite failures;
an ambiguous network result may have delivered already, so record unknown rather
than promise exactly-once messages. Default notices contain a job ID and status;
including a potentially sensitive research title is configurable.

Record structured run/job/request IDs, provider timing, estimated/reported
spend, extraction failures, resource-limit events, and queue delay. Do not log
complete source content or credentials by default. Store timestamps in UTC and
display them in America/New_York when requested by the client.

## 10. First-version scope and delivery order

**Phase 0 — short feasibility checks, no platform build yet**

- One real Exa Agent task: check result fields, async lifecycle, cost reporting
  and report usefulness. A paid call requires explicit authorization.
- Docling on a small representative public-PDF set: ordinary text, multi-column
  paper, table-heavy document and a scan. Measure memory and cold/warm elapsed
  time on the target VM.
- Test Apalis/SQLite crash recovery against the needed semantics; choose queue
  implementation.
- Test the report renderer on a table-heavy sample resembling the homeschool
  report.

**Phase 1 — useful shared tools**

- Rust API/CLI, authentication, DB migrations, artifact storage.
- Exa search/contents, public URL ingestion and bounded PDF parsing.
- arXiv discovery with its documented global request pacing.
- Source inspection and machine-readable results usable by all harnesses.

**Phase 2 — complete first release**

- Managed jobs using one hosted backend, plus client-led report submission.
- Status/events, cancellation, recovery and explicit partial/blocked states.
- Canonical JSON, Markdown/PDF exports and Telegram notifications.
- Basic cost limits, backups, restore instructions, health checks.

**Phase 3 — subscription execution and expansion**

- One official CLI backend, then the second after the first proves useful.
- OpenAlex, alternate retrieval or hosted research backends when evaluation
  shows a benefit.
- Optional semantic citation review, MCP adapter and web UI.

Not in v1: multi-user accounts, public hosting, login-gated scraping,
distributed workers, vector database, autonomous multi-agent debates, whole-web
crawling, scheduled monitoring, or a new general-purpose agent framework.

## 11. Acceptance criteria

- Hermes, Claude Code and Codex can call the same CLI without agent-specific
  server branches.
- Disconnecting a client does not lose an accepted managed job.
- Duplicate submissions return the same run; ambiguous provider submissions do
  not trigger blind paid duplicates.
- Restart during fetch, parsing, export and remote polling preserves honest
  state and recoverable outputs; stale workers cannot publish a newer attempt's
  result.
- A scanned page or bad table is not silently reported as a successful full-text
  extraction.
- Every report citation resolves to a registered source; quoted evidence
  resolves to its exact source version. Missing provenance is visible.
- JSON/Markdown/PDF represent one report revision; wide tables and references
  are visually inspected.
- Parser OOM/timeouts do not take down the API; cancellation leaves no untracked
  child processes.
- Telegram failure does not affect report availability or cause repeated
  research.
- Same-prompt evaluation against your Perplexity example and several other task
  types measures quality, elapsed time, retrieval spend, and agent quota impact.
  No claim of parity before that comparison.

## References and remaining uncertainty

- [Original landscape comparison](research-tool-landscape.md)
- [Axum](https://github.com/tokio-rs/axum),
  [SQLx](https://github.com/launchbadge/sqlx),
  [Apalis](https://github.com/apalis-dev/apalis)
- [Tokio subprocess behavior](https://docs.rs/tokio/latest/tokio/process/index.html)
- [Docling](https://github.com/docling-project/docling),
  [model provisioning/options](https://github.com/docling-project/docling/blob/main/docs/usage/advanced_options.md)
- [Exa pricing](https://exa.ai/docs/reference/pricing),
  [Parallel pricing](https://docs.parallel.ai/resources/pricing),
  [You.com Research](https://you.com/docs/research/overview)
- [Claude Code programmatic use](https://code.claude.com/docs/en/headless),
  [authentication](https://code.claude.com/docs/en/authentication),
  [credential rules](https://code.claude.com/docs/en/legal-and-compliance)
- [Codex noninteractive execution](https://developers.openai.com/codex/noninteractive)
- [arXiv API terms and pacing](https://info.arxiv.org/help/api/tou.html)

Framework/library capabilities were researched earlier in this conversation. No
implementation, runtime benchmark, authenticated provider test, target-VM
inspection, or deployment has been performed for this proposal. The exact VM
memory budget, first backend contract, queue version and PDF renderer remain
validation choices—not assumed facts.
