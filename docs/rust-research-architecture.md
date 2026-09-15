# Rust research service: architecture proposal

Prepared for Robert Guss · September 14, 2026 · Design for discussion, not an
implemented system

Revised the same day after the [design review](design-review-2026-09-14.md). The
review found the reliability design sound and the delivery order wrong: plumbing
was scheduled before any evidence that the reports would be good. This document
keeps the architecture and changes the priorities. Companion documents carry the
detail:

- [Research quality protocol](research-quality-protocol.md): what a run must
  contain, the brief, standing user context, clarification, material claims,
  review levels, Draft/Reviewed labels, follow-up runs.
- [Evaluation harness](evaluation-harness.md): golden set, rubric, hard gates,
  the backend comparison that replaces the single Exa test, regression runs.
- [Run, report, and backend contracts](run-and-report-contracts.md): run state
  dimensions, reconciliation, capability contract, budget semantics, worker
  lanes, the report envelope, added API operations, schema evolution.
- [Source acquisition policy](source-acquisition-policy.md): acquisition ladder,
  freshness classes, source identity and origin groups, caching, terms.
- [Operations runbook](operations-runbook.md): exe.dev layout, supervision,
  CLI-worker isolation, backup and restore objectives, phone workflow, operator
  view, testing strategy.

## 1. Decision summary

Build a **single-user, API-first research application in Rust**, with a thin
Rust CLI. Hermes, Claude Code, Codex, and other harnesses are peers: none owns
the application or its data.

The product objective, ranked, so that every later trade-off has a tie-breaker
(accepted September 14, 2026,
[D-001](decisions.md#d-001-ranked-product-objective)):

1. **Trustworthy answers.** A report's claims are anchored to evidence Robert
   can inspect, its gaps are stated, and its label says how far it was checked.
2. **Reliable completion.** A submitted run finishes, fails visibly, or asks; it
   never disappears, and its state is always recoverable.
3. **Agent integration and a durable archive.** Any harness can submit, follow,
   and read; every run and its evidence remain findable and correctable years
   later.
4. **Cost control.** Spend is capped, reserved, reconciled, and visible.
5. **Presentation.** Reports read well on a phone and export cleanly.

When presentation and trust conflict (a confident nine-page report versus an
honest four-page one with stated gaps), trust wins. When cost and reliable
completion conflict (retrying a hosted task that may double-bill), the run
blocks and asks rather than guessing either way.

Two decisions in this summary are preferences, not technical necessities, and
are recorded as such so they can be revisited:

- **Rust** is the coordinator language because Robert prefers to maintain it.
  The heavy work is Python (Docling), a browser (PDF rendering), and an agent
  CLI; Rust orchestrates them and does not remove their failure modes. If
  iteration on prompts, adapters, and evaluation proves slow in Rust, Python is
  the fallback for the coordinator.
- **Hosted-first execution** reverses the landscape document's
  subscription-first recommendation and its stated requirement that existing
  subscriptions provide the reasoning. That is a product and economic change,
  not a refactor. It stands only if the
  [backend comparison](evaluation-harness.md#4-the-backend-comparison-replaces-the-single-exa-task)
  shows the hosted route produces defensible research on Robert's own tasks.
  Robert approves this reversal explicitly or the official-CLI worker moves to
  Phase 1.

The service owns **jobs, sources, public PDF acquisition/parsing, evidence,
reports, exports, notifications, and the research quality policy**. An external
agent or hosted research API owns investigation strategy and synthesis. The
service does not run the reasoning loop, but it decides what a run must contain
before its report is labelled anything other than a draft
([quality protocol](research-quality-protocol.md)). It enforces the limits a
backend can honour and makes visible the ones it cannot
([capability contract](run-and-report-contracts.md#2-backend-capability-contract)).

Recommended first deployment:

| Component                         | Choice                                                                                                |
| --------------------------------- | ----------------------------------------------------------------------------------------------------- |
| HTTP API                          | Axum, Tokio, Serde, tracing                                                                           |
| Provider HTTP clients             | reqwest; only integrations actually needed                                                            |
| Persistence                       | SQLite on local persistent storage, accessed through SQLx                                             |
| Files                             | Private content-addressed source storage and versioned report artifacts                               |
| CLI                               | Rust/clap; shared protocol types, no duplicated business logic                                        |
| Durable execution                 | One worker process, control and heavy lanes, database-backed claims                                   |
| Research quality                  | Versioned execution protocol, material-claim rules, Draft/Reviewed labels                             |
| Evaluation                        | Golden set of 8–12 cases, rubric, hard gates; gates backend selection and releases                    |
| Source PDF parsing                | Pinned Python/Docling executable in a subprocess; scheduled after the first slice                     |
| PDF report export                 | Pinned Markdown → HTML → PDF renderer in a separate subprocess                                        |
| General retrieval                 | Exa search and contents, behind the acquisition ladder                                                |
| Academic retrieval                | arXiv adapter when the golden set's paper cases need it; OpenAlex after                               |
| First unattended research backend | Chosen by the backend comparison: Exa Agent and one official CLI are the candidates                   |
| Additional execution route        | The other candidate, if and when it earns a place                                                     |
| Notifications                     | Telegram completion, failure, and action-needed messages, each with a link to the private report page |
| Human access                      | Read-only report page behind exe.dev's private proxy                                                  |

Exa Agent is the first **candidate** because the account already exists and the
documented API offers asynchronous research and fixed effort levels. That is a
reason to make it cheap to test, not a reason to choose it. It is **separately
billed** and is not assumed equivalent to Perplexity. The
[backend comparison](evaluation-harness.md#4-the-backend-comparison-replaces-the-single-exa-task)
decides between it and one official CLI on Robert's own tasks. Parallel and
You.com are not integrated as a reflex if Exa fails; they go through the same
comparison if tried.

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

The taxonomy is correct; building three lifecycles at once is not. **V1 builds
service-managed research.** Tool-use endpoints are added when the chosen
execution route needs them (immediately, if the official CLI wins the backend
comparison; later, if a hosted backend does its own retrieval). Client-led
research is reduced to **report import** in v1: an agent that did its own
research produces a finished
[report envelope](run-and-report-contracts.md#5-report-envelope-version-1) with
evidence and imports it; the service validates, labels, and exports. The
incremental evidence-attach lifecycle is designed when there is a demonstrated
need to hand unfinished investigations between harnesses.

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

`--depth` expresses intent; `--max-duration` and `--max-cost` express limits
with the semantics in
[budget semantics](run-and-report-contracts.md#3-budget-semantics). A hard limit
the backend cannot honour is rejected unless the request carries
`--accept-weaker-limits`. Backend selection must be explicit or come from a
visible configured default—never silently choose separately billed reasoning.

Operations the workflows imply but this table omits (clarification answers,
spend approval, reconciliation of `unknown`, follow-up runs, report import,
source-job status, backend listing, standing context, operator summary) are
specified in [API additions](run-and-report-contracts.md#6-api-additions).
`research run` also accepts `--follow-up RUN_ID`, `--clarify ask|assume|hold`,
and `--refresh`.

## 4. Data model

Keep the first schema small and relational. Large extracts and documents are
files, not database blobs.

| Record          | Essential contents                                                                                                                                                                                                                                 |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `runs`          | Brief (including `scope` and `classification`), mode (`managed`/`imported`), backend and the capability contract version it ran under, depth, limits, `follow_up_of`, context version, instruction version, the state dimensions below, timestamps |
| `jobs`          | Run/source reference, task kind, lane, state, attempt, attempt epoch, lease owner/expiry, retry time, external task ID and idempotency key, error                                                                                                  |
| `sources`       | Canonical URL, aliases, title, DOI/arXiv identity where available, origin group, freshness class                                                                                                                                                   |
| `acquisitions`  | Source ID, actual retrieval time, final URL, HTTP status and headers of interest, raw content hash/path, `content_kind` (`raw`, `provider_extract`, `model_summary`), access level, acquisition rung and failure reason                            |
| `extractions`   | Acquisition ID, extractor and `extraction_version`, options, text/layout path, confidence, warnings. One acquisition can have several extractions; locators bind to one                                                                            |
| `searches`      | Run, backend, query, time, result count (including zero), returned URLs, and per-URL exclusion reason                                                                                                                                              |
| `run_sources`   | Run, acquisition, discovery provenance (search ID or agent-cited), inclusion or exclusion reason, inherited-from run if a follow-up                                                                                                                |
| `evidence`      | Run, extraction, `relation`, element/page/section locator, quoted text, normalisation used, mechanical check result                                                                                                                                |
| `reports`       | Run, immutable revision, `supersedes`, envelope path, current computed label, review level, artifact manifest                                                                                                                                      |
| `assessments`   | Report revision, time, policy version, computed label, reasons. Append-only                                                                                                                                                                        |
| `corrections`   | Report revision corrected, new revision, claim IDs, reason, who (agent or Robert), time                                                                                                                                                            |
| `context`       | Versioned standing user context, one profile per `scope`; runs reference the version they used                                                                                                                                                     |
| `spend`         | Run, reservation, estimated, reported, reconciled amounts; daily and monthly aggregates derived                                                                                                                                                    |
| `events`        | Run/job ID, sequence, type, timestamp, bounded structured payload                                                                                                                                                                                  |
| `notifications` | Run, revision, channel, unique key (run, revision, kind), state, attempts                                                                                                                                                                          |

The old `source_versions` record is split into `acquisitions` and `extractions`
because they change for different reasons. Re-extracting a PDF with a newer
Docling must not look like the page changed, and a provider's extract of a page
must never be mistaken for the page itself; `content_kind` makes that
distinction a stored fact rather than a convention.

Run state is not one column. `execution`, `blocked_reason`, `completeness`,
`review`, `label`, per-format artifact state, `notification`, and `spend` are
independent fields with the meanings in
[run state](run-and-report-contracts.md#1-run-state-is-several-fields-not-one-status).
A run whose execution `succeeded` can still be `partial`, `draft`, with a failed
PDF export and a pending notification, and the API says exactly that.

Task kinds initially cover research, fetch/parse, export, and notification. They
use the same durable executor rather than four independent queue systems, in two
lanes: a control lane (polling, cancellation, notification, heartbeats) that
heavy work can never starve, and a heavy lane (research, parsing, rendering)
with bounded concurrency, initially one.

Source identity, URL normalisation, origin groups, freshness classes, and cache
keys are defined in the
[source acquisition policy](source-acquisition-policy.md#3-source-identity-deduplication-independence).

Do not overwrite a source when a page changes. Create a new version. Preserve
the difference between **when we fetched a provider response** and any
**crawl/publication date supplied by the provider**. A fresh API call does not
prove fresh webpage content.

Source access level is explicit: `metadata_only`, `snippet`, `partial_text`, or
`full_text`. A cited URL imported from a hosted report remains metadata-only
until acquired. Fetching it afterward is our own verification step, not proof
that the provider read that exact version.

## 5. Evidence and report contract

Use a versioned `report.json` as the canonical report representation. It is an
**envelope around one authoritative Markdown body**, not a bespoke document
language: schema version, run and revision identity, the brief and applied
assumptions, a claim list with kinds (`observation`, `calculation`, `inference`,
`recommendation`) anchored into the Markdown by stable markers, the evidence
map, sources with access levels and origin groups, the completion block, review
records, the preserved provider-native output, and artifact hashes. The full
shape is in
[report envelope, version 1](run-and-report-contracts.md#5-report-envelope-version-1).
Markdown, HTML, and PDF are derived views of that same revision.

Structured blocks (comparison tables, paper metadata, ranked recommendations)
are optional additions with their own schema versions, introduced when an agent
needs cell-level access rather than up front. Do not force every task into a
vendor-comparison template. Unknowns are null/explicitly unavailable, not
invented defaults.

Every report carries a **Draft** or **Reviewed** label in the header of every
format. The service computes it from the run's completion, review level, and
open clarification questions; the worker cannot assert it. The rules are in the
[quality protocol](research-quality-protocol.md#7-completion-and-labelling).

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

Semantic review is not optional polish. The
[review levels](research-quality-protocol.md#6-review-levels) make
`material_claims_reviewed` a precondition for the Reviewed label, and the
[material-claim rules](research-quality-protocol.md#5-material-claims) define
what must be checked: every claim whose falsity would change a recommendation,
price, ranking, date, compatibility, or legal condition, and every comparison
table cell. Worker self-review with a recorded checklist satisfies this level
initially; the evaluation harness measures how often it misses, and that number
decides whether a second reviewer becomes the default on deeper runs.

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

This diagram describes the `execution` dimension only; completeness, review,
label, artifacts, and notification are separate fields (section 4).

`blocked` means action is needed, and each reason has exactly one resolving
operation: `reauth` (re-authenticate, then `resume`), `spend_decision`
(`approve-spend` with a new cap), `quota` (`resume` after the window, never an
account switch), `clarification` (`clarify` with answers; see the
[clarification policy](research-quality-protocol.md#3-clarification-policy)),
`reconcile` (below), and `disk`. `unknown` means an external action may have
happened but cannot yet be reconciled. Neither is silently treated as failure
eligible for blind resubmission.

`unknown` is resolved by an explicit `reconcile` operation with one of three
actions: **adopt** a provider task ID the worker found, **mark failed** when the
provider confirms nothing ran, or **resubmit** with an explicit acceptance of a
possibly duplicate charge. `resume` alone never resolves `unknown`. The worker
tries automatic reconciliation first using whatever the provider offers; only
inconclusive cases reach the user, and the Telegram message says in plain words
that a paid submission may exist.

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
8. **Publication is one transaction.** The report revision row, the artifact
   manifest, the `report_published` event, the current-revision pointer, and the
   notification outbox row are written in a single SQLite transaction after the
   immutable files exist on disk. A reader can never see a revision without its
   manifest, or a notification for a revision that does not exist.
9. **Notifications are unique per (run, revision, kind).** The outbox row's
   unique key is what stops a retried worker from sending "report ready" twice.
   Delivery is at-least-once to Telegram; the key makes duplicates a Telegram
   retry problem, not a data problem.
10. **The attempt epoch fences every write, not only completion.** Each claim
    increments the job's attempt epoch and hands the worker a credential bound
    to it. Report, evidence, artifact, and spend writes carry the credential and
    are rejected if the job's current epoch has moved on. Rule 1 protects
    completion; this protects the evidence a stale worker keeps writing after it
    lost the lease but before it noticed.
11. **A restored database starts in reconciliation mode.** After a restore from
    backup, the worker makes no paid submissions until every job that was
    `running`, `unknown`, or awaiting collection at backup time has been
    reconciled against the provider and the artifact store. Restores are where
    duplicate charges and orphaned tasks come from, and the
    [runbook](operations-runbook.md#4-backup-restore-and-what-recovery-means)
    describes the procedure.

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

Evaluate against the **first slice's** requirement, not the four-kind universal
executor: one active managed run, persisted stage and external task ID, the
submission-ambiguity handling above, report publication, and a notification
outbox, in two lanes. Apalis solves scheduling and retry mechanics; it does not
solve provider reconciliation or publication atomicity, which are application
code either way. Decision rule: Apalis if the pinned version handles the
mechanics without a competing lifecycle; a narrow SQLx coordinator if
integrating it adds more translation than it removes. Neither option is a
general queue framework.

## 7. PDF ingestion and worker isolation

Ordinary web pages, not PDFs, are where stale or partial content most often
invalidates a report. How pages are acquired, when a live fetch is mandatory,
when a hosted extractor is used, and what happens when a source cannot be read
is defined in the
[source acquisition policy](source-acquisition-policy.md#1-acquisition-ladder).
This section covers rich PDF parsing, which the delivery plan schedules after
the first slice is in daily use; the contract below stands when it arrives.

Use Docling for the first rich parser, not a cascade of three untested parsers.
Prefer accessible XML/HTML full text when it preserves the information needed,
but keep academic metadata separate from full-text acquisition. `full_text`
records how much was obtained, not that the extraction is faithful; source
versions also carry extraction confidence and warnings, and material claims from
tables or multi-column layouts on a warned source require confirmation against
the original rendering
([policy §2](source-acquisition-policy.md#2-full_text-does-not-mean-correctly-extracted)).

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
spawning a subprocess alone does not impose a memory limit.

Parser and renderer children run with **no network and no credentials**: a
dedicated system user with no access to the service's environment, secrets
directory, or database, and outbound networking blocked at the systemd unit
(`IPAddressDeny=any` or a network namespace; **verify** which the exe.dev VM
supports). The parent hands them exactly one input file and one output
directory. A PDF from the open web is untrusted input; the process that parses
it must not be able to reach the API key that submitted the run.

The completion manifest a child writes is validated before anything is
published: every listed path must resolve inside the output directory (no
symlinks, no `..`, no absolute paths), must not exceed the declared size limit,
must be closed and not still growing, and must hash to the value the manifest
states. A manifest that fails any check fails the job with a distinguishable
error; nothing from that directory is referenced from the database.

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

### What every backend must satisfy

Whichever route runs the investigation, the service checks its output against
the [execution protocol](research-quality-protocol.md#4-execution-protocol): a
framed brief, evidence captured before summary, material claims traced to
acquired sources, conflicts recorded, a completion block, and a review record.
For an official CLI the protocol is the versioned instruction file. For a hosted
backend it is an acceptance contract: the service acquires the sources behind
material claims itself, runs the mechanical checks, and labels the report Draft
if the backend's citations do not support what it wrote.

### Hosted backend

Implement submit, inspect/poll, retrieve results, and cancellation if actually
supported. Store raw output and honest provenance. Backend capability
differences remain visible: not every backend supports hard deadlines, cost
ceilings, cancellation, or structured reports. Each adapter declares these in a
static
[capability contract](run-and-report-contracts.md#2-backend-capability-contract);
a request for a hard limit the backend declares as best-effort or unsupported is
rejected unless the client explicitly accepts weaker limits. Nothing is silently
downgraded.

### Official agent CLI worker

Add one of Claude Code/Codex, authenticated through its own supported flow, in
Phase 1 if it wins the backend comparison and in Phase 3 otherwise. It calls the
research CLI for retrieval and submits the final report to the API.

"A dedicated job workspace" is not isolation. The worker runs as a separate OS
user with its own HOME, an allow-listed child environment in which
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, Exa, and Telegram secrets are absent by
construction, a run-scoped service credential minted per attempt, a pinned CLI
version with a canary upgrade path, and the failure tests (auth expiry, quota,
permission prompt, hang, malformed output, wrong billing mode) listed in the
[runbook](operations-runbook.md#3-agent-cli-worker-isolation). It cannot
administer providers, inspect unrelated jobs, expose credentials, or recursively
create managed research jobs. The model CLI's own credential is necessarily a
separate security consideration; use its sandbox and isolate it from
parsing/rendering.

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

The flags have defined semantics
([budget semantics](run-and-report-contracts.md#3-budget-semantics)): a per-run
cap reserved against configured daily and monthly caps and reconciled on
completion; standalone search and read operations spend against the same daily
cap; queue time excluded from `--max-duration`; the last 15–20% of the duration
budget reserved for self-review and publication; unknown spend shown as its own
line, never as savings.

## 9. Access, notifications, and observability

Single owner, private service. Agents use bearer authentication over a private
connection/SSH tunnel or HTTPS reverse proxy. Do not expose raw worker
endpoints. Keep configured provider credentials out of logs, DB exports, CLI
output and report artifacts.

The human reads reports on a phone. A bearer token over SSH is not a phone
workflow. The service serves a small **read-only report page** per run behind
exe.dev's private, browser-authenticated proxy, and every Telegram notification
links to it. No token in the URL; tap-to-read tested on Robert's phone is an
acceptance criterion for the first slice. Details, including how the service
confirms a request came through the proxy, are in the
[runbook](operations-runbook.md#5-phone-workflow).

Standing user context (locale, household and professional facts, defaults) is
stored in the service, versioned, applied to briefs, and recorded on every
report as applied assumptions
([quality protocol §2](research-quality-protocol.md#2-standing-user-context)).
It is never sent to Telegram.

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

Logs are for debugging. Daily operation needs one screen: recent runs with
state, completeness, label, and last progress; anything blocked or unknown with
its reason and the one command that resolves it; spend today and this month
against caps; worker health and free disk; last backup and last restore
rehearsal. `GET /v1/summary` backs both `research dashboard` and the report page
index. An external heartbeat alerts by Telegram when the service is unreachable,
because a dead VM cannot report its own death
([runbook §6](operations-runbook.md#6-operator-view)).

## 10. First-version scope and delivery order

The original plan built retrieval, PDF parsing, and arXiv (Phase 1) before any
managed report existed (Phase 2). If a hosted backend wins the comparison it
does its own retrieval and most of that Phase 1 goes unused by the first useful
report. The order below builds one complete vertical slice to the phone first
and lets the platform grow from failures observed in daily use.

**Phase 0 — evaluate before building**

- Write the quality rubric, the first golden cases, and the Draft/Reviewed
  policy ([evaluation harness](evaluation-harness.md)). Score the Perplexity
  homeschool report against it.
- Run the
  [backend comparison](evaluation-harness.md#4-the-backend-comparison-replaces-the-single-exa-task):
  Exa Agent at two effort levels versus one official CLI, on every golden case,
  repeated cases three times, under an explicitly authorised spending ceiling.
  Record quality, time, cost, quota events, and self-review misses. Decide the
  first execution route with the stated decision rule.
- Build the **thin evidence loop** the comparison needs anyway: Exa search and
  contents behind `POST /v1/search` and `POST /v1/sources`, the acquisition and
  extraction store, the envelope validator, and `POST /v1/reports/import`. This
  is the evaluation infrastructure for the official-CLI candidate, it is the
  only way the CLI candidate can be scored on the same evidence rules as the
  hosted one, and Claude Code can use it on real work from the day it exists. It
  is deliberately small: no managed execution, no worker, no phone page.
- Verify the Exa capability contract against Robert's account, not the docs:
  whether `grounding` appears on every run at each effort, whether stopping a
  task retains partial output and at which efforts, whether ZDR is enabled and
  what it does to result collection, and how fixed-effort pricing interacts with
  `--max-cost`
  ([contract](run-and-report-contracts.md#2-backend-capability-contract)).
- Measure on the actual exe.dev VM: memory and elapsed time for the PDF renderer
  on a table-heavy sample, and for Docling on a small PDF set even though
  Docling ships later, so the VM size is chosen with real numbers. Then run the
  **mixed-load test**: one hosted poll loop, one renderer job, one Docling job,
  and one Telegram retry at the same time on the chosen VM size, and confirm the
  control lane's latency stays within its budget while the heavy lane is under
  memory pressure. A VM size chosen from single-job measurements is a guess
  about the case that matters.
- Test the queue candidate against the first slice's recovery requirements
  (section 6) and choose.
- Approve or reject the hosted-first reversal with the comparison results in
  hand and record it in [decisions](decisions.md). The ranked objective and the
  operating numbers were accepted on September 14, 2026 (D-001, D-015).

**Phase 1 — one vertical slice in daily use**

- Rust API and CLI: submit, status, report retrieval, find, evidence inspection,
  clarify, approve-spend, reconcile, cancel, correct. Bearer auth for agents.
  Migrations. Artifact store (extends the Phase 0 loop; nothing from Phase 0 is
  thrown away).
- The chosen backend adapter with its verified capability contract; if the
  official CLI won, the isolated worker and the minimum retrieval tools it
  needs.
- SQLite run state with all dimensions, atomic spend reservation before
  submission, submission idempotency, ambiguous-submission reconciliation,
  transactional publication, attempt-epoch fencing, two-lane worker.
- Standing user context per scope; briefs with `scope`, `classification`, and
  applied assumptions recorded; classification-based rejection before any
  transmission.
- Acquisition of the sources behind material claims through the ladder (provider
  contents, live fetch, one hosted extractor); mechanical checks; computed
  label.
- Report envelope v1, Markdown body, HTML and PDF from the same revision with
  the label in the header.
- Read-only report page behind exe.dev's private proxy; Telegram notifications
  linking to it, retried independently of research.
- Per-run, daily, and monthly caps.
- Hourly consistent backup off-VM, one restore rehearsal on a clean VM, external
  heartbeat.
- Pilot on real work for at least two weeks, scoring a sample of reports with
  the rubric, before cancelling the incumbent.

**Phase 2 — grow from observed failures**

- Follow-up runs with inherited evidence.
- Rich PDF parsing with Docling under the section 7 contract, when the pilot or
  golden set shows source PDFs blocking material claims.
- arXiv adapter with its documented pacing, when paper-heavy cases need it;
  OpenAlex after.
- Bundle export, `rerender`, `recheck`, and `refresh`, when the first policy
  change or source drift makes them necessary.
- Second-reviewer pass on `deep`/`extended` runs, if self-review miss rates
  justify it.

**Phase 3 — second route and expansion**

- The other execution route (hosted or official CLI), after the first proves
  useful and the second passes the same comparison.
- Alternate retrieval or hosted research backends when the harness shows a
  benefit.
- Incremental client-led evidence attachment, MCP adapter, richer structured
  report blocks, web UI beyond the read-only page.

Not in v1: multi-user accounts, public hosting, login-gated scraping,
distributed workers, vector database, autonomous multi-agent debates, whole-web
crawling, scheduled monitoring, or a new general-purpose agent framework.

## 11. Acceptance criteria

Quality:

- No golden case fails a hard gate (fabricated quotation or source, unsupported
  decision-changing recommendation, material arithmetic error, missing or
  contradicted label) on the chosen backend across repeated runs.
- Every material claim in a Reviewed report resolves to acquired evidence that
  supports it; every comparison table cell is a checked claim. Missing support
  is visible as Draft or `unsupported`, never hidden.
- Applied standing context and derived assumptions appear in the report; no
  silent Pennsylvania.
- Same-prompt evaluation against the Perplexity example and the other golden
  cases measures quality, elapsed time, retrieval spend, and quota impact. No
  claim of parity before that comparison, and the incumbent is scored with the
  same rubric.

Reliability:

- Hermes, Claude Code and Codex can call the same CLI without agent-specific
  server branches.
- Disconnecting a client does not lose an accepted managed job.
- Duplicate submissions return the same run; ambiguous provider submissions do
  not trigger blind paid duplicates; `unknown` is resolved only through
  `reconcile`.
- Restart during fetch, parsing, export and remote polling preserves honest
  state and recoverable outputs; stale workers cannot publish a newer attempt's
  result.
- A request for a hard limit the backend cannot honour is rejected unless weaker
  limits are explicitly accepted.
- A scanned page or bad table is not silently reported as a successful full-text
  extraction.
- JSON/Markdown/PDF represent one report revision; wide tables, footnotes, and
  the label header are visually inspected.
- Parser or renderer OOM/timeouts do not take down the API or delay cancellation
  and notification; cancellation leaves no untracked child processes.
- Telegram failure does not affect report availability or cause repeated
  research; a retried publication never sends the same (run, revision, kind)
  notification twice.
- Two simultaneous submissions that together exceed the remaining daily cap
  result in exactly one reservation and one provider call; the other is rejected
  or queued, and no provider call happens before its reservation commits.
- A worker whose lease has expired and been reclaimed cannot write evidence,
  artifacts, or spend for that job; the writes fail with the fencing error and
  the new attempt's output is the only one referenced.
- A child manifest listing a symlink, an absolute path, an oversize file, or a
  still-open file fails the job without publishing anything.

Operations:

- A run whose `classification` is not permitted for the chosen backend is
  rejected before any outbound request, and the test proves it with a recording
  proxy that observes no traffic.
- After a restore, the worker performs no paid submission until reconciliation
  mode has been cleared, and the runbook's restore test exercises a job that was
  `running` at backup time.
- Tap-to-read from a Telegram notification to the report page and PDF works on
  Robert's phone without a token in the URL.
- A restore from off-VM backup onto a clean VM completes within the recovery
  time objective, and every referenced artifact is present.
- An outage is reported by the external heartbeat within its window.
- The official-CLI failure scenarios in the runbook each produce the expected
  blocked or failed state, and never a billing-mode or account switch.

## References and remaining uncertainty

- [Design review, September 14, 2026](design-review-2026-09-14.md)
- [Decision records](decisions.md)
- Companion documents: [quality protocol](research-quality-protocol.md),
  [evaluation harness](evaluation-harness.md),
  [contracts](run-and-report-contracts.md),
  [acquisition policy](source-acquisition-policy.md),
  [operations runbook](operations-runbook.md)
- [Original landscape comparison](research-tool-landscape.md)
- [exe.dev HTTP proxies](https://exe.dev/docs/proxy)
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
memory budget, first backend, queue version and PDF renderer remain validation
choices—not assumed facts. The load-bearing assumptions and how each is
validated are tabulated in the
[design review](design-review-2026-09-14.md#assumptions-that-remain-unvalidated).
